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
It can download missing flight data from free sources.
It processes and keeps recordings on your computer.

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

- A local video or audio file.
- FFmpeg to read the audio.
- Flight position data for a possible recording date.
- A possible date or date range, if you use automatic downloads.

## Use the desktop application

1. Start `soundgeo.exe`.
2. Select a recording, or drag the file into the window.
3. Keep **Download missing flight data automatically** selected.
4. Enter a possible UTC date. You can select a date range of up to 31 days.
5. Select a folder for the downloaded data.
6. Leave **Recording time** empty if you do not know the start time.
7. Set a search area if you know the region.
8. Enter an approximate ground elevation, or select **Download missing terrain from free sources**.
9. Select **Analyze recording**.
10. Read the result and its warnings. Export the JSON report if necessary.

Put FFmpeg on `PATH`, beside the application, or select its path in the advanced settings.
A new daily flight archive can require several GB of storage.
The program checks downloaded files and uses valid files again.

Select **Worldwide: ADSB.lol** for the worldwide archive.
For a first-of-month recording, you can also select **Free evaluation sample:
first of each month**. Set a search area. This source is limited to testing and
evaluation. It downloads the full day's traces for aircraft found in 288
worldwide snapshots, spaced five minutes apart. The report and cache manifest
list missing traces.
For a recording near Chicago, you can select **Chicago: public airport replay**.
For a recording near Los Angeles, select **LAX: public airport replay**.
The Chicago source contains local airport radar data. It keeps about 90 days of history.
It can contain positions that are missing from the worldwide archive.

To use existing flight data, clear the automatic-download option. Then select the flight data file or folder.

Select **Try a synthetic demo** to create a test recording and example flight data.
The demo runs offline.

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

To download during analysis, use `--provider adsb-exchange-sample` with `--date`
and `--cache`. Put the search bounds in the job file. Leave `start_utc` as `null`.
The app uses the provider's [public sample access](https://www.adsbexchange.com/pull-data/).
The [sample page](https://www.adsbexchange.com/data-products/sample-data/)
sets out the dates and terms of use. A missing snapshot stops the download. Missing
aircraft traces are listed in `sample-manifest.json` and in the report warning.
The app checks cached file hashes and keeps damaged cache bytes before repair.

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

Before it compares sound curves, the flight-corridor search checks its hypothesis budget.
If the search needs more hypotheses than the limit, it stops without a partial result.
The check uses the same proposed locations as the search.
To retry with a larger limit, use `--max-hypotheses`:

```powershell
soundgeo-cli.exe analyze --config "job.json" --max-hypotheses 50000000 --output "report-larger-search.json"
```

The limit can be between 100 and 100,000,000.
This option sets the search budget for that run. The report records the limit used.

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

Install Rust stable, the native build tools for your platform, and FFmpeg.
Windows requires the MSVC C++ build tools.
Linux requires X11, Wayland, and OpenGL development libraries, plus a file dialog portal.
The [CI configuration](.github/workflows/ci.yml) lists the platform dependencies.

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
