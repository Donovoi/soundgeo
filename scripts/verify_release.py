"""Exercise an extracted release in a VM, including its desktop window."""
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import wave
import zipfile
from pe_dependencies import audit

root = Path(__file__).resolve().parents[1]
source_commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
archive, = (root / 'dist').glob('*.exe' if sys.platform == 'win32' else '*-linux-x64.zip')
destination = root / 'release-check'
destination.mkdir(exist_ok=False)
checks = []
if sys.platform == 'win32':
    # No developer tools, Chocolatey, FFmpeg, Tesseract, or Mesa on the app PATH.
    # PE imports are independently checked against Windows inbox DLLs below.
    os.environ['PATH'] = os.path.join(os.environ['SystemRoot'], 'System32')
    for variable in ('TESSDATA_PREFIX', 'SOUNDGEO_TEST_MESA_DIR', 'GALLIUM_DRIVER', 'LIBGL_ALWAYS_SOFTWARE'):
        os.environ.pop(variable, None)
    isolated = destination / 'Windows profile with spaces'
    isolated.mkdir()
    os.environ['LOCALAPPDATA'] = str(isolated)
    unpacked = destination / 'Extracted app with spaces'
    result = subprocess.run([str(archive), '--portable-extract', str(unpacked)],
                            capture_output=True, text=True, timeout=180)
    assert result.returncode == 0, result.stderr
    folder, = unpacked.iterdir()
    checks.append('single-exe-extraction-without-external-tools')
    audit(folder)
    checks.append('windows-inbox-or-bundled-dlls-only')
    existing = subprocess.run([str(archive), '--portable-extract', str(unpacked)],
                              capture_output=True, text=True, timeout=30)
    assert existing.returncode == 1 and 'already exists' in existing.stderr
    checks.append('preserve-existing-extraction')
    damaged = destination / 'damaged.exe'
    shutil.copy2(archive, damaged)
    with damaged.open('r+b') as stream:
        stream.seek(-1, 2)
        byte = stream.read(1)
        stream.seek(-1, 2)
        stream.write(bytes([byte[0] ^ 1]))
    result = subprocess.run([str(damaged), '--version'], capture_output=True, text=True, timeout=60)
    assert result.returncode == 1 and 'damaged' in result.stderr
    checks.append('damaged-download-rejected')
else:
    with zipfile.ZipFile(archive) as stream:
        for entry in stream.infolist():
            path = (destination / entry.filename).resolve()
            if not path.is_relative_to(destination.resolve()):
                raise RuntimeError('Archive contains an escaping path')
        stream.extractall(destination)
    folder, = destination.iterdir()
build = json.loads((folder / 'BUILD.json').read_text(encoding='utf-8'))
assert build['source_commit'] == source_commit
actual = {p.relative_to(folder).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
          for p in folder.rglob('*') if p.is_file() and p.name != 'BUILD.json'}
assert actual == build['files'], 'Extracted archive contents do not match the manifest'
renderer = 'System OpenGL in Linux Xvfb VM'
if sys.platform == 'win32':
    renderer = 'Bundled Mesa 26.3.1 llvmpipe in Windows VM'
suffix = '.exe' if sys.platform == 'win32' else ''
cli, gui = (folder / (name + suffix) for name in ('soundgeo-cli', 'soundgeo'))
if sys.platform == 'win32':
    cli = gui = archive
if sys.platform != 'win32':
    cli.chmod(0o755)
    gui.chmod(0o755)


def run(name, args, expected=0, timeout=240):
    result = subprocess.run([str(cli), *map(str, args)], cwd=destination, capture_output=True, text=True, timeout=timeout)
    (destination / (name + '.log')).write_text(result.stdout + result.stderr, encoding='utf-8')
    assert result.returncode == expected, (name, result.returncode, result.stderr)
    checks.append(name)
    return result.stdout


assert build['version'] in run('version', ['--version'])
if sys.platform == 'win32':
    cache_root = isolated / 'SoundGeo/runtime'
    cached_folders = list(cache_root.iterdir())
    assert len(cached_folders) == 1
    cached_app, = cached_folders[0].glob('*/soundgeo-cli.exe')
    original_mtime = cached_app.stat().st_mtime_ns
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

