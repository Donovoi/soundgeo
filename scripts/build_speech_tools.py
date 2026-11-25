"""Build pinned CPU helpers and bundle small detectors/fonts, never ASR/MT weights.

Run only in a build VM. Each helper lives with its own ggml libraries. Source
archives, modifications, notices and a file manifest accompany the binaries.
"""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import wave
import zipfile

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / 'speech-tools-build'
WINDOWS = sys.platform == 'win32'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def fetch(entry):
    cache = WORK / 'downloads'
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / entry['filename']
    if path.is_file() and path.stat().st_size == entry['size'] and sha(path) == entry['sha256']:
        return path
    temporary = path.with_name(path.name + '.part')
    aria = shutil.which('aria2c')
    if aria and entry['size'] >= 8 * 1024 * 1024:
        subprocess.run([aria, '--no-conf', '--continue=true', '--allow-overwrite=true',
                        '--auto-file-renaming=false', '--file-allocation=none', '--split=8',
                        '--max-connection-per-server=8', '--min-split-size=8M',
                        '--max-tries=3', '--retry-wait=3', '--console-log-level=warn',
                        '--summary-interval=0', '--check-certificate=true',
                        '--checksum=sha-256=' + entry['sha256'],
                        '--dir=' + str(cache), '--out=' + temporary.name, entry['url']],
                       check=True, timeout=1800)
    else:
        with urllib.request.urlopen(entry['url'], timeout=90) as response, temporary.open('wb') as out:
            shutil.copyfileobj(response, out, 1024 * 1024)
    if temporary.stat().st_size != entry['size'] or sha(temporary) != entry['sha256']:
        raise RuntimeError('Pinned speech download failed verification: ' + entry['filename'])
    temporary.replace(path)
    return path


def execute(command, log, timeout=3600, env=None, cwd=ROOT):
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open('w', encoding='utf-8') as stream:
        result = subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT,
                                env=env, timeout=timeout, stdin=subprocess.DEVNULL)
    if result.returncode:
        print(log.read_text(encoding='utf-8', errors='replace')[-12000:], flush=True)
        raise RuntimeError(f'Speech helper command failed; see {log}')


def licenses(source, destination):
    destination.mkdir(parents=True)
    notices = []
    for path in sorted(source.rglob('*')):
        if not path.is_file() or path.stat().st_size > 2_000_000:
            continue
        named = re.search(r'license|licence|copying|copyright|notice', path.name, re.I)
        if named:
            target = destination / path.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)
        elif path.suffix in ('.h', '.hpp', '.c', '.cpp'):
            text = path.read_text(encoding='utf-8', errors='replace')
            # Single-header dependencies commonly keep license/copyright text
            # inside the source instead of a separate LICENSE file.
            if re.search(r'copyright|permission is hereby granted|SPDX-License', text, re.I):
                notices.append(f'\n--- {path.relative_to(source).as_posix()} ---\n{text}')
    if not any(destination.rglob('*')):
        raise RuntimeError('Missing native source license notices')
    (destination / 'SOURCE-NOTICES.txt').write_text('\n'.join(notices), encoding='utf-8')


