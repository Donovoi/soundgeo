# Run SoundGeo

Download from [GitHub Releases](https://github.com/Donovoi/soundgeo/releases).
These experimental downloads are unsigned. Keep earlier versions and reports
when upgrading. SoundGeo has no automatic updater.

## Windows x64: one download

Download `soundgeo-VERSION-windows-x64.exe` for 64-bit Windows 10 or 11 and run it.
The EXE contains the desktop and command-line apps, FFmpeg, FFprobe, Tesseract,
the English OCR model, software OpenGL, and dependency notices. You do not need
to install those tools, a Visual C++ runtime, Python, Rust, or a graphics driver.
No administrator permission is needed.

Allow several seconds for startup. Each run verifies and unpacks its embedded
files into a new folder under `%LOCALAPPDATA%\SoundGeo\runtime`, and removes that
folder when SoundGeo closes normally. Leave at least 1 GB of free disk space.
Separate launches use separate folders. Interrupted launches can leave temporary
folders there; close SoundGeo before removing leftover `run-*` folders.
Recordings, reports, and downloaded reference data stay in the locations you
choose. The app does not upload recordings.

Compare the EXE's SHA-256 hash with `SHA256SUMS.txt` from the same release:

```powershell
Get-FileHash .\soundgeo-VERSION-windows-x64.exe -Algorithm SHA256
```

The EXE also accepts the app's command-line arguments. For scripts that need a
console executable, extract the bundle once into a new folder:

```powershell
Start-Process -Wait -FilePath .\soundgeo-VERSION-windows-x64.exe -ArgumentList '--portable-extract', 'SoundGeo-tools'
```

Open the versioned folder inside `SoundGeo-tools`, then use:

```powershell
.\soundgeo-cli.exe --version
.\soundgeo-cli.exe check-audio --input C:\recordings\clip.mp4 --output audio-check.json
```

Keep the whole extracted folder together. Explicit FFmpeg, Tesseract and language
folder overrides remain available for custom installations.

## Linux x64

Use `soundgeo-VERSION-linux-x64.zip` on Ubuntu 24.04 or a compatible distribution
with glibc 2.39 or newer. ARM and macOS binaries are not provided.
Extract the whole archive. On Ubuntu 24.04 install the runtime dependencies:

```sh
sudo apt-get update
sudo apt-get install ffmpeg libx11-6 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libegl1 libgl1 libudev1 libdbus-1-3 libxcb-shape0 libxcb-xfixes0
```

After extracting, open a terminal in the extracted folder:

```sh
chmod +x soundgeo soundgeo-cli
./soundgeo-cli --version
./soundgeo-cli check-audio --input /path/to/clip.mp4 --output audio-check.json
./soundgeo
```

The desktop app needs a graphical session and OpenGL support. The CLI can run
headlessly. Optional video text reading also needs `tesseract-ocr` and
`tesseract-ocr-eng`. In the download folder verify hashes with
`sha256sum --check --ignore-missing SHA256SUMS.txt`.

## First use

Try **Try a synthetic demo** in the desktop app, or run `soundgeo-cli demo
--output demo`, followed by `soundgeo-cli --offline analyze --config demo/job.json
--output demo/report.json`. The demo needs no flight download. It is a software
check using synthetic audio.

For your own recording, start with `check-audio`. Flight references, terrain and
GeoNames place data are downloaded through the app when needed; they vary by
date and search area and can require substantial disk space. An offline search
needs those references prepared in advance. See the [README](../README.md) and
[limitations](limitations.md) for supported evidence and accuracy results.

`THIRD_PARTY.txt` and `runtime-notices` contain dependency notices. `BUILD.json`
records source and bundled-file hashes. Windows runtime versions are recorded
in `RUNTIME.json`. Corresponding media-tool sources and build instructions are
provided in `soundgeo-VERSION-windows-runtime-sources.zip` on the same release.
SoundGeo includes FFmpeg under LGPL 2.1 or later as separate executables.
