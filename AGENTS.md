# Working on SoundGeo

Use one Cargo target directory per checkout. Do not share `CARGO_TARGET_DIR`
between worktrees. A shared directory previously reused test artifacts from
an experimental branch during a main-branch check. That run was rejected.
Use each checkout's default `target/` directory. If a directory was shared,
stop concurrent builds and run `cargo clean --release --package soundgeo`
in the affected checkout before qualification. Preserve dependency caches.
Check that the test names and count agree with the source being qualified.

Run the required checks after code changes:

```text
cargo fmt --all -- --check
cargo clippy --release --all-targets --locked -- -D warnings
cargo test --release --locked --all-targets
cargo test --release --locked --doc
cargo build --release --locked --bins
```

Freeze each validation executable under a new name. Record its source commit
and SHA-256 hash. Do not replace an executable used by a running or planned
validation job. Keep failed attempts and their original inputs.

The current real-video target is a location within a 1 km radius.
Preserve the original 250 m benchmark and fixed plans. Record the 1 km
evaluation separately. A close coordinate alone does not remove competing
locations or establish a calibrated accuracy radius.
Use free reference sources. Tests without recording times must not receive
the camera position or its recording clock. A possible date and the original
broad region are allowed by the current benchmark. Record those inputs.

Keep computed-proposal and known-position diagnostics separate from broad-area
acceptance tests. Software tests, a successful download, and a coordinate hit
do not establish reliable geolocation. Describe remaining ambiguity and errors
in the README and validation record.

Write documentation in simple English. Keep the source YouTube reference.
Keep documentation and validation records focused on the project. Do not include
personal details, conversation history, or local account paths.
