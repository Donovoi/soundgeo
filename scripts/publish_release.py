"""Publish only the requested version, after the same workflow's VM checks pass."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib
import time

root = Path(__file__).resolve().parents[1]
os.chdir(root)
request = json.loads((root / '.github' / 'release.json').read_text())
version = request['version']
tag = 'v' + version
repo = os.environ['GITHUB_REPOSITORY']
commit = os.environ['GITHUB_SHA']
assert os.environ['GITHUB_EVENT_NAME'] == 'push' and os.environ['GITHUB_REF'] == 'refs/heads/main'


def gh(*args):
    return subprocess.check_output(['gh', *map(str, args)], text=True).strip()


def find_release():
    # The tag endpoint returns published releases; list releases also finds
    # drafts whose tag has not been created yet. Paginate for older drafts.
    pages = json.loads(gh('api', '--paginate', '--slurp', f'repos/{repo}/releases?per_page=100'))
    matches = [release for page in pages for release in page if release['tag_name'] == tag]
    assert len(matches) <= 1, 'Multiple releases use the requested tag'
    return matches[0] if matches else None


draft = find_release()
if draft is not None:
    if not draft['draft']:
        print(f'{tag} is already published; leaving its tag and assets unchanged.')
        raise SystemExit(0)
    assert draft['target_commitish'] == commit, 'An existing draft belongs to another source revision'
    assert draft['prerelease'] == request['prerelease'], 'Existing draft has a different release classification'

assert version == tomllib.loads((root / 'Cargo.toml').read_text())['package']['version']
assert isinstance(request['prerelease'], bool)
notes = root / 'docs' / 'releases' / (tag + '.md')
assert notes.is_file()
assets = []
for platform, runner in [('windows-x64', 'Windows'), ('linux-x64', 'Linux')]:
    folder = root / 'release-artifacts' / ('release-' + runner)
    extension = '.exe' if runner == 'Windows' else '.zip'
    archive = folder / f'soundgeo-{version}-{platform}{extension}'
    receipt = folder / f'validation-{platform}.json'
    check = json.loads(receipt.read_text())
    assert check['passed'] and check['file_hashes_verified'] and 'desktop-visible-window' in check['checks']
    assert check['source_commit'] == commit and check['version'] == version
    assert check['source_tree'] == gh('api', f'repos/{repo}/git/commits/{commit}', '--jq', '.tree.sha')
    assert check['archive_sha256'] == hashlib.sha256(archive.read_bytes()).hexdigest()
    assets.extend([archive, receipt])
    if runner == 'Windows':
        assert 'bundled-video-ocr' in check['checks'] and 'windows-inbox-or-bundled-dlls-only' in check['checks']
        source_name = f'soundgeo-{version}-windows-runtime-sources.zip'
        source, = check['additional_assets']
        assert source['name'] == source_name
        sources = folder / source_name
        assert source['sha256'] == hashlib.sha256(sources.read_bytes()).hexdigest()
        assets.append(sources)
checksums = root / 'release-artifacts' / 'SHA256SUMS.txt'
checksums.write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in assets), encoding='utf-8')
assets.append(checksums)
if draft is None:
    # Refuse to adopt a pre-existing tag whose source has not been qualified here.
    ref = subprocess.run(['gh', 'api', f'repos/{repo}/git/ref/tags/{tag}'], capture_output=True, text=True)
    if ref.returncode == 0:
        assert json.loads(ref.stdout)['object']['sha'] == commit, 'Release tag points to another commit'
    elif '404' not in ref.stderr:
        raise RuntimeError(ref.stderr)
    args = ['release', 'create', tag, '--repo', repo, '--target', commit,
            '--title', f'SoundGeo {version}', '--notes-file', notes, '--draft']
    if request['prerelease']:
        args.append('--prerelease')
    gh(*args)
    for attempt in range(6):
        draft = find_release()
        if draft is not None:
            break
        time.sleep(2)
    assert draft is not None and draft['draft'] and draft['target_commitish'] == commit
# Only an unpublished draft from the exact checked revision can be resumed.
gh('release', 'upload', tag, '--repo', repo, '--clobber', *assets)
uploaded = json.loads(gh('api', f'repos/{repo}/releases/{draft["id"]}'))
assert uploaded['draft'] and uploaded['target_commitish'] == commit
by_name = {a['name']: a for a in uploaded['assets']}
assert set(by_name) == {p.name for p in assets}, 'Unexpected assets in draft'
for path in assets:
    assert by_name[path.name]['size'] == path.stat().st_size
    assert by_name[path.name]['digest'] == 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()
gh('release', 'edit', tag, '--repo', repo, '--draft=false', '--latest=false')
published = json.loads(gh('api', f'repos/{repo}/releases/{draft["id"]}'))
assert not published['draft'] and published['prerelease'] == request['prerelease']
print(published['html_url'])
