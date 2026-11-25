# Download recordings from links

Open **Or paste a video / page URL** in the desktop app. Paste an HTTP(S) URL,
select **Find media**, choose a clip, then **Download and use selected clip**.
The recording becomes the app's selected local file. Run an audio check or
choose flight data and analyze it as usual.

SoundGeo uses yt-dlp to inspect supported sites, embedded players and direct
media links. Automatic discovery retries with Chrome impersonation through
curl_cffi, then uses gallery-dl for additional sites. gallery-dl finds media
links and passes the selected link and its HTTP headers to yt-dlp for download.
The Windows bundle includes both tools, their Python runtime and dependencies,
Deno, FFmpeg, FFprobe and aria2. Nothing needs to be installed separately.

The app lists at most 20 candidates. Gallery inspection is limited to the first
100 files and 20 posts/child extractors. Use a specific post URL for a large
gallery. If a page has several clips, the command line requires an item number.
Streams are merged without recompressing the recording. Still images are not
accepted as playable recordings; a silent video can still supply OCR clues.

```powershell
soundgeo-cli.exe fetch-media --url "https://example.org/page" --list
soundgeo-cli.exe fetch-media --url "https://example.org/page" --item 1 --cache media-cache --output downloaded.json
soundgeo-cli.exe fetch-media --url "https://example.org/page" --engine gallery-dl
soundgeo-cli.exe fetch-media --url "https://example.org/page" --impersonate
soundgeo-cli.exe --offline fetch-media --url "https://example.org/page" --item 1 --cache media-cache
soundgeo-cli.exe analyze --url "https://example.org/page" --media-item 1 --media-cache media-cache --config job.json --output result.json
```

On Windows, the default media cache is in Local AppData under `SoundGeo/media`.
Choose another folder in the app or use `--cache` (`--media-cache` for analyze).
Each recording has its source listing, selected item, stream types, duration,
SHA-256 and download time. The cache also retains helper logs and interrupted
transfers. Retrying the same item can resume a partial download. Offline mode
uses saved listings and hash-verified recordings without starting downloaders.
If a completed recording fails verification, keep that cache for inspection
and choose a new cache folder to fetch a fresh copy.

The source page's title, dates and location metadata remain in the download
record. They are not used to set the recording clock, search bounds or accepted
location. A download is not evidence of a location match.

## Transfer behavior

Large unsigned archive files use aria2 with checksum verification and atomic
promotion into the cache. An interrupted transfer stays separate from completed
files. A queue adapts concurrency to observed throughput, up to 16 files and four
connections per file. Direct media can use up to 16 connections; native HLS/DASH
uses eight fragment requests. Small files and servers without range support can
use fewer connections. There is no default bandwidth cap.

Impersonated media stays with yt-dlp's curl_cffi transport, since aria2 cannot
reproduce its TLS fingerprint. Signed sample requests keep their original
authentication and validation, with eight requests in flight. Airport replay
uses eight workers; terrain uses four. Retries are bounded and provider errors
remain visible. The Cancel button stops helper processes and their descendants.

Site logins, paywalls, DRM, deleted files and some anti-bot checks can prevent
downloads. The app does not import browser cookies automatically. Try a direct
video/post URL or enable browser impersonation for a compatible site. See
[limitations](limitations.md) for geolocation accuracy and recording requirements.

## Bundled versions

Checked against upstream releases on 25 November 2026: yt-dlp 2026.11.23,
gallery-dl 1.32.21, aria2 1.37.0 and Deno 2.9.8. Both Python downloaders are built
with the same pinned dependencies, including curl_cffi 0.17.0 and current
certificate data. aria2 is built for HTTP(S) with Windows TLS and DNS. FFmpeg 9.1,
Tesseract 5.5.4, the current tessdata_fast English model and Mesa 26.3.1 are included.

`URL-TOOLS.json`, `RUNTIME.json` and `BUILD.json` record versions and hashes.
Runtime license notices are inside the bundle. Corresponding sources and build
recipes are in the release's runtime sources archive. Bundled tools are updated
through a new verified SoundGeo release; self-updating files inside an existing
bundle would invalidate its integrity checks.
