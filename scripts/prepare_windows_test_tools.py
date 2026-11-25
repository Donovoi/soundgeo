"""Use the bundled FFmpeg for unit tests; package tests later clear this PATH."""
import os
from pathlib import Path
import zipfile

root = Path(__file__).resolve().parents[1]
destination = Path(os.environ['RUNNER_TEMP']) / 'soundgeo-unit-test-tools'
destination.mkdir(exist_ok=False)
with zipfile.ZipFile(root / 'runtime-build/windows-runtime.zip') as stream:
    for name in ('ffmpeg.exe', 'ffprobe.exe'):
        (destination / name).write_bytes(stream.read(name))
with open(os.environ['GITHUB_PATH'], 'a', encoding='utf-8') as output:
    output.write(str(destination) + '\n')
