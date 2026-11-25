# Windows runtime build

Run `python scripts/build_windows_runtime.py` in a disposable Windows x64 VM
with Git, Python 3.12, Visual Studio C++ build tools, and 7-Zip available. CI sets
`GITHUB_ACTIONS=true` and `RUNNER_TEMP` to the VM's temporary directory. The script
requires these variables to avoid accidental host builds. It limits vcpkg to two
build jobs and uses a release-only triplet with a static C runtime.

The script pins vcpkg, which pins each upstream source archive by SHA-512 and
applies the patches in its ports. It disables vcpkg binary caches so corresponding
source trees are present. FFmpeg's GPL and nonfree features are disabled. The
tools run as separate executables. The English OCR model and Mesa download are
pinned independently by revision and SHA-256. No downloads occur in the launcher.

The source archive contains `patched-sources` for the exact built dependency
sources, vcpkg's ports, patches, scripts and triplets, and this build recipe under
`soundgeo-build/scripts`. Run the recipe from `soundgeo-build` in a Windows VM to
reproduce the source selection and configuration. It downloads the original
pinned archives and applies the included port patches. Compiler versions can
affect binary hashes. The release's `RUNTIME.json` records runtime versions and
FFmpeg's complete configuration.

Mesa and the English model are redistributed unmodified with their upstream
notices. The Windows package chooses the OpenGL DLLs and their imported
dependencies, and audits every EXE/DLL against bundled files or Windows inbox
libraries. Source for Mesa and its build scripts is linked in the retained
upstream notices and at https://github.com/pal1000/mesa-dist-win/tree/26.3.1.
