"""Check the actual HTTP fixture on each VM before the full Rust compilation."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "src/network.rs").read_text(encoding="utf-8")
start = source.index("    fn server(\n")
end = source.index("\n    #[test]\n    fn download_verifies_digest", start)
fixture = source[start:end]
assert fixture.count("#[test]") == 1
assert "fn fixture_waits_for_complete_request_headers()" in fixture
logs = ROOT / "release-logs"
logs.mkdir(exist_ok=True)
output = Path(tempfile.mkdtemp(prefix="http-fixture-", dir=logs))
harness = output / "fixture.rs"
harness.write_text("use std::io::{Read, Write};\nuse std::time::Duration;\n" + fixture, encoding="utf-8")
executable = output / ("fixture.exe" if os.name == "nt" else "fixture")
with (output / "compile.log").open("w", encoding="utf-8") as log:
    subprocess.run(["rustc", "--edition=2024", "--test", str(harness), "-o", str(executable)],
                   cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
with (output / "test.log").open("w", encoding="utf-8") as log:
    result = subprocess.run([str(executable), "--nocapture"], cwd=ROOT, stdout=log,
                            stderr=subprocess.STDOUT, timeout=30)
with executable.open("rb") as file:
    executable_sha256 = hashlib.file_digest(file, "sha256").hexdigest()
proof = {"source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
         "network_source_sha256": hashlib.sha256((ROOT / "src/network.rs").read_bytes()).hexdigest(),
         "executable_sha256": executable_sha256, "fixture_extracted_verbatim": True,
         "passed": result.returncode == 0}
(output / "result.json").write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
print((output / "test.log").read_text(encoding="utf-8"))
print(json.dumps(proof, indent=2))
result.check_returncode()
