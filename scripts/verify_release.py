"""Exercise an extracted release in a VM, including its desktop window."""
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import time
import wave
import zipfile

root = Path(__file__).resolve().parents[1]
archive, = (root / 'dist').glob('*.zip')
destination = root / 'release-check'
destination.mkdir(exist_ok=False)
with zipfile.ZipFile(archive) as stream:
    for entry in stream.infolist():
        path = (destination / entry.filename).resolve()
        if not path.is_relative_to(destination.resolve()):
            raise RuntimeError('Archive contains an escaping path')
    stream.extractall(destination)
folder, = destination.iterdir()
build = json.loads((folder / 'BUILD.json').read_text(encoding='utf-8'))
assert build['source_commit'] == subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
actual = {p.relative_to(folder).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
          for p in folder.rglob('*') if p.is_file() and p.name != 'BUILD.json'}
assert actual == build['files'], 'Extracted archive contents do not match the manifest'
suffix = '.exe' if sys.platform == 'win32' else ''
cli, gui = (folder / (name + suffix) for name in ('soundgeo-cli', 'soundgeo'))
if sys.platform != 'win32':
    cli.chmod(0o755)
    gui.chmod(0o755)
checks = []


def run(name, args, expected=0, timeout=240):
    result = subprocess.run([str(cli), *map(str, args)], cwd=destination, capture_output=True, text=True, timeout=timeout)
    (destination / (name + '.log')).write_text(result.stdout + result.stderr, encoding='utf-8')
    assert result.returncode == expected, (name, result.returncode, result.stderr)
    checks.append(name)
    return result.stdout


assert build['version'] in run('version', ['--version'])
assert 'check-audio' in run('help', ['--help'])
silent = destination / 'silence.wav'
with wave.open(str(silent), 'wb') as stream:
    stream.setnchannels(1)
    stream.setsampwidth(2)
    stream.setframerate(8000)
    stream.writeframes(b'\0' * (20 * 8000 * 2))
report = destination / 'silence.json'
run('audio-intake', ['check-audio', '--input', silent, '--output', report, '--require-searchable'], 3)
data = json.loads(report.read_text(encoding='utf-8'))
assert data['status'] == 'insufficient_evidence' and data['offline'] is True
preserved = report.read_bytes()
run('preserve-report', ['check-audio', '--input', silent, '--output', report], 1)
assert report.read_bytes() == preserved
demo = destination / 'demo'
run('demo', ['demo', '--output', demo])
run('analyze-demo', ['--offline', 'analyze', '--config', demo / 'job.json',
                     '--search-threads', '2', '--output', demo / 'report.json', '--require-unambiguous'])
data = json.loads((demo / 'report.json').read_text(encoding='utf-8'))
assert data['status'] == 'candidates_found'
point = data['candidates'][0]
distance = math.hypot((point['latitude'] - 35.0) * 111320,
                      (point['longitude'] + 110.0) * 111320 * math.cos(math.radians(35)))
assert distance < 1000, f'Synthetic result is {distance:.1f} m from its known position'
assert abs(point['recording_start_unix'] - 1788861600) < 30
checks.append('synthetic-location-and-clock')


def has_window(pid):
    if sys.platform == 'win32':
        found = []
        user32 = ctypes.windll.user32
        callback_type = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
        user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
        user32.IsWindowVisible.argtypes = [ctypes.c_void_p]
        user32.EnumWindows.argtypes = [callback_type, ctypes.c_void_p]
        @callback_type
        def callback(handle, _):
            owner = ctypes.c_ulong()
            user32.GetWindowThreadProcessId(handle, ctypes.byref(owner))
            if owner.value == pid and user32.IsWindowVisible(handle):
                found.append(handle)
            return True
        user32.EnumWindows(callback, 0)
        return bool(found)
    result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(pid)], capture_output=True)
    return result.returncode == 0 and bool(result.stdout.strip())


with (destination / 'desktop.log').open('wb') as log:
    process = subprocess.Popen([str(gui)], cwd=destination, stdout=log, stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 30
        while not has_window(process.pid):
            assert process.poll() is None, 'Desktop application exited before creating a visible window'
            if time.monotonic() > deadline:
                raise RuntimeError('Desktop window did not appear')
            time.sleep(0.25)
        time.sleep(2)
        assert process.poll() is None, 'Desktop application exited after opening'
        checks.append('desktop-visible-window')
    finally:
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=15)
        print((destination / 'desktop.log').read_text(encoding='utf-8', errors='replace'))
platform = 'windows-x64' if sys.platform == 'win32' else 'linux-x64'
receipt = {'version': build['version'], 'source_commit': build['source_commit'],
           'source_tree': build['source_tree'], 'target': build['target'],
           'archive': archive.name, 'archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
           'file_hashes_verified': True, 'checks': checks,
           'scope': 'Package smoke checks using synthetic/silent inputs; no new real-world accuracy measurement',
           'passed': True}
(root / 'dist' / f'validation-{platform}.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
print(json.dumps(receipt, indent=2))
