"""Prepare downloader fixtures in CI VMs; Windows package checks later clear PATH."""
import os
from pathlib import Path
import sys
import zipfile
from build_url_tools import fetch

ROOT = Path(__file__).resolve().parents[1]
destination = Path(os.environ['RUNNER_TEMP']) / 'soundgeo-url-test-tools'
destination.mkdir(exist_ok=False)
if sys.platform == 'win32':
    with zipfile.ZipFile(ROOT / 'url-tools-build/url-tools.zip') as stream:
        stream.extractall(destination)
else:
    fetch('https://github.com/yt-dlp/yt-dlp/releases/download/2026.11.23/yt-dlp_linux', destination / 'yt-dlp',
          '249ddb058234276bc449b9cd9492aad9f133bda5923c61a9e39983ae114ef17d')
    fetch('https://codeberg.org/mikf/gallery-dl/releases/download/v1.32.21/gallery-dl.bin', destination / 'gallery-dl',
          'a0adde5c659a621218ab3d5e41e7b8c5d1578827cc2765436aa2767488a8b180')
    for path in destination.iterdir(): path.chmod(0o755)
with open(os.environ['GITHUB_PATH'], 'a', encoding='utf-8') as out: out.write(str(destination) + '\n')