def bundle_windows_runtime(build_dir, destination, notices):
    from pe_dependencies import imports, system
    cache = {}
    for line in (build_dir / 'CMakeCache.txt').read_text(encoding='utf-8').splitlines():
        if line and not line.startswith(('#', '//')) and '=' in line:
            key, value = line.split('=', 1)
            cache[key.split(':', 1)[0]] = value
    instance = Path(cache['CMAKE_GENERATOR_INSTANCE'].split(',', 1)[0])
    assert instance.is_dir(), 'CMake did not record its Visual Studio installation'
    vswhere = Path(os.environ['ProgramFiles(x86)']) / 'Microsoft Visual Studio/Installer/vswhere.exe'
    installations = json.loads(subprocess.check_output(
        [str(vswhere), '-all', '-products', '*', '-format', 'json', '-utf8'], encoding='utf-8'))
    selected, = [item for item in installations if Path(item['installationPath']).resolve() == instance.resolve()]
    assert not selected.get('isPrerelease', False), 'Preview Visual Studio runtimes cannot be redistributed'
    candidates = {p.resolve() for p in instance.glob('VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT')}
    assert candidates, 'The selected Visual Studio installation has no x64 redistributable runtime'
    redist = max(candidates, key=lambda p: tuple(map(int, re.findall(r'\d+', p.parent.parent.name))))
    available = {p.name.lower(): p for p in redist.glob('*.dll')}
    copied = {}
    while True:
        files = {p.name.lower(): p for p in destination.iterdir() if p.suffix.lower() in ('.exe', '.dll')}
        missing = {name for path in files.values() for name in imports(path)
                   if name not in files and not system(name)}
        if not missing:
            break
        for name in sorted(missing):
            assert name in available, f'Native helper needs a library outside the selected CRT redist: {name}'
            shutil.copy2(available[name], destination / available[name].name)
            copied[available[name].name] = sha(available[name])
    assert copied, 'Shared native helpers unexpectedly have no redistributable C++ runtime dependencies'
    major = int(selected['installationVersion'].split('.')[0])
    assert major in (17, 18), 'Review the redistributable terms for this Visual Studio version'
    year = 2022 if major == 17 else 2026
    record = {'visual_studio_version': selected['installationVersion'],
        'redist_version': redist.parent.parent.name, 'architecture': 'x64',
        'source': redist.relative_to(instance).as_posix(),
        'redistribution_terms': f'https://learn.microsoft.com/en-us/visualstudio/releases/{year}/redistribution',
        'files': copied}
    notices.mkdir(parents=True)
    (notices / 'MSVC-RUNTIME.json').write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
    (notices / 'NOTICE.txt').write_text(
        'Microsoft Visual C++ Runtime\nCopyright Microsoft Corporation. All rights reserved.\n\n'
        'These unmodified x64 runtime DLLs come from the REDIST directory of the Visual Studio\n'
        'installation used to compile this native helper. They are deployed beside the helper\n'
        'so the application does not require a separate runtime installation.\n'
        'Redistribution is governed by the Microsoft Visual Studio license terms and REDIST list:\n'
        + record['redistribution_terms'] + '\n'
        'https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files\n', encoding='utf-8')
    return record


def probe_translator(destination):
    model, = [entry for entry in json.loads((ROOT / 'src/speech/models.json').read_text())
              if entry['id'] == 'translation-compact']
    path = fetch(model)
    executable = destination / ('llama-completion.exe' if WINDOWS else 'llama-completion')
    # Model loading crosses native DLL boundaries and is not covered by --help.
    # CI enables this probe; downloaded weights never enter the runtime bundle.
    command = [str(executable), '-m', str(path), '-p',
        'Translate the following text into English. Output only the translation: Le ciel est bleu.',
        '--jinja', '-cnv', '-st', '-ngl', '0', '-n', '128', '-c', '2048', '-t', '2',
        '--temp', '0', '--seed', '42', '--no-display-prompt', '--simple-io', '--no-warmup']
    environment = dict(os.environ, LD_LIBRARY_PATH=str(destination))
    result = subprocess.run(command, capture_output=True, timeout=600, env=environment, stdin=subprocess.DEVNULL)
    (WORK / 'logs/translator-probe.stdout').write_bytes(result.stdout)
    (WORK / 'logs/translator-probe.stderr').write_bytes(result.stderr)
    assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')[-5000:]
    text = result.stdout.decode('utf-8')
    assert text.strip().endswith('[end of text]') and all(word in text.casefold() for word in ('sky', 'blue')), text
    print('Real translator model load and completion passed', flush=True)


