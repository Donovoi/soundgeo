"""Recreate the synthetic noise regression. Requires NumPy, SciPy and FFmpeg."""
import argparse
import hashlib
import subprocess
import tempfile
from pathlib import Path

import numpy as np
from scipy import signal
from scipy.io import wavfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--ffmpeg', default='ffmpeg')
    args = parser.parse_args()
    rate = 8000
    time = np.arange(600 * rate) / rate
    noise = np.random.default_rng(1985).normal(0, 1, len(time))
    wind = signal.sosfilt(signal.butter(3, 250, fs=rate, output='sos'), noise)
    audio = (.12 * noise + .6 * wind * (1 + .7 * np.sin(time * .11))
             + .15 * np.sin(2 * np.pi * 65 * time))
    audio /= max(1, np.abs(audio).max())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as scratch:
        source = Path(scratch) / 'noise.wav'
        wavfile.write(source, rate, audio.astype(np.float32))
        subprocess.run([
            args.ffmpeg, '-nostdin', '-hide_banner', '-loglevel', 'error', '-y',
            '-ss', '200', '-i', str(source), '-t', '30', '-map_metadata', '-1',
            '-sample_fmt', 's16', '-c:a', 'flac', str(args.output),
        ], check=True)
    with args.output.open('rb') as source:
        print(hashlib.file_digest(source, 'sha256').hexdigest())


if __name__ == '__main__':
    main()
