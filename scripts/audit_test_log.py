"""Reject stale/missing test binaries by comparing test names with this checkout."""
from collections import Counter
from pathlib import Path
import re
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
source = []
for name in subprocess.check_output(['git', 'ls-files'], cwd=root, text=True).splitlines():
    if name.endswith('.rs'):
        tests = re.findall(r'#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn\s+(\w+)', (root / name).read_text(encoding='utf-8'))
        # Both binary entry points include these same modules. Their tests must
        # pass in both binaries; retain that multiplicity instead of deduplicating.
        copies = 2 if name in {'src/cli.rs', 'src/ui.rs', 'src/speech_ui.rs'} else 1
        source.extend(tests * copies)
log = Path(sys.argv[1]).read_text(encoding='utf-8')
passed = re.findall(r'^test (\S+) \.\.\. ok$', log, re.M)
if Counter(source) != Counter(name.rsplit('::', 1)[-1] for name in passed):
    raise SystemExit('Test names/count do not match source')
rows = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out', log)
if sum(int(row[0]) for row in rows) != len(source) or any(int(n) for row in rows for n in row[1:]):
    raise SystemExit('Test summaries are incomplete or contain excluded tests')
print(f'All {len(passed)} Rust test names match the source; no tests failed or were excluded.')
