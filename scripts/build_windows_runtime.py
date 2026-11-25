"""Build redistributable tools from pinned sources in a Windows VM."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VCPKG = '15f012269634c31d5f4b204f8ecd51681f130fa5'
MODEL = '87416418657359cb625c412a48b6e1d6d41c29bd'
MESA_URL = 'https://github.com/pal1000/mesa-dist-win/releases/download/26.3.1/mesa3d-26.3.1-release-mingw.7z'
MESA_SHA = 'c7d8c39f43a248a06debb5523d39facb58d0e029e0334ed49bfd23273e4809cf'
MODEL_SHA = '7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2'


def run(*args, **kwargs):
    subprocess.run(list(map(str, args)), check=True, **kwargs)


def download(url, path, expected=None):
    with urllib.request.urlopen(url, timeout=90) as response, path.open('xb') as output:
        shutil.copyfileobj(response, output)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if expected is not None and digest != expected:
        raise RuntimeError(f'Checksum mismatch for {path.name}')
    return digest


def main():
    assert sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true', 'Build inside a Windows VM'
    base = ROOT / 'runtime-build'
    base.mkdir(exist_ok=False)
    work = Path(os.environ['RUNNER_TEMP']) / 'soundgeo-runtime-build'
    work.mkdir(exist_ok=False)
    vcpkg = work / 'vcpkg'
    run('git', 'init', vcpkg)
    run('git', '-C', vcpkg, 'remote', 'add', 'origin', 'https://github.com/microsoft/vcpkg.git')
    run('git', '-C', vcpkg, 'fetch', '--depth=1', 'origin', VCPKG)
    run('git', '-C', vcpkg, 'checkout', '--detach', 'FETCH_HEAD')
    run('cmd', '/c', str(vcpkg / 'bootstrap-vcpkg.bat'), '-disableMetrics')
    installed = work / 'installed'
    env = dict(os.environ, VCPKG_MAX_CONCURRENCY='2', VCPKG_BINARY_SOURCES='clear',
               VCPKG_DISABLE_METRICS='1')
    run(vcpkg / 'vcpkg.exe', 'install', '--triplet=soundgeo-x64',
        '--overlay-triplets=' + str(ROOT / 'scripts/windows-runtime'),
        '--x-manifest-root=' + str(ROOT / 'scripts/windows-runtime'),
        '--x-install-root=' + str(installed), env=env)
    package = installed / 'soundgeo-x64'
    bundle = base / 'runtime'
    bundle.mkdir()
    for tool, port in [('ffmpeg', 'ffmpeg'), ('ffprobe', 'ffmpeg'), ('tesseract', 'tesseract')]:
        shutil.copy2(package / 'tools' / port / (tool + '.exe'), bundle / (tool + '.exe'))
    notices = bundle / 'runtime-notices'
    notices.mkdir()
    for path in sorted((package / 'share').glob('*/copyright')):
        shutil.copy2(path, notices / (path.parent.name + '.txt'))
    (bundle / 'tessdata').mkdir()
    download(f'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/{MODEL}/eng.traineddata',
             bundle / 'tessdata/eng.traineddata', MODEL_SHA)
    download(f'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/{MODEL}/LICENSE',
             notices / 'tessdata-fast-LICENSE.txt')
    mesa_archive = work / 'mesa.7z'
    download(MESA_URL, mesa_archive, MESA_SHA)
    seven = shutil.which('7z') or 'C:/Program Files/7-Zip/7z.exe'
    mesa = work / 'mesa'
    run(seven, 'x', mesa_archive, '-o' + str(mesa), '-y')
    # Keep Mesa isolated from the CLI tools. Only the desktop loads these DLLs.
    driver = bundle / 'graphics'
    driver.mkdir()
    # The dependency audit and VM startup check below determine the required set.
    for path in (mesa / 'x64').glob('*.dll'):
        shutil.copy2(path, driver / path.name)
    assert (driver / 'opengl32.dll').is_file()
    for path in mesa.rglob('*'):
        if path.is_file() and (path.suffix.lower() in ('.txt', '.md', '.rst') or
                               'license' in path.name.lower() or 'copying' in path.name.lower()):
            dest = notices / 'mesa' / path.relative_to(mesa)
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, dest)
    assert any((notices / 'mesa').rglob('*')), 'Mesa notices missing'
    versions = {}
    for tool in ('ffmpeg', 'ffprobe', 'tesseract'):
        args = [str(bundle / (tool + '.exe')), '--version' if tool == 'tesseract' else '-version']
        result = subprocess.run(args, check=True, capture_output=True, text=True)
        versions[tool] = result.stdout + result.stderr
    assert '--enable-gpl' not in versions['ffmpeg'] and '--enable-nonfree' not in versions['ffmpeg']
    (bundle / 'RUNTIME.json').write_text(json.dumps({
        'vcpkg_commit': VCPKG, 'model_commit': MODEL, 'model_sha256': MODEL_SHA,
        'mesa_url': MESA_URL, 'mesa_sha256': MESA_SHA, 'versions': versions,
    }, indent=2) + '\n', encoding='utf-8')
    # Publish exact patched sources plus the pinned build scripts alongside the EXE.
    # Disable vcpkg binary caches above so source trees are always available.
    sources = base / 'windows-runtime-sources.zip'
    with zipfile.ZipFile(sources, 'x', zipfile.ZIP_DEFLATED) as output:
        for part in ('ports', 'scripts', 'triplets'):
            for path in sorted((vcpkg / part).rglob('*')):
                if path.is_file():
                    output.write(path, 'vcpkg/' + path.relative_to(vcpkg).as_posix())
        for name in ('LICENSE.txt', 'bootstrap-vcpkg.bat', 'bootstrap-vcpkg.sh'):
            output.write(vcpkg / name, 'vcpkg/' + name)
        for directory in sorted((vcpkg / 'buildtrees').glob('*/src')):
            for path in sorted(directory.rglob('*')):
                if path.is_file() and '.git' not in path.relative_to(directory).parts:
                    output.write(path, 'patched-sources/' + path.relative_to(vcpkg / 'buildtrees').as_posix())
        for path in sorted((ROOT / 'scripts/windows-runtime').iterdir()):
            if path.is_file():
                output.write(path, 'soundgeo-build/scripts/windows-runtime/' + path.name)
        output.write(Path(__file__), 'soundgeo-build/scripts/build_windows_runtime.py')
        output.write(bundle / 'RUNTIME.json', 'RUNTIME.json')
    with zipfile.ZipFile(base / 'windows-runtime.zip', 'x', zipfile.ZIP_DEFLATED) as output:
        for path in sorted(bundle.rglob('*')):
            if path.is_file():
                output.write(path, path.relative_to(bundle).as_posix())
    print(json.dumps({'versions': versions, 'files': [str(p.relative_to(bundle)) for p in bundle.rglob('*') if p.is_file()],
                      'sources_size': sources.stat().st_size}, indent=2))


if __name__ == '__main__':
    main()