def build(entry, bundle, sources):
    archive = fetch(entry)
    shutil.copy2(archive, sources / archive.name)
    source_parent = WORK / ('source-' + entry['name'])
    source_parent.mkdir(exist_ok=False)
    with tarfile.open(archive) as stream:
        stream.extractall(source_parent, filter='data')
    source, = source_parent.iterdir()
    if entry['name'] == 'transcribe':
        patch = ROOT / 'scripts/speech-patches/transcribe-cli-init.patch'
        execute(['git', 'apply', str(patch)], WORK / 'logs/transcribe-patch.log', 60, cwd=source)
    build_dir = WORK / ('build-' + entry['name'])
    arguments = ['cmake', '-S', str(source), '-B', str(build_dir), '-DCMAKE_BUILD_TYPE=Release',
                 '-DGGML_NATIVE=OFF', '-DGGML_CPU_ALL_VARIANTS=ON', '-DGGML_BACKEND_DL=ON',
                 '-DBUILD_SHARED_LIBS=ON', '-DGGML_CUDA=OFF', '-DGGML_VULKAN=OFF',
                 '-DGGML_METAL=OFF', '-DGGML_OPENMP=OFF', '-DGGML_BLAS=OFF',
                 '-DCMAKE_BUILD_RPATH=$ORIGIN', '-DCMAKE_INSTALL_RPATH=$ORIGIN',
                 '-DLLAMA_OPENSSL=OFF', '-DLLAMA_BUILD_BORINGSSL=OFF', '-DLLAMA_BUILD_LIBRESSL=OFF',
                 '-DLLAMA_BUILD_TESTS=OFF', '-DLLAMA_BUILD_SERVER=OFF',
                 '-DWHISPER_BUILD_TESTS=OFF', '-DWHISPER_FFMPEG=OFF',
                 '-DTRANSCRIBE_BUILD_SHARED=ON', '-DTRANSCRIBE_GGML_BACKEND_DL=ON',
                 '-DTRANSCRIBE_X86_CONSERVATIVE=ON', '-DTRANSCRIBE_BUILD_TESTS=ON',
                 '-DTRANSCRIBE_USE_SYSTEM_BLAS=OFF', '-DTRANSCRIBE_USE_OPENMP=OFF']
    if WINDOWS:
        # Native DLLs exchange FILE pointers and C++ objects. They must share
        # one CRT; separate /MT copies can fail fast when loading a model.
        # CMP0091 must be NEW before whisper's first project() call.
        arguments += ['-A', 'x64', '-DCMAKE_POLICY_DEFAULT_CMP0091=NEW',
                      '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL']
        manifest = (ROOT / 'scripts/speech-utf8.manifest').as_posix()
        arguments += [f'-DCMAKE_EXE_LINKER_FLAGS=/MANIFEST:EMBED /MANIFESTINPUT:"{manifest}"']
    execute(arguments, WORK / 'logs' / (entry['name'] + '-configure.log'))
    targets = [] if entry['name'] == 'transcribe' else ['--target', entry['executable']]
    execute(['cmake', '--build', str(build_dir), '--config', 'Release', '--parallel', '2', *targets],
            WORK / 'logs' / (entry['name'] + '-build.log'))
    if entry['name'] == 'transcribe':
        execute(['ctest', '--test-dir', str(build_dir), '-C', 'Release', '--output-on-failure'],
                WORK / 'logs/transcribe-tests.log', 600)
    destination = bundle / 'speech-tools' / entry['name']
    destination.mkdir(parents=True)
    executable = entry['executable'] + ('.exe' if WINDOWS else '')
    for path in sorted(build_dir.rglob('*')):
        if not path.is_file() or 'CMakeFiles' in path.parts:
            continue
        if path.name == executable or path.suffix.lower() == '.dll' or re.search(r'\.so(?:\.[0-9]+)*$', path.name):
            target = destination / path.name
            if target.exists() and sha(target) != sha(path):
                raise RuntimeError('Conflicting native library: ' + path.name)
            shutil.copy2(path, target)
    if not (destination / executable).is_file():
        raise RuntimeError('Native helper executable was not built')
    if WINDOWS:
        from pe_dependencies import audit
        bundle_windows_runtime(build_dir, destination, bundle / 'speech-notices/windows-runtime' / entry['name'])
        (destination / 'PE-IMPORTS.json').write_text(json.dumps(audit(destination), indent=2) + '\n')
    environment = dict(os.environ, LD_LIBRARY_PATH=str(destination))
    execute([str(destination / executable), '--help'], WORK / 'logs' / (entry['name'] + '-help.log'), 60, environment)
    if entry['name'] == 'llama' and os.environ.get('SOUNDGEO_SPEECH_BUILD_PROBE') == '1':
        probe_translator(destination)
    licenses(source, bundle / 'speech-notices' / entry['name'])
    print(entry['name'] + ' built and checked', flush=True)


