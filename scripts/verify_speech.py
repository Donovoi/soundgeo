"""Exercise the packaged speech feature in a VM, with real held-out recordings."""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import unicodedata
import wave


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def normalize(text):
    return ''.join(c for c in unicodedata.normalize('NFKC', text).casefold()
                   if unicodedata.category(c)[0] in 'LMN')


def distance(left, right):
    previous = list(range(len(right) + 1))
    for i, a in enumerate(left, 1):
        row = [i]
        for j, b in enumerate(right, 1):
            row.append(min(row[-1] + 1, previous[j] + 1, previous[j - 1] + (a != b)))
        previous = row
    return previous[-1]


def verify(cli, folder, destination, run):
    directory = destination / 'speech-check'
    directory.mkdir()
    cache = directory / 'Model cache with spaces'
    empty = directory / 'empty cache'
    manifest = json.loads((folder / 'SPEECH-TOOLS.json').read_text(encoding='utf-8'))
    for relative, digest in manifest['files'].items():
        assert sha(folder / relative) == digest, relative
    if sys.platform == 'win32':
        from pe_dependencies import audit
        for family in (folder / 'speech-tools').iterdir():
            audit(family)
    else:
        for helper in (folder / 'speech-tools').glob('*/*'):
            if helper.name in ('whisper-cli', 'transcribe-cli', 'llama-completion'):
                helper.chmod(0o755)
    # A clean offline cache must handle silence without any large model download.
    silence = directory / 'silence.wav'
    with wave.open(str(silence), 'wb') as stream:
        stream.setnchannels(1); stream.setsampwidth(2); stream.setframerate(16000)
        stream.writeframes(b'\0' * 16000 * 2 * 3)
    output = directory / 'silence.json'
    run('speech-silence-offline', ['--offline', 'transcribe', '--input', silence, '--cache', empty, '--output', output])
    report = json.loads(output.read_text(encoding='utf-8'))
    assert report['status'] == 'no_speech' and not report['segments']
    assert not (empty / 'models').exists()
    fixtures = folder / 'validation/speech/fixtures'
    inputs = json.loads((fixtures / 'manifest.json').read_text(encoding='utf-8'))
    assert inputs['license'] == 'CC-BY-4.0'
    records = []
    for sample in inputs['fixtures']:
        source = fixtures / sample['fixture']
        assert sha(source) == sample['fixture_sha256']
        # All helpers must accept Unicode recording paths, not only ASCII paths.
        media = directory / (sample['language'] + '-录音-أصلي.flac')
        shutil.copy2(source, media)
        language = sample['language']
        arguments = ['transcribe', '--input', media, '--cache', cache, '--threads', '2',
                     '--translation-quality', 'compact']
        if language in ('hi', 'ur', 'bn'):
            arguments += ['--language', language]
        if language == 'en':
            missing_arguments = [empty if value == cache else value for value in arguments]
            run('speech-missing-offline-model', ['--offline', *missing_arguments, '--output', directory / 'missing.json'], 1)
            assert 'offline' in (destination / 'speech-missing-offline-model.log').read_text(encoding='utf-8').lower()
            assert not (directory / 'missing.json').exists()
        preparation = directory / (language + '-preparation.json')
        run('speech-prepare-' + language, [*arguments, '--prepare-only', '--output', preparation], timeout=1800)
        prepared = json.loads(preparation.read_text(encoding='utf-8'))
        assert prepared['status'] == 'ready' and prepared['selected_language'] == language and not prepared['segments']
        output = directory / (language + '.json')
        run('speech-offline-' + language, ['--offline', *arguments, '--output', output], timeout=1800)
        report = json.loads(output.read_text(encoding='utf-8'))
        assert report['status'] == 'complete' and report['selected_language'] == language
        assert report['source']['sha256'] == sha(media)
        assert report['segments'] and all(s['original'] and s['english'] for s in report['segments'])
        assert all(0 <= s['start_s'] < s['end_s'] <= report['source']['duration_s'] + 0.05 for s in report['segments'])
        assert all(len(m['sha256']) == 64 and len(m['runtime_sha256']) == 64 for m in report['models'])
        original = ' '.join(s['original'] for s in report['segments'])
        reference = normalize(sample['normalized_text'])
        cer = distance(reference, normalize(original)) / max(1, len(reference))
        # Broad package regression bound, not a field accuracy claim. Independent
        # per-language quality measurements are kept in the validation record.
        assert cer < 0.40, (language, 'unusable packaged transcript', cer, original)
        if language == 'en':
            assert not any(m['stage'] == 'translation' for m in report['models'])
            assert all(s['original'] == s['english'] for s in report['segments'])
        records.append({'language': language, 'source_sha256': sample['audio_sha256'],
                        'fixture_sha256': sample['fixture_sha256'], 'cer': cer,
                        'segments': len(report['segments']), 'status': report['status']})
        preserved = output.read_bytes()
        run('speech-preserve-' + language, ['--offline', *arguments, '--output', output], 1)
        assert output.read_bytes() == preserved
    # Also exercise the default quality translator's real load and completion.
    french = directory / 'fr-录音-أصلي.flac'
    quality = ['transcribe', '--input', french, '--language', 'fr', '--cache', cache, '--translation-quality', 'quality']
    run('speech-prepare-quality', [*quality, '--prepare-only', '--output', directory / 'quality-preparation.json'], timeout=1800)
    run('speech-quality-offline', ['--offline', *quality, '--output', directory / 'quality.json'], timeout=1800)
    quality_report = json.loads((directory / 'quality.json').read_text(encoding='utf-8'))
    assert quality_report['status'] == 'complete' and all(s['english'] for s in quality_report['segments'])
    assert any(m['id'] == 'translation-quality' for m in quality_report['models'])
    result = {'scope': 'Packaged-runtime regression on four held-out read-speech recordings, Unicode paths, offline cache, silence, no-clobber output and both translator sizes',
              'cases': records, 'passed': True}
    (directory / 'qualification.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    return directory / 'ur.json'