if sys.platform == 'win32':
    # Exercise FFprobe, frame extraction, Tesseract and its English model through
    # the public app command. The generated image and gazetteer are fixtures.
    # System.Drawing is only a test fixture generator in the Windows VM.
    image = destination / 'visible-text.png'
    script = destination / 'draw-text.ps1'
    script.write_text('''Add-Type -AssemblyName System.Drawing
$bitmap = New-Object Drawing.Bitmap 1280,720
$graphics = [Drawing.Graphics]::FromImage($bitmap)
$graphics.Clear([Drawing.Color]::White)
$font = New-Object Drawing.Font 'Arial',72
$graphics.DrawString('NEW YORK', $font, [Drawing.Brushes]::Black, 150, 250)
$bitmap.Save($args[0], [Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
''', encoding='utf-8')
    powershell = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
    subprocess.run([str(powershell), '-NoProfile', '-File', str(script), str(image)], check=True, timeout=30)
    video = destination / 'visible-text.mkv'
    subprocess.run([str(folder / 'ffmpeg.exe'), '-v', 'error', '-loop', '1', '-i', str(image),
                    '-t', '2', '-r', '2', '-c:v', 'ffv1', str(video)], check=True, timeout=30)
    gazetteer = destination / 'places.txt'
    # Place matching deliberately requires at least two words.
    gazetteer.write_text('5128581\tNew York\tNew York\t\t40.71427\t-74.00597\tP\tPPL\tUS\t\tNY\t\t\t\t8175133\t\t10\tAmerica/New_York\t2026-01-01\n', encoding='utf-8')
    ocr_report = destination / 'ocr.json'
    run('bundled-video-ocr', ['video-clues', '--input', video, '--gazetteer', gazetteer,
                            '--frames', '1', '--output', ocr_report, '--require-match'])
    ocr = json.loads(ocr_report.read_text(encoding='utf-8'))
    assert ocr['status'] == 'place_clues_found' and any('NEW YORK' in frame['text'].upper() for frame in ocr['frames']), ocr
    # Start two instances with a fresh profile to exercise atomic first extraction.
    original_profile = os.environ['LOCALAPPDATA']
    os.environ['LOCALAPPDATA'] = str(destination / 'Concurrent fresh profile')
    concurrent = [subprocess.Popen([str(archive), '--version'], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                  for _ in range(2)]
    for child in concurrent:
        stdout, stderr = child.communicate(timeout=120)
        assert child.returncode == 0 and build['version'].encode() in stdout, stderr
    fresh_root = Path(os.environ['LOCALAPPDATA']) / 'SoundGeo/runtime'
    assert len(list(fresh_root.iterdir())) == 1, 'Concurrent launches left incomplete runtime folders'
    os.environ['LOCALAPPDATA'] = original_profile
    checks.append('simultaneous-portable-launches')


def visible_windows(pid):
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
        return found
    result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(pid)], capture_output=True)
    return result.returncode == 0 and bool(result.stdout.strip())


def child_pids(parent):
    # Toolhelp reads process ancestry without PowerShell or third-party modules.
    from ctypes import wintypes
    class ProcessEntry(ctypes.Structure):
        _fields_ = [('size', wintypes.DWORD), ('usage', wintypes.DWORD), ('pid', wintypes.DWORD),
                    ('heap', ctypes.c_size_t), ('module', wintypes.DWORD), ('threads', wintypes.DWORD),
                    ('parent', wintypes.DWORD), ('priority', wintypes.LONG), ('flags', wintypes.DWORD),
                    ('exe', wintypes.WCHAR * 260)]
    kernel = ctypes.windll.kernel32
    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
    entry = ProcessEntry()
    entry.size = ctypes.sizeof(entry)
    found = []
    try:
        available = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
        while available:
            if entry.parent == parent:
                found.append(entry.pid)
            available = kernel.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    return found


with (destination / 'desktop.log').open('wb') as log:
    process = subprocess.Popen([str(gui)], cwd=destination, stdout=log, stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 120
        windows = []
        while not windows:
            pids = child_pids(process.pid) if sys.platform == 'win32' else [process.pid]
            windows = [window for pid in pids for window in (visible_windows(pid) if sys.platform == 'win32' else [visible_windows(pid)]) if window]
            assert process.poll() is None, 'Desktop application exited before creating a visible window'
            if time.monotonic() > deadline:
                raise RuntimeError('Desktop window did not appear')
            time.sleep(0.25)
        time.sleep(2)
        assert process.poll() is None, 'Desktop application exited after opening'
        checks.append('desktop-visible-window')
    finally:
        if process.poll() is None:
            if sys.platform == 'win32' and windows:
                ctypes.windll.user32.PostMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t]
                for window in windows:
                    ctypes.windll.user32.PostMessageW(window, 0x0010, 0, 0)
            else:
                process.terminate()
        process.wait(timeout=30)
        if sys.platform == 'win32':
            assert process.returncode == 0, 'Portable app did not exit cleanly'
        print((destination / 'desktop.log').read_text(encoding='utf-8', errors='replace'))
platform = 'windows-x64' if sys.platform == 'win32' else 'linux-x64'
extra = []
if sys.platform == 'win32':
    assert list(cache_root.iterdir()) == cached_folders and cached_app.stat().st_mtime_ns == original_mtime
    checks.append('stable-runtime-path-for-resumable-searches')
    cached_probe = cached_app.with_name('ffprobe.exe')
    original = cached_probe.read_bytes()
    cached_probe.write_bytes(b'corrupted cached tool')
    run('modified-runtime-rejected', ['--version'], 1)
    cached_probe.write_bytes(original)
    extra_dll = cached_app.with_name('unlisted.dll')
    extra_dll.write_bytes(b'unexpected library')
    run('unlisted-runtime-file-rejected', ['--version'], 1)
    extra_dll.unlink()
    assert build['version'] in run('verified-cache-reused', ['--version'])
    sources, = (root / 'dist').glob('*-windows-runtime-sources.zip')
    extra.append({'name': sources.name, 'sha256': hashlib.sha256(sources.read_bytes()).hexdigest()})
receipt = {'version': build['version'], 'source_commit': build['source_commit'],
           'source_tree': build['source_tree'], 'target': build['target'],
           'archive': archive.name, 'archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
           'file_hashes_verified': True, 'checks': checks,
           'desktop_renderer': renderer,
           'additional_assets': extra,
           'scope': 'Package smoke checks using synthetic/silent inputs; no new real-world accuracy measurement',
           'passed': True}
(root / 'dist' / f'validation-{platform}.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
print(json.dumps(receipt, indent=2))