def zip_tree(directory, path):
    with zipfile.ZipFile(path, 'x', zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for file in sorted(directory.rglob('*')):
            if file.is_file():
                archive.write(file, file.relative_to(directory).as_posix())


def main():
    WORK.mkdir(exist_ok=True)
    bundle, sources = WORK / 'bundle', WORK / 'sources'
    bundle.mkdir(exist_ok=False)
    sources.mkdir(exist_ok=False)
    entries = json.loads((ROOT / 'scripts/speech-tools-lock.json').read_text())
    fonts = json.loads((ROOT / 'scripts/speech-fonts-lock.json').read_text())['files']
    detectors = [entry for entry in json.loads((ROOT / 'src/speech/models.json').read_text())
                 if entry['id'] in ('silero-vad', 'ecapa-lid')]
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        downloaded = list(pool.map(fetch, fonts + detectors))
    for entry, path in zip(fonts + detectors, downloaded, strict=True):
        directory = bundle / ('assets/speech-fonts' if entry in fonts else 'speech-models')
        directory.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, directory / entry['filename'])
    for entry in entries:
        build(entry, bundle, sources)
    shutil.copytree(ROOT / 'docs/third-party/speech', bundle / 'speech-notices/models')
    shutil.copytree(ROOT / 'scripts/speech-patches', sources / 'soundgeo-patches')
    for name in ('build_speech_tools.py', 'pe_dependencies.py', 'speech-tools-lock.json', 'speech-fonts-lock.json', 'speech-utf8.manifest'):
        shutil.copy2(ROOT / 'scripts' / name, sources / name)
    shutil.copy2(ROOT / 'src/speech/models.json', sources / 'models.json')
    # Exercise actual model loading, not merely --help: this catches a shared
    # backend initialization failure that upstream API smoke tests missed.
    silence = WORK / 'silence with spaces.wav'
    with wave.open(str(silence), 'wb') as stream:
        stream.setnchannels(1); stream.setsampwidth(2); stream.setframerate(16000)
        stream.writeframes(b'\x00\x00' * 16000)
    helper_dir = bundle / 'speech-tools/transcribe'
    executable = helper_dir / ('transcribe-cli.exe' if WINDOWS else 'transcribe-cli')
    output = WORK / 'silence-vad.txt'
    execute([str(executable), '-m', str(bundle / 'speech-models/ggml-silero-v6.2.0.bin'),
             '--backend', 'cpu', '--threads', '2', '-q', '-o', str(output), str(silence)],
            WORK / 'logs/vad-model-load.log', 60, dict(os.environ, LD_LIBRARY_PATH=str(helper_dir)))
    if output.read_text().strip():
        raise RuntimeError('Silence unexpectedly produced speech intervals')
    manifest = {'sources': entries, 'detectors': detectors,
        'build_script_sha256': sha(Path(__file__)),
        'patches': {p.name: sha(p) for p in (ROOT / 'scripts/speech-patches').glob('*.patch')},
        'files': {
        file.relative_to(bundle).as_posix(): sha(file) for file in sorted(bundle.rglob('*')) if file.is_file()}}
    (bundle / 'SPEECH-TOOLS.json').write_text(json.dumps(manifest, indent=2) + '\n')
    zip_tree(bundle, WORK / 'speech-tools.zip')
    zip_tree(sources, WORK / 'speech-tools-sources.zip')


if __name__ == '__main__':
    main()
