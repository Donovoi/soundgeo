"""Collect Deno's notices from its exact source release and Cargo.lock archives."""
import concurrent.futures
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import time
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
SOURCE = {'name': 'deno_src.tar.gz',
          'url': 'https://github.com/denoland/deno/releases/download/v2.9.8/deno_src.tar.gz',
          'sha256': 'b57d0281c1647effc735f242ba52bc409747fc6ed6ad4946e76f062c67e113eb'}


def fetch(url, path, digest):
    if path.exists():
        assert hashlib.sha256(path.read_bytes()).hexdigest() == digest, path
        return path
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=90) as response: data = response.read()
            assert hashlib.sha256(data).hexdigest() == digest, url
            path.write_bytes(data)
            return path
        except Exception:
            if attempt == 2: raise
            time.sleep(2 * (attempt + 1))


def license_files(stream):
    result = []
    for entry in stream.getmembers():
        name = PurePosixPath(entry.name).name.lower()
        if entry.isfile() and (re.match(r'^(license|licence|unlicense|copying|copyright|notice)([._-]|$)', name)
                              or any(p.lower() in ('licenses', 'licences') for p in PurePosixPath(entry.name).parts[:-1])):
            if entry.size > 16 * 1024 * 1024: continue
            data = stream.extractfile(entry).read()
            try: text = data.decode('utf-8')
            except UnicodeDecodeError: continue
            result.append((entry.name, text))
    return result


def prepare(cache, notices, sources):
    cache = Path(cache); cache.mkdir(parents=True, exist_ok=True)
    notices = Path(notices); notices.mkdir(parents=True, exist_ok=True)
    sources = Path(sources); sources.mkdir(parents=True, exist_ok=True)
    path = fetch(SOURCE['url'], sources / SOURCE['name'], SOURCE['sha256'])
    with tarfile.open(path) as stream:
        lock = tomllib.loads(stream.extractfile('deno/Cargo.lock').read().decode())
        own = license_files(stream)
        # TypeScript's copyright/Apache notice is embedded in the compiler source.
        for name in ('deno/cli/tsc/00_typescript.js',):
            try:
                header = stream.extractfile(name).read(6000).decode('utf-8')
                own.append((name + ' (copyright header)', header.split('*/', 1)[0] + '*/'))
            except KeyError: pass
    packages = [p for p in lock['package'] if p.get('source', '').startswith('registry+')]
    assert all('checksum' in p for p in packages)
    def collect(package):
        name, version = package['name'], package['version']
        assert re.fullmatch(r'[a-zA-Z0-9_-]+', name) and re.fullmatch(r'[a-zA-Z0-9.+-]+', version)
        url = f'https://static.crates.io/crates/{name}/{name}-{version}.crate'
        archive = fetch(url, cache / f'{name}-{version}.crate', package['checksum'])
        with tarfile.open(archive) as stream:
            metadata = tomllib.loads(stream.extractfile(f'{name}-{version}/Cargo.toml').read().decode())['package']
            texts = license_files(stream)
            # A few crates publish their notice only in source comments.
            if not texts:
                for entry in stream.getmembers():
                    if entry.isfile() and entry.name.endswith(('.rs', '.c', '.h')) and entry.size < 256000:
                        header = stream.extractfile(entry).read(8000).decode('utf-8', errors='replace')
                        if re.search(r'copyright', header, re.I) and re.search(r'license|permission', header, re.I):
                            texts.append((entry.name + ' (source notice)', header))
        return {'name': name, 'version': version, 'sha256': package['checksum'], 'url': url,
                'license': metadata.get('license'), 'repository': metadata.get('repository'),
                'authors': metadata.get('authors'), 'texts': texts}
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        records = list(pool.map(collect, packages))
    overrides = json.loads((ROOT / 'docs/third-party/deno-dependency-notices.json').read_text(encoding='utf-8'))
    overrides = {(p['name'], p['version']): p for p in overrides}
    v8 = json.loads((ROOT / 'docs/third-party/deno-v8-notices.json').read_text(encoding='utf-8'))
    # Apache-2.0 text is identical across packages; some crates omit its file.
    apache = next((text for item in records for name, text in item['texts']
                   if PurePosixPath(name).name.lower() == 'license-apache'), None)
    for item in records:
        extra = overrides.get((item['name'], item['version']))
        if extra:
            assert extra['crate_sha256'] == item['sha256']
            for entry in extra['notices']:
                assert hashlib.sha256(entry['text'].encode()).hexdigest() == entry['sha256']
                item['texts'].append((entry['url'] + '\n' + extra['note'], entry['text']))
        if item['name'] == 'v8':
            assert item['version'] == v8['rusty_v8']
            for entry in v8['notices']:
                assert hashlib.sha256(entry['text'].encode()).hexdigest() == entry['sha256']
                item['texts'].append((entry['path'] + '\n' + entry['url'], entry['text']))
        if not item['texts'] and 'Apache-2.0' in (item['license'] or '') and apache:
            item['texts'] = [('Apache-2.0 (standard text; selected license)', apache)]
        if not item['texts'] and item['license'] == 'CC0-1.0':
            item['texts'] = [('CC0-1.0 (standard text)', (ROOT / 'docs/third-party/CC0-1.0.txt').read_text(encoding='utf-8'))]
        if item['name'] in ('deno_core_icudata', 'deno_native_certs', 'fqdn'):
            assert item['license'] == 'MIT'
            # These exact source releases declare MIT but contain no copyright
            # or license file. Preserve their declared authors and standard terms
            # without inventing a copyright notice. ICU's data notices are in V8.
            mit = next(text for name, text in own if name == 'deno/LICENSE.md')
            terms = mit[mit.index('Permission is hereby granted'):]
            item['texts'].append(('Declared MIT license (standard terms); authors: ' + str(item['authors']), terms))
    remaining = [item['name'] + '-' + item['version'] for item in records if not item['texts']]
    assert not remaining, f'Missing Deno dependency notices: {remaining}'
    with (notices / 'DENO-THIRD-PARTY.txt').open('w', encoding='utf-8') as output:
        output.write('Deno 2.9.8 notices. Includes build/test/platform dependencies from the release lockfile.\n')
        for name, text in own: output.write(f'\n--- {name} ---\n{text}\n')
        for item in sorted(records, key=lambda p: (p['name'], p['version'])):
            output.write(f"\n{'=' * 72}\n{item['name']} {item['version']}\nLicense: {item['license']}\nSource: {item['url']}\n")
            for name, text in item['texts']: output.write(f'\n--- {name} ---\n{text}\n')
    (notices / 'DENO-DEPENDENCIES.json').write_text(json.dumps([{k:v for k,v in p.items() if k != 'texts'} for p in records], indent=2) + '\n', encoding='utf-8')
    print(f'Deno notices: {len(records)} locked crate archives verified', flush=True)
    return cache


if __name__ == '__main__':
    import sys
    prepare(*sys.argv[1:])
