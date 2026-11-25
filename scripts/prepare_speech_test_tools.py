"""Install the exact packaged speech runtime for VM tests and font qualification."""
import os
from pathlib import Path
import zipfile

root = Path(__file__).resolve().parents[1]
target = root / 'target/release'
target.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(root / 'speech-tools-build/speech-tools.zip') as archive:
    archive.extractall(target)
    # Rust unit-test executables live in deps; use the same bytes there.
    archive.extractall(target / 'deps')
if os.name != 'nt':
    for directory in [target, target / 'deps']:
        for helper in (directory / 'speech-tools').glob('*/*'):
            if helper.name in ('whisper-cli', 'transcribe-cli', 'llama-completion'):
                helper.chmod(0o755)
fonts = str(target / 'assets/speech-fonts')
if 'GITHUB_ENV' in os.environ:
    with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as stream:
        stream.write('SOUNDGEO_TEST_SPEECH_FONTS=' + fonts + '\n')
else:
    print('Set SOUNDGEO_TEST_SPEECH_FONTS=' + fonts)
