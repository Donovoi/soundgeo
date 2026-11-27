# SoundGeo

SoundGeo is an open-source intelligence (OSINT) tool in development.
It uses aircraft sounds to search for the location of an audio or video recording.
It is intended for existing samples from the internet, with different lengths and quality.
These can include noisy audio, compressed uploads, and edited clips.
Recording times and capture conditions can be unknown.

The app accepts local video and audio files.
It compares changes in sound frequency, or the timing of sound peaks, with recorded aircraft positions.
It can search when the recording start time is unknown.

The project uses Rust. It has a desktop application and a command-line application.
It can download recordings from media/page URLs and missing flight data from free sources.
It processes and keeps recordings on your computer.
Optional speech analysis transcribes the original language and translates it into English using local open-source models.

## Source video

This project is based on the method in [colsto's YouTube video](https://www.youtube.com/watch?v=UytYrhYLOKI).
The video explains how aircraft sounds can give information about a recording location.
Colsto also provides the [original audio recording](https://www.youtube.com/watch?v=4CxVxuMVkMc).

SoundGeo is an independent implementation.

## Project status

SoundGeo is in development. The current location target is within 1 km.
See [limitations](docs/limitations.md) for input coverage and model constraints.
The [validation record](validation/README.md) contains test results, sources, and build checks.

## Requirements

Download the Windows x64 EXE or Linux x64 archive from [Releases](https://github.com/Donovoi/soundgeo/releases).
On Windows, run the EXE: media tools, URL downloaders, English OCR and software graphics are included.
See [installation instructions](docs/installation.md) for Linux and console scripts.
Releases marked **Pre-release** are experimental builds.
The Windows bundle includes FFmpeg under LGPL 2.1 or later. Corresponding sources
and dependency notices are provided with each release.

- A video/audio file or an HTTP(S) link to the recording or its page.
- FFmpeg to read the audio (included in the Windows EXE).
- A free flight-data source, or a prepared flight cache.
- A folder for downloaded data.

## Use the desktop application

Hover over any app button for an explanation of its action and what to do next.
Tooltips also cover disabled buttons, checkboxes, dropdown choices, result tabs,
links and expandable section headings.

URL discovery and download progress, cancellation and errors appear beside the
URL controls. A completed download shows its title, size and duration and selects
it as the recording. Downloading does not start location analysis: set the flight
data, then choose **Analyze downloaded recording** or **Analyze recording**.

1. Start `soundgeo.exe`.
2. Select a recording, drag a file into the window, or open **Or paste a video / page URL**. Paste the link, select **Find media**, choose the clip, then **Download and use selected clip**.
3. Keep **Download missing flight data automatically** selected.
4. Leave the dates blank to find and search available days. You can also enter a UTC date range.
5. Select a folder for the downloaded data.
6. Leave **Recording time** empty if you do not know the start time.
7. Set a search area if you know the region.
8. Enter an approximate ground elevation, or select **Download missing terrain from free sources**.
9. Select **Analyze recording**.
10. Read the result and its warnings. Export the JSON report if necessary.

The Windows EXE selects its bundled tools automatically. For other installations,
put FFmpeg on `PATH`, beside the application, or select its path in advanced settings.
A new daily flight archive can require several GB of storage.
The program checks downloaded files and uses valid files again.
A large archive uses parallel, resumable aria2 transfers. Concurrency adjusts to
available bandwidth, with up to 16 files at once. Small authenticated flight
requests and terrain tiles also download concurrently.
A split archive must be complete before the app marks it ready.
When online, it can upgrade a saved ADSB.lol staging archive to a production
archive. It keeps the previous files until the replacement is verified.
With blank dates, online analysis discovers the provider's available days and
downloads missing files as it searches. Offline analysis searches prepared days.

Select **Worldwide: ADSB.lol** for the worldwide archive.
For a first-of-month recording, you can also select **Free evaluation sample:
first of each month**. Set a search area. This source is limited to testing and
evaluation. It downloads the full day's traces for aircraft found in 288
worldwide snapshots near five-minute intervals. The report and cache manifest
list missing traces, unavailable snapshot slots, and snapshots taken from neighboring minutes.
For a recording near Chicago, you can select **Chicago: public airport replay**.
For a recording near Los Angeles, select **LAX: public airport replay**.
The Chicago source contains local airport radar data. It keeps about 90 days of history.
It can contain positions that are missing from the worldwide archive.

To use existing flight data, clear the automatic-download option. Then select the flight data file or folder.

Select **Try a synthetic demo** to create a test recording and example flight data.
The demo runs offline.

## Optional transcription and translation

Tick **Transcribe speech and translate to English** after choosing a recording.
SoundGeo finds speech, tries to detect its language, and downloads the required
model files with progress messages. The large recognition and translation
weights stay in a reusable cache outside the EXE. English recordings skip the
translation download. No account or API key is needed.

The initial languages are English, Mandarin Chinese, Hindi, Spanish, Arabic,
French, Bengali, Portuguese, Indonesian and Urdu. Choose the language when
detection is uncertain. Hindi and Urdu require a script choice.
Select **Analyze speech only** to work without flight data, or use
**Analyze recording** for speech and geolocation. Review the original text and
English translation, then export JSON, TXT, SRT or VTT.

The default quality translator downloads 5.15 GB and needs about 10 GB of free
memory. The compact translator downloads 1.35 GB. Recognition weights download
separately for the chosen language. See [speech analysis](docs/speech.md) for
cache locations, command-line use and model details, and
[limitations](docs/limitations.md#speech-analysis) before relying on a transcript.

## Download a recording from a URL

The Windows EXE includes yt-dlp, gallery-dl, curl_cffi browser impersonation,
Deno for YouTube JavaScript challenges, and aria2. SoundGeo tries yt-dlp first,
retries discovery with Chrome impersonation, then tries gallery-dl. You can
also choose either downloader or force impersonation in the app.

```powershell
soundgeo-cli.exe fetch-media --url "https://example.org/video-page" --list
soundgeo-cli.exe fetch-media --url "https://example.org/video-page" --item 2
soundgeo-cli.exe analyze --url "https://example.org/video-page" --media-item 2 --config job.json --output report.json
```

The selected clip keeps its video and audio. Source links, tool versions and a
file hash are saved with the local download. Page titles, dates and location
metadata do not set the geolocation search inputs. See [URL intake](docs/url-intake.md)
for supported input types, cache use and command options.

## Check a recording before preparing flight data

Check the audio with the default detectors, without a date, search area or flight cache:

```powershell
soundgeo-cli.exe check-audio --input recording.mp4 --output audio-check.json
```

This check always runs offline. It records the file hash, decoded duration,
detected tones and sound peaks, and whether each method has enough observations
to attempt a search. It preserves existing output files. Add `--require-searchable`
to return exit code 3 when both methods lack enough evidence.
The check does not establish a location. Review the source for edits, added audio
and camera movement before running a location search.

## Prepare data for offline use

To list available days without downloading flight files:

```powershell
soundgeo-cli.exe dates --provider webtrak-chicago --cache flight-cache --output available-dates.json
```

To prepare all available days, omit the dates:

```powershell
soundgeo-cli.exe download --provider webtrak-chicago --output flight-cache
```

Download flight data for the period you want to keep available. The worldwide and airport downloaders also
prepare the aircraft catalog. Add terrain bounds and a terrain cache if needed:

```powershell
soundgeo-cli.exe download --date 2026-09-01 --date-to 2026-10-31 --provider webtrak-chicago --output flight-cache --terrain-cache terrain-cache --south 41.65 --north 42.01 --west -87.98 --east -87.5
```

Keep the full cache folders. You can copy them to another computer with SoundGeo
and FFmpeg installed. Run the same download command to reuse complete files and
finish an interrupted download.

Analyze offline with the prepared cache:

```powershell
soundgeo-cli.exe analyze --config region.json --input recording.mp4 --cache flight-cache --provider webtrak-chicago --terrain-cache terrain-cache --offline --output report.json
```

Leave dates out of the command and job file to search every prepared date offline. The report estimates
the recording time from the matched sounds. Batched reports also list the dates searched.
Longer searches run in overlapping daily batches. This covers recordings across
midnight and keeps flight data for at most two adjacent days in memory at once.
The app compares results across all batches before reporting a location.

Offline mode disables network access during analysis. It reports missing or
invalid cache files. Prepare those files online, then repeat the offline run.

In the desktop app, use **Prepare data for offline use** with the selected flight
provider, date range, and terrain settings. Then select **Offline mode: use cached
data only**. Leave the dates blank to search the prepared cache.

## Optional video clues

SoundGeo can read visible place names and clocks from a video. This option uses
local text recognition. It produces a separate JSON report with the text, frame
times, and matching places on a map. Audio searches keep their own results.

In the desktop app, open **Video clues (optional)** and tick **Read video clues
during analysis**, then choose **Analyze recording**. SoundGeo checks for a video
stream, prepares missing place data, reads the text, and continues the location
analysis. Progress appears as it works. A video-clue error is shown alongside the
results and does not stop location analysis. Completed clues remain available if
the flight-data setup needs attention.

The default **ALL** catalog covers the world and is a large download. Enter a
two-letter country code such as `GB`, `US` or `AU` for a smaller catalog when
appropriate. This selects the place names that can match; it does not set the
location-search area. Downloaded data is verified and reused from the app's cache,
including in offline mode. Audio-only recordings do not download place data.

Use **Read video clues only** to scan a video without running location analysis.
Hover over each control for instructions. **Advanced video settings** contains
optional overrides:

| Control | Purpose |
| --- | --- |
| Choose GeoNames .txt… | Use an already extracted place-name database instead of automatic preparation. |
| Use automatic place data | Clear the database override and use the selected country's cache. |
| Prepare place data… | Prepare the selected country's data in a chosen folder, for example before going offline. |
| Choose Tesseract… | Select an OCR executable if the bundled or system tool is unsuitable. |
| Choose Tesseract language folder… | Select the folder containing `eng.traineddata`. Video OCR currently uses English. |
| Use default OCR tools | Clear the OCR overrides and return to the bundled or system tools. |

The **Save video clue report…** button exports the text, frame times, matches and
source hashes as JSON. Review the text and named places alongside the location
results; see the [video-clue limitations](docs/limitations.md).

The Windows EXE includes Tesseract and English language data. On other platforms,
install [Tesseract 5](https://tesseract-ocr.github.io/tessdoc/Installation.html)
with its English model and keep FFmpeg and FFprobe together. Prepare free
[GeoNames](https://www.geonames.org/) place data before going offline:

```powershell
soundgeo-cli.exe video-data --country ALL --output video-data
```

Use a two-letter country code such as `US` to prepare one country. GeoNames data
uses the [CC BY 4.0 license](https://creativecommons.org/licenses/by/4.0/).
Copy the `.txt` and `.source.json` files to the offline computer. The Windows EXE
contains the required tools; other installations also need Tesseract, its
`tessdata` folder, FFmpeg and FFprobe on that computer.

```powershell
soundgeo-cli.exe video-clues --input recording.mp4 --gazetteer video-data/allCountries.txt --offline --output video-clues.json
```

Use `--tesseract`, `--tessdata`, and `--ffmpeg` to set tool paths if needed.
Use `--frames 6` to read more frames. Use `--output -` for JSON on standard output.
Read JSON output as UTF-8.
The `--require-match` option returns exit code 3 if no place names match.
Read all entries in `places` and the source text in `frames`.

## Use the command line

Use `soundgeo-cli.exe` for scripts and scheduled tasks.
Processing commands run without a window or interactive prompts.
The `view` command opens the desktop application.

To use flight passage timing, add `--method passage-timing`:

```powershell
soundgeo-cli.exe analyze --config "region.json" --input "recording.mp4" --method passage-timing --date 2026-11-04 --provider webtrak-chicago --cache "flight-cache" --output "report.json"
```

Set the search area in `region.json`. Leave `start_utc` empty or set it to `null`.
The app compares the result with 19 versions that shuffle blocks of sound peaks.
It also checks for competing locations across the searched area.
Timing details are in the report's `timing` field. For a result with status
`candidates_found`, read the area center from `timing.region.latitude` and
`timing.region.longitude`. The region covers the competitive search points.
Its `enclosing_radius_m` field gives the radius of that region.
Older reports can have no region field. Individual search candidates remain in
`timing.search.candidates`. The top-level `candidates` field is for Doppler results.
Reports preserve diagnostic results from failed checks.

In the desktop app, select **Flight passage timing** under **Sound matching method**.
Set a search area before you start. The default method for older jobs remains **Doppler curves**.

The timing search uses aircraft within 500 m of each candidate position by default.
To include more distant aircraft, add this setting to the job file:

```json
"timing_options": {
  "maximum_passage_distance_m": 1000
}
```

This is the three-dimensional distance from the aircraft to the candidate position.
Set it between 100 and 5,000 m.

Use existing flight data:

```powershell
soundgeo-cli.exe analyze --input "recording.mp4" --tracks "flight-data" --output "report.json"
```

Download missing flight data. Use the search area and other settings in `region.json`:

```powershell
soundgeo-cli.exe analyze --config "region.json" --input "recording.mp4" --date 2026-11-03 --cache "flight-cache" --output "report.json"
```

Select the free Chicago airport source:

```powershell
soundgeo-cli.exe analyze --config "region.json" --input "recording.mp4" --date 2026-11-03 --provider webtrak-chicago --cache "flight-cache" --output "report.json"
```

To download a day before analysis, use `download`:

```powershell
soundgeo-cli.exe download --date 2026-11-03 --provider webtrak-chicago --output "flight-cache"
```

The [Chicago Department of Aviation](https://www.flychicago.com/community/ordnoise/flighttracker/pages/default.aspx) provides the public flight tracker.
For Los Angeles, use `--provider webtrak-lax` instead.
[Los Angeles World Airports](https://www.lawa.org/lawa-environment/noise-management/lawa-noise-management-lax/lax-internet-flight-tracking-system) provides that tracker.
These sources cover their local radar areas and keep about 90 days of data.
The program checks every 30-second part of the selected UTC day.
If a part is missing or invalid, the download fails and keeps the valid parts for the next attempt.

For the free first-of-month evaluation sample, specify a search area:

```powershell
soundgeo-cli.exe download --date 2026-11-01 --provider adsb-exchange-sample --south 17.85 --north 18.25 --west -63.35 --east -62.95 --output "flight-cache"
```

To download during analysis, use `--provider adsb-exchange-sample` and `--cache`.
Leave dates blank to discover available archives. Put the search bounds in the
job file. Leave `start_utc` as `null`.
The app uses the provider's [public sample access](https://www.adsbexchange.com/pull-data/).
The [sample page](https://www.adsbexchange.com/data-products/sample-data/)
sets out the dates and terms of use. Source gaps are listed in
`sample-manifest.json` and in the report warning.
The app checks cached file hashes and keeps damaged cache bytes before repair.
Repeat the download command to resume preparation. Verified completed days are
reused. Add `--refresh-gaps` to retry source files listed as unavailable.
In the desktop app, use **Retry unavailable source files** for this action.

For the sample's high-resolution flight traces, use
`--provider adsb-exchange-sample-hires` in the download or analysis command.
In the desktop app, select **Free evaluation sample: high resolution**.
Both options use the same discovery snapshots. Each keeps its own flight traces.
Prepared high-resolution data can be used with `--offline` and blank dates.

To search more than one possible date, add `--date-to`:

```powershell
soundgeo-cli.exe analyze --config "region.json" --date 2026-11-01 --date-to 2026-11-03 --cache "flight-cache" --output "report.json"
```

To write only JSON to standard output, use `--output - --quiet`:

```powershell
soundgeo-cli.exe analyze --config "job.json" --input "next.mp4" --output - --quiet
```

Progress messages go to standard error. The `--quiet` option removes progress messages but keeps errors.
Use `soundgeo-cli.exe --help` or `soundgeo-cli.exe analyze --help` to see the available options.

### Download terrain

Set a search area in the job file. Add `--terrain-cache` to download its ground heights:

```powershell
soundgeo-cli.exe analyze --config "region.json" --date 2026-11-03 --cache "flight-cache" --terrain-cache "terrain-cache" --output "report.json"
```

To download terrain before analysis:

```powershell
soundgeo-cli.exe terrain --south 41.65 --north 42.01 --west -87.98 --east -87.5 --output "terrain-cache"
```

The program uses free [Mapzen Terrain Tiles](https://registry.opendata.aws/terrain-tiles/).
No account or API key is needed. Each tile covers one degree of latitude and longitude.

The program checks the compressed data and tile size before it saves a tile.
It calculates SHA-256 hashes and checks cached tiles before use.
A failed download stops the analysis. Valid earlier tiles remain in the cache.
The report records the source URL and file hash for each downloaded terrain tile.
Provider credits are saved as `TERRAIN-ATTRIBUTION.txt` in the cache.

Terrain supplies ground height. Missing samples use the configured fallback
elevation and add a report warning.

To use existing terrain without downloads, set `terrain` to its folder in the job file.
To allow missing tiles to download, also set `download_terrain` to `true`.
The desktop app has the same option under **Search area & terrain**.

### Settings and paths

Use a job JSON file to set the search area, terrain, search limits, and aircraft filters.
For example, save this as `region.json` for an unknown start time near Chicago Midway:

```json
{
  "media": "recording.mp4",
  "analysis_method": "passage_timing",
  "start_utc": null,
  "bounds": {"south": 41.65, "north": 42.01, "west": -87.98, "east": -87.5},
  "ground_m": 180,
  "ground_datum": "egm96"
}
```

Use this file with the download command examples above.
Replace the area and ground height for your recording. The height is in metres above mean sea level.
The program uses its default values for settings that are not listed.

The `demo` command creates a complete example in `demo/job.json`.

```powershell
soundgeo-cli.exe demo --output demo
soundgeo-cli.exe analyze --config demo/job.json --output reports/demo.json
```

Set `start_utc` to `null` when the recording start time is unknown.
For Doppler searches, set `bounds` to `null` to search the area covered by the imported flight data.
Flight passage timing requires a search area.

The flight-corridor search divides large jobs into batches automatically.
It checks each batch before it compares sound curves. Matching keeps access
to all flight tracks across batch boundaries.
Use `--max-hypotheses` to set the budget for each batch:

```powershell
soundgeo-cli.exe analyze --config "job.json" --max-hypotheses 50000000 --output "report-larger-search.json"
```

The limit can be between 100 and 100,000,000.
The report records the limit and each automatic division. Total work across
dates and proposal batches can exceed the limit for one batch.

Use `--search-threads` to set the maximum CPU workers for flight-corridor matching:

```powershell
soundgeo-cli.exe analyze --config "job.json" --search-threads 8 --output "report.json"
```

The default is up to four workers. The program caps the worker count at the
available CPU capacity. The progress log shows the count used. In the desktop
app, set **Flight-corridor workers** under **Advanced audio & search**.

Paths in a job file are relative to that file.
Paths supplied with `--input`, `--tracks`, `--cache`, and `--terrain-cache` are relative to the current directory.
These command-line options replace the related job settings.
The `--date` option selects automatic downloads. It takes priority over `--tracks`.

### Results and exit codes

The report contains possible locations, detected sounds, matched flights, warnings, and the settings used.
Each possible location includes an estimated recording start time and the evidence for each match.

To reject ambiguous results in a script, add `--require-unambiguous`:

```powershell
soundgeo-cli.exe analyze --config "job.json" --output "report.json" --require-unambiguous --quiet
```

The program saves the report before it returns a result code.
This option checks the report status.

| Report status | Meaning |
| --- | --- |
| `insufficient_evidence` | No location passed the model checks. |
| `ambiguous` | More evidence is necessary, or different answers fit the recording. |
| `candidates_found` | A result passed the current model checks. |

| Exit code | Meaning |
| --- | --- |
| `0` | Analysis finished. This can include an empty result. |
| `1` | An input, processing, or file error occurred. |
| `2` | A command-line argument was invalid. |
| `3` | Analysis found no candidate, and `--require-match` was set. |
| `4` | A result remained ambiguous, and `--require-unambiguous` was set. |

The `--require-unambiguous` option also returns code `3` when there is no candidate.

For Doppler analysis, the location screen uses the current 1 km target and
allows a two-thirds sound consensus. Competitive recording times can remain
uncertain even when the location screen passes; see `doppler_assessment` in
the JSON report and the [screening limits](docs/limitations.md#search-and-result-interpretation).
The program writes the report before it returns code `3` or `4`.
The `--require-match` option checks for candidates.
Check the report status and supporting evidence in automation.

## Flight data

The default download source is the [free ADSB.lol archive](https://www.adsb.lol/docs/open-data/historical).
Other choices are the Chicago and LAX public airport replays, and the
[ADS-B Exchange free evaluation sample](https://www.adsbexchange.com/data-products/sample-data/).
The program can also read CSV files, readsb JSON files, compressed traces, and split TAR archives.
Select the first `.tar.aa` file when you use a split archive.

CSV files require these column names. Altitude values must use metres.
Time values can use Unix seconds or RFC3339 with a time zone.

```csv
aircraft,time,lat,lon,altitude_m
example,2026-09-08T10:00:00Z,35.0000,-110.0200,1200
example,2026-09-08T10:00:02Z,35.0000,-110.0184,1200
```

Passage timing needs at least three recorded positions per flight segment.
Doppler matching needs at least eight. Gaps between positions must not exceed 60 seconds.
Use WGS84 coordinates. Use geometric altitude when it is available.
Use consistent altitude units and reference levels.

The program reports missing dates and damaged files. It can use a complete cache without an internet connection.

For offline archive searches covering at least two prepared dates, add
`--date-checkpoints checkpoints` to the
`analyze` command. Repeat the same command after an interruption to reuse completed
date batches and fully refined flight-proposal batches within unfinished dates.
The proposal batch running at interruption is repeated. The final report still
requires every planned proposal batch and date. Keep the checkpoint folder
outside the flight and terrain folders.
The SoundGeo executable, recording, settings, dates, reference files and terrain
must stay unchanged. Audio is decoded again on each run; its extracted observations
must also match before results are reused. Use a new checkpoint folder after
changing these inputs. The program checks every planned date batch before returning
a location report.

The ADSB.lol archive uses the [ODbL 1.0 licence](https://opendatacommons.org/licenses/odbl/1-0/).
Other providers have separate terms. The downloader saves a source notice.
Follow each source's terms when you use or distribute its data.

See the [technical reference](docs/technical-reference.md) for format details, altitude rules, search limits, and the sound model.

## How the search works

1. Read the recording audio with FFmpeg.
2. Find sound peaks or frequency changes that could come from passing aircraft.
3. Compare those sounds with historical flight positions.
4. Search for one location and one recording start time that explain the matched sounds.
5. Report the possible locations and their supporting evidence.

The model includes the time that sound takes to reach the microphone.
The Doppler method can correct a small, constant clock-rate error in a long recording.
Flight passage timing fits a shared start time.

Reports include unmatched sounds and model fit errors.
A map link sends the selected coordinates to OpenStreetMap when you open it.

## Build and test

Run builds and tests inside a VM. Install Rust stable, Python 3.14, CMake, Git,
the native build tools for your platform, and FFmpeg.
Windows requires the MSVC C++ build tools.
Linux requires X11, Wayland, and OpenGL development libraries, plus a file dialog portal.
The [CI configuration](.github/workflows/ci.yml) lists the platform dependencies.

Prepare the pinned speech helpers, small detectors and fonts before testing.
This does not download the large recognition or translation weights:

```powershell
python scripts/build_speech_tools.py
python scripts/prepare_speech_test_tools.py
$env:SOUNDGEO_TEST_SPEECH_FONTS = (Resolve-Path "target/release/assets/speech-fonts").Path
```

On Linux, use `python3` and set
`export SOUNDGEO_TEST_SPEECH_FONTS="$PWD/target/release/assets/speech-fonts"`.
URL tests also need the downloaders described in the
[installation instructions](docs/installation.md#linux-x64) on PATH; CI prepares
their pinned versions before testing.

```powershell
cargo fmt --all -- --check
cargo clippy --release --all-targets --locked -- -D warnings
cargo test --release --locked --all-targets
cargo test --release --locked --doc
cargo build --release --locked --bins
```

The build creates `soundgeo` and `soundgeo-cli` in `target/release`. Windows adds the `.exe` extension.
Run `cargo run --release` to start the desktop application from source.

Tests include generated videos, unknown recording times, missing data, damaged downloads, cancellation, and command-line use.
Real-recording accuracy tests are described separately in the [validation record](validation/README.md).
