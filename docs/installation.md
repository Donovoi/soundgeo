# Install SoundGeo

Download the archive for your computer from
[GitHub Releases](https://github.com/Donovoi/soundgeo/releases). Extract the whole
archive into a new folder. Both the desktop app and command-line app are included.
The downloads are unsigned. These experimental releases have no installer or
automatic updater. Keep earlier versions and reports when upgrading.

## Windows x64

Use `soundgeo-VERSION-windows-x64.zip` on 64-bit Windows 10 or 11.
Install the [Microsoft Visual C++ x64 runtime](https://aka.ms/vs/17/release/vc_redist.x64.exe)
if Windows reports a missing runtime DLL.
The desktop app needs a graphics driver with OpenGL 2.0 or newer. Install the
current driver from your graphics-card manufacturer if the app reports an OpenGL
error. Basic display drivers in some VMs and remote sessions only provide OpenGL 1.1.

Install FFmpeg using a Windows build linked from the
[FFmpeg download page](https://ffmpeg.org/download.html). Put `ffmpeg.exe` and
`ffprobe.exe` beside `soundgeo.exe`, or add their folder to `PATH`.
Start `soundgeo.exe` to open the desktop app.

From PowerShell in that folder:

```powershell
.\soundgeo-cli.exe --version
.\soundgeo-cli.exe check-audio --input C:\recordings\clip.mp4 --output audio-check.json
Get-FileHash ..\soundgeo-VERSION-windows-x64.zip -Algorithm SHA256
```

Compare the archive hash with `SHA256SUMS.txt` from the same release.

## Linux x64

Use `soundgeo-VERSION-linux-x64.zip` on Ubuntu 24.04 or a compatible distribution
with glibc 2.39 or newer. ARM and macOS binaries are not provided.
On Ubuntu 24.04 install the runtime dependencies:

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
headlessly. In the download folder, verify the archives with
`sha256sum --check --ignore-missing SHA256SUMS.txt`.

## First use

Try **Try a synthetic demo** in the desktop app, or use the CLI:

```powershell
.\soundgeo-cli.exe demo --output demo
.\soundgeo-cli.exe --offline analyze --config demo/job.json --output demo/report.json
```

On Linux replace `.\soundgeo-cli.exe` with `./soundgeo-cli`.
The demo needs no flight download. It is a software check using synthetic audio.

For your own recording, start with `check-audio`. A location search also needs
flight references, and can take substantial time and disk space. See the
[README](../README.md) for online and offline workflows and
[limitations](limitations.md) for supported evidence and accuracy results.

FFmpeg, flight data, terrain, Tesseract and GeoNames are separate downloads.
Dependency notices are included in `THIRD_PARTY.txt`; `BUILD.json` records the
source revision, tools and file hashes for this archive.
