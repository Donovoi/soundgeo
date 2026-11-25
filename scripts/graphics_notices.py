"""Retain notices from the exact graphics sources omitted by the binary pack."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import tarfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = Path(__file__).with_name('graphics-sources.json')


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def notice(name, data):
    if b'\0' in data:
        return ''
    text = data.decode('utf-8', errors='replace')
    basename = name.rsplit('/', 1)[-1]
    if (re.search(r'license|licence|copying|copyright|notice|^authors$', basename, re.I)
            or '/licenses/' in name.lower() or '/copying.' in name.lower()):
        return text
    blocks = re.findall(r'/\*.*?\*/', text, re.S)
    # Preserve complete comment groups, without copying executable source text.
    blocks.extend(re.findall(r'(?m)(?:^[ \t]*(?://|\#|;|@?REM\b)[^\n]*(?:\n|$))+', text[:32768]))
    selected = [block for block in blocks if re.search(r'copyright|permission is hereby|redistribution and use', block, re.I)]
    return '\n\n'.join(selected)


def collect(archive, destination):
    count = 0
    collected = {}
    def record(name, data):
        nonlocal count
        value = notice(name, data)
        if value:
            collected.setdefault(value, []).append(name)
            count += 1
    if archive.suffix == '.zip':
        with zipfile.ZipFile(archive) as stream:
            for entry in stream.infolist():
                if not entry.is_dir():
                    with stream.open(entry) as source:
                        record(entry.filename, source.read(2 * 1024 * 1024))
    else:
        with tarfile.open(archive, 'r|*') as stream:
            for entry in stream:
                if entry.isfile():
                    with stream.extractfile(entry) as source:
                        record(entry.name, source.read(2 * 1024 * 1024))
    with destination.open('w', encoding='utf-8', newline='\n') as output:
        output.write('Graphics dependency notices, including build and test source notices.\n'
                     'Identical notices are retained once with their source paths.\n'
                     'Source: ' + archive.name + '\nSHA-256: ' + sha(archive) + '\n')
        for value, paths in collected.items():
            output.write('\n' + '=' * 72 + '\n' + '\n'.join(paths) + '\n\n' + value + '\n')
    assert count > 0, f'No notices collected from {archive.name}'
    return count


def prepare(downloads, destination, sources_zip):
    downloads.mkdir(parents=True, exist_ok=True)
    destination.mkdir(parents=True, exist_ok=False)
    entries = json.loads(MANIFEST.read_text(encoding='utf-8'))
    counts = {}
    with zipfile.ZipFile(sources_zip, 'a', zipfile.ZIP_STORED) as sources:
        for entry in entries:
            path = downloads / entry['name']
            if not path.exists():
                with urllib.request.urlopen(entry['url'], timeout=90) as response, path.open('xb') as output:
                    shutil.copyfileobj(response, output)
            assert path.stat().st_size == entry['size'] and sha(path) == entry['sha256'], f'Graphics source checksum mismatch: {path.name}'
            if path.suffix == '.txt':
                shutil.copy2(path, destination / path.name)
            else:
                counts[path.name] = collect(path, destination / (path.name + '-NOTICES.txt'))
                # Original source archives are already compressed. Preserve them
                # byte for byte with the distribution's corresponding sources.
                sources.write(path, 'graphics-sources/' + path.name)
        for path in (MANIFEST, Path(__file__)):
            sources.write(path, 'soundgeo-build/scripts/' + path.name)
    shutil.copy2(MANIFEST, destination / MANIFEST.name)
    print(json.dumps({'graphics_notice_sections': counts,
                      'notice_bytes': sum(p.stat().st_size for p in destination.iterdir())}), flush=True)
