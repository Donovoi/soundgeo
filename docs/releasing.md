# Release a version

Builds and tests run in VMs. The Windows and Ubuntu CI jobs qualify the exact
source tree, build the applications, package them and exercise the downloads.
Each checkout uses its own default Cargo target directory.

Windows builds use static C runtimes. `scripts/build_windows_runtime.py` builds
FFmpeg and Tesseract from a pinned vcpkg revision, downloads the pinned English
model and Mesa package with SHA-256 checks, and preserves patched dependency
sources and build scripts. CI caches these outputs by build-input hashes.
`scripts/package_release.py` audits DLL imports, creates the payload and appends
it with a length and SHA-256 trailer to `soundgeo-portable.exe`. It publishes the
resulting EXE and a corresponding runtime-source archive. The runtime ZIP is an
intermediate build artifact. The Windows package check clears external tool paths
and exercises the actual portable executable, including OCR and desktop startup.

1. Update the package version in `Cargo.toml` and `Cargo.lock`.
2. Add `docs/releases/vVERSION.md` with changes and current validation limits.
3. Set `.github/release.json` to that version and its `prerelease` flag.
4. Open a pull request. Review both platform checks and package validation artifacts.
5. Merge after the checks pass. The main-branch workflow repeats all checks and
   publishes the requested release only after both platform jobs succeed.

The publisher creates a draft, attaches the tested downloads, runtime sources, validation receipts
and checksums, then publishes it. An existing published release is left unchanged.
A failed upload leaves a draft that the same source revision can resume. A draft
from a different revision fails instead of replacing files. Tags and published
assets must never be moved or overwritten to repair a release; publish a new version.

Changing ordinary source files does not request another release. Publication is
requested by changing the version in `.github/release.json`. Leave `prerelease`
true while broader real-recording accuracy remains unmeasured.
