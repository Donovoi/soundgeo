"""Prepare a pinned software OpenGL driver in the disposable Windows test VM."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import urllib.request

assert sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true'
directory = Path(os.environ['RUNNER_TEMP']) / 'soundgeo-mesa-26.3.1'
directory.mkdir(exist_ok=False)
archive = directory / 'mesa.7z'
url = 'https://github.com/pal1000/mesa-dist-win/releases/download/26.3.1/mesa3d-26.3.1-release-msvc.7z'
expected = '64e84cf13d67dd354640bf443ed0445e6a83afd06a5729f0750b3744d633b3f1'
with urllib.request.urlopen(url, timeout=60) as response, archive.open('xb') as output:
    shutil.copyfileobj(response, output)
assert hashlib.sha256(archive.read_bytes()).hexdigest() == expected, 'Test renderer checksum mismatch'
seven_zip = shutil.which('7z') or 'C:/Program Files/7-Zip/7z.exe'
subprocess.run([seven_zip, 'x', str(archive), '-o' + str(directory / 'driver'), '-y'], check=True, timeout=120)
dlls = directory / 'driver' / 'x64'
assert (dlls / 'opengl32.dll').is_file()
with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as output:
    output.write(f'SOUNDGEO_TEST_MESA_DIR={dlls}\n')
print(f'Prepared Mesa 26.3.1 test driver; SHA-256 {expected}. Not bundled in release archives.')
