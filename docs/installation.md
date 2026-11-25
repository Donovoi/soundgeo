# Run SoundGeo

Download from [GitHub Releases](https://github.com/Donovoi/soundgeo/releases).
These experimental downloads are unsigned. Keep earlier versions and reports
when upgrading. SoundGeo has no automatic updater.

## Windows x64: one download

Download `soundgeo-VERSION-windows-x64.exe` for 64-bit Windows 10 or 11 and run it.
The EXE contains the desktop and command-line apps, FFmpeg, FFprobe, Tesseract,
the English OCR model, yt-dlp, gallery-dl, curl_cffi, Deno, aria2, software OpenGL,
and dependency notices. You do not need
to install those tools, a Visual C++ runtime, Python, Rust, or a graphics driver.
No administrator permission is needed.

Native speech runtimes, small language/speech detectors and fonts are included.
Recognition and translation weights download on demand after enabling speech
analysis. They are stored outside the runtime cache. See [speech analysis](speech.md)
for download sizes, memory needs and offline preparation.

Allow several seconds for first startup and leave at least 1 GB of free disk space.
The EXE unpacks its embedded files into a versioned cache under
`%LOCALAPPDATA%\SoundGeo\runtime`. Later launches verify every cached file and
reuse the same folder. This keeps executable paths stable for resumable searches.
Concurrent launches share a complete, verified cache. Close SoundGeo before
removing an old runtime cache or leftover `unpacking-*` folders from interrupted
extraction. A damaged cache is rejected with its location and recovery instructions.
Keep your work outside the runtime cache. Recordings, reports, and reference data stay in the locations you
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
sudo apt-get install ffmpeg aria2 libx11-6 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libegl1 libgl1 libudev1 libdbus-1-3 libxcb-shape0 libxcb-xfixes0
```

After extracting, open a terminal in the extracted folder:

```sh
chmod +x soundgeo soundgeo-cli
chmod +x speech-tools/*/*-cli speech-tools/llama/llama-completion
./soundgeo-cli --version
./soundgeo-cli check-audio --input /path/to/clip.mp4 --output audio-check.json
./soundgeo
```

The desktop app needs a graphical session and OpenGL support. The CLI can run
headlessly. Optional video text reading also needs `tesseract-ocr` and
`tesseract-ocr-eng`. In the download folder verify hashes with
`sha256sum --check --ignore-missing SHA256SUMS.txt`.

For URL intake on Linux, put the current [yt-dlp standalone Linux executable](https://github.com/yt-dlp/yt-dlp/releases),
[gallery-dl executable](https://codeberg.org/mikf/gallery-dl/releases), and
[Deno executable](https://github.com/denoland/deno/releases) on PATH or beside
SoundGeo as `yt-dlp`, `gallery-dl` and `deno`, with execute permission.
yt-dlp's standalone Linux build includes curl_cffi for browser impersonation.
See [URL intake](url-intake.md) for usage and the versions qualified with this release.

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

`THIRD_PARTY.txt` contains application dependency notices; the Windows package
also has `runtime-notices` for its bundled tools and graphics. `BUILD.json`
records source and bundled-file hashes. Windows runtime versions are recorded
in `RUNTIME.json` and `URL-TOOLS.json`. Media, downloader and graphics sources and build instructions are
provided in `soundgeo-VERSION-windows-runtime-sources.zip` on the same release.
`SPEECH-TOOLS.json` records native speech sources and bundled detector hashes;
`speech-notices` contains their license notices. Speech runtime sources are in
the Windows source archive and in `speech-tools-sources.zip` inside the Linux package.
SoundGeo includes FFmpeg under LGPL 2.1 or later as separate executables.
