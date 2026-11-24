"""Package the already-tested binaries; record source and bundled-file hashes."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def notices(target):
    metadata = json.loads(run('cargo', 'metadata', '--locked', '--format-version', '1',
                              '--filter-platform', target))
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    selected, pending = set(), list(metadata['workspace_members'])
    while pending:
        current = pending.pop()
        if current in selected:
            continue
        selected.add(current)
        pending.extend(d['pkg'] for d in nodes[current]['deps'])
    # Some published crates omit their Apache license file. Reuse the standard
    # unmodified Apache text from another crate in this locked dependency set.
    apache = next((p for item in packages.values()
                   for p in Path(item['manifest_path']).parent.glob('LICENSE-APACHE')
                   if p.is_file()), None)
    output = ['Third-party dependencies of SoundGeo',
              'License expressions are recorded from the locked Cargo metadata.',
              'The following notices include build and test dependencies.']
    inventory = []
    for ident in sorted(selected, key=lambda key: (packages[key]['name'], packages[key]['version'])):
        package = packages[ident]
        if package['source'] is None:
            continue
        directory = Path(package['manifest_path']).parent
        files = sorted(p for p in directory.rglob('*') if p.is_file() and (
            re.search(r'license|licence|copying|copyright|notice|^ofl\.|^ufl\.', p.name, re.I)
            or any(part.lower() in ('licenses', 'licences') for part in p.relative_to(directory).parts[:-1])
        ))
        text_files = []
        for path in files:
            if path.stat().st_size > 1_000_000:
                continue
            try:
                text_files.append((path.relative_to(directory).as_posix(), path.read_text(encoding='utf-8')))
            except UnicodeDecodeError:
                continue
        if not text_files and 'Apache-2.0' in (package['license'] or '') and apache:
            text_files = [('Apache-2.0 (standard text; selected license)', apache.read_text(encoding='utf-8'))]
        if not text_files and package['license'] == 'CC0-1.0':
            text_files = [('CC0-1.0', (ROOT / 'docs/third-party/CC0-1.0.txt').read_text(encoding='utf-8'))]
        if not text_files and (package['name'], package['version'], package['license']) == ('clipboard-win', '5.4.3', 'BSL-1.0'):
            text_files = [('LICENSE (from exact upstream revision)',
                           (ROOT / 'docs/third-party/clipboard-win-5.4.3-LICENSE.txt').read_text(encoding='utf-8'))]
        if not text_files:
            raise RuntimeError(f"Missing dependency license text: {package['name']} {package['version']}")
        inventory.append({key: package[key] for key in ('name', 'version', 'license', 'repository')})
        output.append(f"\n{'=' * 72}\n{package['name']} {package['version']}\nLicense: {package['license']}\nSource: {package['repository']}")
        for name, content in text_files:
            output.append(f'\n--- {name} ---\n{content}')
    # Rust distributes its standard library with the toolchain, outside Cargo's graph.
    rust_docs = Path(run('rustc', '--print', 'sysroot')) / 'share' / 'doc' / 'rust'
    rust_notices = [p for p in rust_docs.rglob('*') if p.is_file() and
                    (p.name in ('COPYRIGHT', 'LICENSE-APACHE', 'LICENSE-MIT') or 'licenses' in p.parts)]
    if not rust_notices and Path('/usr/share/doc/rustc/copyright').is_file():
        rust_docs = Path('/usr/share/doc/rustc')
        rust_notices = [rust_docs / 'copyright']
    if not rust_notices:
        raise RuntimeError('Rust toolchain copyright/license notices are unavailable')
    output.append('\nRust toolchain and standard library notices')
    for path in sorted(rust_notices):
        output.append(f'\n--- {path.relative_to(rust_docs).as_posix()} ---\n{path.read_text(encoding="utf-8")}')
    return '\n'.join(output) + '\n', inventory


def main():
    os.chdir(ROOT)
    if os.environ.get('CARGO_TARGET_DIR'):
        raise RuntimeError('Use the checkout default target directory')
    if run('git', 'status', '--porcelain'):
        raise RuntimeError('Packaging requires a clean source checkout')
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    commit = run('git', 'rev-parse', 'HEAD')
    target = run('rustc', '-vV').split('host: ', 1)[1].splitlines()[0]
    platform = {'x86_64-pc-windows-msvc': 'windows-x64',
                'x86_64-unknown-linux-gnu': 'linux-x64'}[target]
    suffix = '.exe' if sys.platform == 'win32' else ''
    dist = ROOT / 'dist'
    dist.mkdir(exist_ok=False)
    folder = dist / f'soundgeo-{version}-{platform}'
    folder.mkdir()
    for name in ('soundgeo', 'soundgeo-cli'):
        shutil.copy2(ROOT / 'target' / 'release' / (name + suffix), folder / (name + suffix))
    shutil.copy2(ROOT / 'README.md', folder / 'README.md')
    (folder / 'assets').mkdir()
    for path in (ROOT / 'assets').glob('*.txt'):
        shutil.copy2(path, folder / 'assets' / path.name)
    shutil.copytree(ROOT / 'docs', folder / 'docs')
    shutil.copytree(ROOT / 'validation', folder / 'validation',
                    ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
    text, dependencies = notices(target)
    (folder / 'THIRD_PARTY.txt').write_text(text, encoding='utf-8')
    manifest = {
        'version': version, 'source_commit': commit,
        'source_tree': run('git', 'rev-parse', 'HEAD^{tree}'), 'target': target,
        'rustc': run('rustc', '--version'), 'cargo': run('cargo', '--version'),
        'dependencies': dependencies,
        'files': {p.relative_to(folder).as_posix(): sha(p) for p in sorted(folder.rglob('*')) if p.is_file()},
    }
    (folder / 'BUILD.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    archive = dist / (folder.name + '.zip')
    with zipfile.ZipFile(archive, 'x', zipfile.ZIP_DEFLATED) as stream:
        for path in sorted(folder.rglob('*')):
            if path.is_file():
                stream.write(path, path.relative_to(dist).as_posix())
    (dist / f'{platform}.sha256').write_text(f'{sha(archive)}  {archive.name}\n', encoding='utf-8')
    print(json.dumps({'archive': archive.name, 'sha256': sha(archive), 'source_commit': commit,
                      'dependency_count': len(dependencies)}))


if __name__ == '__main__':
    main()
