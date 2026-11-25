"""Publication guards, using a fake GitHub service with no network or uploads."""
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('publish_release.py')


class PublisherTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.cwd = Path.cwd()
        self.addCleanup(self.directory.cleanup)
        self.addCleanup(os.chdir, self.cwd)
        self.version = '0.3.0-alpha.2'
        self.commit = 'a' * 40
        self.tree = 'b' * 40
        self.release = None
        self.mutations = []
        for directory in ('scripts', '.github', 'docs/releases'):
            (self.root / directory).mkdir(parents=True)
        shutil.copy2(SCRIPT, self.root / 'scripts/publish_release.py')
        (self.root / '.github/release.json').write_text(json.dumps({'version': self.version, 'prerelease': True}))
        (self.root / 'Cargo.toml').write_text(f'[package]\nversion = "{self.version}"\n')
        (self.root / f'docs/releases/v{self.version}.md').write_text('Notes')
        for platform, runner in [('windows-x64', 'Windows'), ('linux-x64', 'Linux')]:
            folder = self.root / 'release-artifacts' / ('release-' + runner)
            folder.mkdir(parents=True)
            extension = '.exe' if runner == 'Windows' else '.zip'
            archive = folder / f'soundgeo-{self.version}-{platform}{extension}'
            archive.write_bytes(b'archive fixture')
            receipt = {'passed': True, 'file_hashes_verified': True,
                       'checks': ['desktop-visible-window', 'bundled-video-ocr', 'windows-inbox-or-bundled-dlls-only', 'bundled-url-intake-and-parallel-downloads', 'bundled-local-speech-and-translation'], 'source_commit': self.commit,
                       'source_tree': self.tree, 'version': self.version,
                       'archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest()}
            if runner == 'Windows':
                sources = folder / f'soundgeo-{self.version}-windows-runtime-sources.zip'
                sources.write_bytes(b'sources fixture')
                receipt['additional_assets'] = [{'name': sources.name, 'sha256': hashlib.sha256(sources.read_bytes()).hexdigest()}]
            (folder / f'validation-{platform}.json').write_text(json.dumps(receipt))

    def output(self, command, **kwargs):
        args = list(map(str, command[1:]))
        if args[:3] == ['api', '--paginate', '--slurp']:
            return json.dumps([[self.release] if self.release else []])
        if args[0] == 'api' and '/git/commits/' in args[1]:
            return self.tree
        if args[0] == 'api' and '/releases/123' in args[1]:
            return json.dumps(self.release)
        assert args[0] == 'release', args
        action = args[1]
        self.mutations.append(action)
        if action == 'create':
            assert '--draft' in args and '--prerelease' in args
            self.release = {'id': 123, 'tag_name': 'v' + self.version, 'draft': True,
                            'prerelease': True, 'target_commitish': self.commit, 'assets': [],
                            'html_url': 'https://example.invalid/release'}
        elif action == 'upload':
            assert self.release['draft']
            paths = [Path(p) for p in args[args.index('--clobber') + 1:]]
            self.release['assets'] = [{'name': p.name, 'size': p.stat().st_size,
                                      'digest': 'sha256:' + hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]
        elif action == 'edit':
            assert self.release['draft'] and len(self.release['assets']) == 6
            self.release['draft'] = False
        else:
            raise AssertionError(args)
        return ''

    def execute(self):
        env = {'GITHUB_REPOSITORY': 'test/soundgeo', 'GITHUB_SHA': self.commit,
               'GITHUB_EVENT_NAME': 'push', 'GITHUB_REF': 'refs/heads/main'}
        absent_ref = subprocess.CompletedProcess([], 1, '', 'HTTP 404')
        with patch.dict(os.environ, env), patch('subprocess.check_output', side_effect=self.output), \
                patch('subprocess.run', return_value=absent_ref):
            runpy.run_path(str(self.root / 'scripts/publish_release.py'), run_name='__main__')

    def test_publishes_only_after_draft_assets_verified(self):
        self.execute()
        self.assertEqual(self.mutations, ['create', 'upload', 'edit'])
        self.assertFalse(self.release['draft'])

    def test_rejects_corrupted_archive_before_any_mutation(self):
        next(self.root.rglob('*.zip')).write_bytes(b'corrupt')
        with self.assertRaises(AssertionError):
            self.execute()
        self.assertEqual(self.mutations, [])

    def test_requires_url_intake_qualification_before_publication(self):
        receipt = self.root / 'release-artifacts/release-Windows/validation-windows-x64.json'
        value = json.loads(receipt.read_text())
        value['checks'].remove('bundled-url-intake-and-parallel-downloads')
        receipt.write_text(json.dumps(value))
        with self.assertRaises(AssertionError): self.execute()
        self.assertEqual(self.mutations, [])

    def test_rejects_foreign_draft_without_overwriting_assets(self):
        self.release = {'tag_name': 'v' + self.version, 'draft': True,
                        'prerelease': True, 'target_commitish': 'c' * 40}
        with self.assertRaises(AssertionError):
            self.execute()
        self.assertEqual(self.mutations, [])

    def test_requires_speech_qualification_before_publication(self):
        receipt = self.root / 'release-artifacts/release-Linux/validation-linux-x64.json'
        value = json.loads(receipt.read_text())
        value['checks'].remove('bundled-local-speech-and-translation')
        receipt.write_text(json.dumps(value))
        with self.assertRaises(AssertionError): self.execute()
        self.assertEqual(self.mutations, [])

    def test_preserves_published_release(self):
        self.release = {'tag_name': 'v' + self.version, 'draft': False}
        with self.assertRaises(SystemExit) as result:
            self.execute()
        self.assertEqual(result.exception.code, 0)
        self.assertEqual(self.mutations, [])

    def test_wrong_uploaded_digest_leaves_release_unpublished(self):
        original = self.output
        def corrupt(command, **kwargs):
            result = original(command, **kwargs)
            if command[1:3] == ['release', 'upload']:
                self.release['assets'][0]['digest'] = 'sha256:' + '0' * 64
            return result
        self.output = corrupt
        with self.assertRaises(AssertionError):
            self.execute()
        self.assertEqual(self.mutations, ['create', 'upload'])
        self.assertTrue(self.release['draft'])

    def test_rejects_corrupted_corresponding_sources(self):
        next(self.root.rglob('*-runtime-sources.zip')).write_bytes(b'corrupt sources')
        with self.assertRaises(AssertionError):
            self.execute()
        self.assertEqual(self.mutations, [])

    def test_waits_for_new_draft_visibility(self):
        original = self.output
        hidden = [False]
        def eventual(command, **kwargs):
            if command[1:4] == ['api', '--paginate', '--slurp'] and self.release and not hidden[0]:
                hidden[0] = True
                return '[[]]'
            return original(command, **kwargs)
        self.output = eventual
        with patch('time.sleep'):
            self.execute()
        self.assertEqual(self.mutations, ['create', 'upload', 'edit'])
        self.assertTrue(hidden[0])


if __name__ == '__main__':
    unittest.main()
