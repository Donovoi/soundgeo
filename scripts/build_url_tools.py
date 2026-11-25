"""Build current, pinned downloaders in a Windows VM; keep licenses and sources."""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import venv
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def fetch(url, target, digest):
    if target.exists():
        assert hashlib.sha256(target.read_bytes()).hexdigest() == digest, target
        return target
    with urllib.request.urlopen(url, timeout=90) as response, target.open('xb') as out:
        shutil.copyfileobj(response, out)
    assert hashlib.sha256(target.read_bytes()).hexdigest() == digest, target
    return target


def run(*args, **kwargs):
    subprocess.run(list(map(str, args)), check=True, **kwargs)


def main():
    assert sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true', 'Build in a Windows VM'
    lock = json.loads((ROOT / 'scripts/url-tools-lock.json').read_text())
    assert sys.version.split()[0] == lock['python'], 'Use the pinned Python patch release'
    work = Path(os.environ['RUNNER_TEMP']) / 'soundgeo-url-tools'
    work.mkdir(exist_ok=False)
    downloads = work / 'downloads'; downloads.mkdir()
    bundle = work / 'bundle'; bundle.mkdir()
    notices = bundle / 'runtime-notices/url-tools'; notices.mkdir(parents=True)
    sources = work / 'sources'; sources.mkdir()
    artifacts = [(p['url'], downloads / p['url'].rsplit('/', 1)[1], p['sha256']) for p in lock['packages']]
    artifacts += [(p['url'], sources / p['filename'], p['sha256']) for entry in lock['packages'] for p in entry['sources']]
    artifacts += [(p['url'], sources / p['name'], p['sha256']) for p in lock['sources']]
    artifacts.append((lock['deno']['url'], downloads / 'deno.zip', lock['deno']['sha256']))
    # Hash-pinned independent build inputs; bounded transfer parallelism.
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        for future in [pool.submit(fetch, *args) for args in artifacts]: future.result()
    envdir = work / 'venv'; venv.create(envdir, with_pip=True)
    python = envdir / 'Scripts/python.exe'
    requirements = work / 'requirements.txt'
    requirements.write_text(''.join(f"{p['name']}=={p['version']} --hash=sha256:{p['sha256']}\n" for p in lock['packages']))
    run(python, '-m', 'pip', 'install', '--no-index', '--no-deps', '--require-hashes', '--find-links', downloads, '-r', requirements)
    run(python, '-m', 'pip', 'check')
    # Wheel notices also cover gallery-dl-only dependencies and fresh certificates.
    with (notices / 'PYTHON-PACKAGES.txt').open('w', encoding='utf-8') as output:
        for package in lock['packages']:
            output.write(f"\n{'=' * 72}\n{package['name']} {package['version']}\n")
            with zipfile.ZipFile(downloads / package['url'].rsplit('/', 1)[1]) as wheel:
                for name in wheel.namelist():
                    if any(word in name.lower() for word in ['license', 'copying', 'notice']):
                        output.write(f'\n--- {name} ---\n' + wheel.read(name).decode('utf-8', errors='replace'))
    for label, archive in [('yt', 'yt-dlp-source-2026.11.23.tar.gz'), ('gallery', 'gallery-dl-source-1.32.21.tar.gz'), ('aria', 'aria2-1.37.0.tar.xz')]:
        dest = work / label; dest.mkdir()
        with tarfile.open(sources / archive) as stream: stream.extractall(dest, filter='data')
    yt, = (work / 'yt').iterdir()
    gallery, = (work / 'gallery').iterdir()
    aria, = (work / 'aria').iterdir()
    run(python, 'devscripts/make_lazy_extractors.py', cwd=yt)
    run(python, '-m', 'bundle.pyinstaller', '--onefile', cwd=yt)
    shutil.copy2(yt / 'dist/yt-dlp.exe', bundle / 'yt-dlp.exe')
    shutil.copy2(yt / 'THIRD_PARTY_LICENSES.txt', notices / 'PYTHON-RUNTIME.txt')
    # Both tools use the same pinned environment, including curl_cffi and current yt-dlp.
    run(python, 'scripts/pyinstaller.py', cwd=gallery)
    shutil.copy2(gallery / 'dist/gallery-dl.exe', bundle / 'gallery-dl.exe')
    shutil.copy2(gallery / 'LICENSE', notices / 'gallery-dl-LICENSE.txt')
    # HTTP(S) is all SoundGeo needs. WinTLS/Windows DNS avoids shipping obsolete
    # copies of OpenSSL, Expat, SQLite, c-ares, GMP, zlib or libssh2 from the 2023 EXE.
    script = work / 'build-aria.sh'
    script.write_text('''set -eu
cd "$(cygpath -u "$1")"
export PATH=/mingw64/bin:/usr/bin
export ARIA2_STATIC=yes
./configure --host=x86_64-w64-mingw32 --build=x86_64-w64-mingw32 \\
 --with-wintls --without-openssl --without-gnutls --without-libgcrypt \\
 --without-libnettle --without-libgmp --without-libcares --without-libz \\
 --without-libxml2 --without-libexpat --without-sqlite3 --without-libssh2 \\
 --disable-bittorrent --disable-metalink --disable-websocket --disable-nls \\
 --disable-dependency-tracking CXXFLAGS="-O2" LDFLAGS="-static"
make -j2
strip src/aria2c.exe
''', encoding='utf-8', newline='\n')
    msys = Path(os.environ['MSYS2_ROOT'])
    bash = msys / 'usr/bin/bash.exe'
    run(bash, str(script).replace('\\', '/'), str(aria))
    shutil.copy2(aria / 'src/aria2c.exe', bundle / 'aria2c.exe')
    shutil.copy2(aria / 'COPYING', notices / 'aria2-COPYING.txt')
    mingw = msys / 'mingw64/share/licenses'
    assert mingw.is_dir()
    shutil.copytree(mingw, notices / 'mingw')
    with (notices / 'MINGW-PACKAGES.txt').open('wb') as output:
        run(bash, '-lc', 'pacman -Q', stdout=output)
    with zipfile.ZipFile(downloads / 'deno.zip') as stream:
        (bundle / 'deno.exe').write_bytes(stream.read('deno.exe'))
    with (notices / 'DENO-LICENSES.txt').open('wb') as output:
        run(bundle / 'deno.exe', 'license', stdout=output)
    versions = {}
    for tool in ('yt-dlp', 'gallery-dl', 'aria2c', 'deno'):
        versions[tool] = subprocess.check_output([str(bundle / (tool + '.exe')), '--version'], text=True).strip()
    targets = subprocess.check_output([str(bundle / 'yt-dlp.exe'), '--ignore-config', '--list-impersonate-targets'], text=True)
    assert 'curl_cffi' in targets and 'Chrome' in targets
    (bundle / 'URL-TOOLS.json').write_text(json.dumps({'lock': lock, 'versions': versions, 'impersonation_targets': targets}, indent=2) + '\n', encoding='utf-8')
    # Verify every DLL/PYD inside the PyInstaller payload, not just its bootloader.
    run(python, ROOT / 'scripts/audit_url_tools.py', bundle)
    destination = ROOT / 'url-tools-build'; destination.mkdir(exist_ok=False)
    with zipfile.ZipFile(destination / 'url-tools.zip', 'x', zipfile.ZIP_DEFLATED) as stream:
        for path in sorted(bundle.rglob('*')):
            if path.is_file(): stream.write(path, path.relative_to(bundle).as_posix())
    with zipfile.ZipFile(destination / 'url-tools-sources.zip', 'x', zipfile.ZIP_DEFLATED) as stream:
        for path in sorted(sources.iterdir()): stream.write(path, path.name)
        for path in (script, ROOT / 'scripts/build_url_tools.py', ROOT / 'scripts/url-tools-lock.json', requirements): stream.write(path, 'build/' + path.name)
        stream.write(notices / 'MINGW-PACKAGES.txt', 'build/MINGW-PACKAGES.txt')
    print(json.dumps(versions), flush=True)


if __name__ == '__main__': main()
