"""Real downloader integration against controlled HTTP fixtures, entirely in a VM."""
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading
import time
import wave


def verify(cli, folder, workspace, run):
    suffix = '.exe' if sys.platform == 'win32' else ''
    tools = {name: str(folder / (name + suffix)) if (folder / (name + suffix)).is_file() else shutil.which(name)
             for name in ('ffmpeg', 'yt-dlp', 'gallery-dl', 'aria2c')}
    assert all(tools.values()), tools
    for name in ('yt-dlp', 'gallery-dl', 'aria2c'):
        result = subprocess.run([tools[name], '--version'], capture_output=True, text=True, timeout=30)
        (workspace / f'url-{name}-version.log').write_text(result.stdout + result.stderr, encoding='utf-8')
        assert result.returncode == 0
    targets = subprocess.check_output([tools['yt-dlp'], '--ignore-config', '--list-impersonate-targets'], text=True, timeout=30)
    assert 'Chrome' in targets and 'curl_cffi' in targets, targets
    (workspace / 'url-impersonation-targets.log').write_text(targets, encoding='utf-8')
    fixtures = workspace / 'url-fixtures'; fixtures.mkdir()
    image = fixtures / 'frame.ppm'
    image.write_bytes(b'P6\n160 90\n255\n' + b'\x40\x80\xc0' * 160 * 90)
    sound = fixtures / 'audio.wav'
    with wave.open(str(sound), 'wb') as output:
        output.setnchannels(1); output.setsampwidth(2); output.setframerate(8000)
        output.writeframes(b'\0' * 8000 * 2 * 2)
    video = fixtures / 'clip.mp4'
    subprocess.run([tools['ffmpeg'], '-v', 'error', '-loop', '1', '-i', str(image), '-i', str(sound),
                    '-t', '2', '-r', '2', '-c:v', 'mpeg4', '-c:a', 'aac', str(video)], check=True, timeout=30)
    large = fixtures / 'large.wav'
    with wave.open(str(large), 'wb') as output:
        output.setnchannels(1); output.setsampwidth(2); output.setframerate(8000)
        for _ in range(32): output.writeframesraw(b'\0' * 1024 * 1024)
    files = {'/clip.mp4': video, '/second.mp4': video, '/large.wav': large}
    requests = []; active = 0; peak = 0; mutex = threading.Lock()
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_HEAD(self): self.serve(False)
        def do_GET(self): self.serve(True)
        def serve(self, body):
            nonlocal active, peak
            with mutex:
                requests.append({'path': self.path, 'range': self.headers.get('Range'), 'user_agent': self.headers.get('User-Agent')})
            if self.path == '/multi':
                data = b'<html><head><title>Two fixture clips</title></head><body><video controls src="/clip.mp4"></video><video controls src="/second.mp4"></video></body></html>'
                self.send_response(200); self.send_header('Content-Type', 'text/html'); self.send_header('Content-Length', str(len(data))); self.end_headers()
                if body: self.wfile.write(data)
                return
            path = files.get(self.path)
            if path is None: self.send_error(404); return
            size = path.stat().st_size; start = 0; end = size - 1
            ranged = self.headers.get('Range')
            if ranged:
                match = re.fullmatch(r'bytes=(\d+)-(\d*)', ranged)
                if not match: self.send_error(416); return
                start = int(match[1]); end = min(int(match[2]) if match[2] else end, end)
                if start > end: self.send_error(416); return
            self.send_response(206 if ranged else 200)
            self.send_header('Content-Type', 'video/mp4' if path == video else 'audio/wav')
            self.send_header('Content-Length', str(end - start + 1)); self.send_header('Accept-Ranges', 'bytes')
            if ranged: self.send_header('Content-Range', f'bytes {start}-{end}/{size}')
            self.end_headers()
            if not body: return
            with mutex: active += 1; peak = max(peak, active)
            try:
                with path.open('rb') as source:
                    source.seek(start); remaining = end - start + 1
                    while remaining:
                        data = source.read(min(65536, remaining)); remaining -= len(data)
                        self.wfile.write(data)
                        if path == large: time.sleep(0.002)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError): pass
            finally:
                with mutex: active -= 1
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
    base = f'http://127.0.0.1:{server.server_port}'
    cache = workspace / 'Media cache with spaces'
    common = ['fetch-media', '--cache', cache, '--output', '-']
    try:
        listing = json.loads(run('url-list-multiple-clips', [*common, '--url', base + '/multi', '--list']))
        assert len(listing['candidates']) == 2, listing
        run('url-requires-clip-selection', [*common, '--url', base + '/multi'], expected=1)
        selected = json.loads(run('url-selected-video-and-audio', [*common, '--url', base + '/multi', '--item', '2']))
        assert selected['has_video'] and selected['has_audio'] and selected['selected_item'] == 2, selected
        assert selected['sha256'] == hashlib.sha256(Path(selected['path']).read_bytes()).hexdigest()
        gallery = json.loads(run('url-gallery-dl-intake', [*common, '--url', base + '/clip.mp4', '--engine', 'gallery-dl']))
        assert gallery['source']['engine'] == 'gallery-dl' and gallery['has_video'] and gallery['has_audio']
        spoofed = json.loads(run('url-browser-impersonation', [*common, '--url', base + '/clip.mp4', '--impersonate']))
        assert spoofed['source']['impersonate'] and spoofed['has_audio']
        downloaded = json.loads(run('url-parallel-ranged-transfer', [*common, '--url', base + '/large.wav']))
        assert downloaded['sha256'] == hashlib.sha256(large.read_bytes()).hexdigest(), downloaded
        ranges = [r for r in requests if r['path'] == '/large.wav' and r['range']]
        assert len(ranges) > 1 and peak > 1, (ranges, peak)
        count = len(requests)
        cached = json.loads(run('url-offline-cache-reuse', ['--offline', *common, '--url', base + '/large.wav']))
        assert cached['path'] == downloaded['path'] and len(requests) == count
        target = Path(cached['path'])
        with target.open('r+b') as output: output.seek(1000); output.write(b'corrupt')
        run('url-offline-corruption-rejected', ['--offline', *common, '--url', base + '/large.wav'], expected=1)
        assert len(requests) == count
        run('url-local-scheme-rejected', [*common, '--url', 'file:///private/local.mp4'], expected=1)
        run('url-missing-offline-cache', ['--offline', *common, '--url', base + '/uncached'], expected=1)
    finally:
        server.shutdown(); server.server_close(); thread.join(timeout=10)
        (workspace / 'url-http-evidence.json').write_text(json.dumps({'peak_transfers': peak, 'requests': requests}, indent=2), encoding='utf-8')


if __name__ == '__main__':
    cli = Path(sys.argv[1]).resolve(); workspace = Path(sys.argv[2]).resolve(); workspace.mkdir(exist_ok=False)
    def run(name, args, expected=0):
        result = subprocess.run([str(cli), *map(str, args)], capture_output=True, text=True, timeout=240, cwd=workspace)
        (workspace / (name + '.log')).write_text(result.stdout + result.stderr, encoding='utf-8')
        assert result.returncode == expected, (name, result.returncode, result.stderr)
        print(name, 'passed', flush=True)
        return result.stdout
    verify(cli, cli.parent, workspace, run)
