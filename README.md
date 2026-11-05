# SoundGeo

SoundGeo uses aircraft sounds to search for the location of a recording.
It accepts a video file or an audio file.
It compares changes in sound frequency with recorded aircraft positions.
It can search when the recording start time is unknown.

The project uses Rust. It has a desktop application and a command-line application.
It can download missing flight data from free sources.
It keeps recordings on your computer. It does not require an AI service or an API key.

## Source video

This project is based on the method in [colsto's YouTube video](https://www.youtube.com/watch?v=UytYrhYLOKI).
The video explains how aircraft sounds can give information about a recording location.
Colsto also provides the [original audio recording](https://www.youtube.com/watch?v=4CxVxuMVkMc).

SoundGeo is an independent implementation. It does not contain the creator's original program.

## Current status

**The program does not yet find locations reliably in real recordings.**

Ten public-camera recordings were tested without their recording start times.
The tests used known dates and broad search areas.
The completed benchmark used one fixed build for all ten recordings.
Nine runs returned reports. One Midway run reached its 50-million-hypothesis limit.
Two first-choice locations were within 250 m.
Midway was 28 m away. Lake Hood was 46 m away.
Both results remain ambiguous. No recording passed the full location test.
The failed Midway run previously returned a 92 m result on an older build.
The Midway recording now 28 m away previously returned first choices 24.6 km and 26.9 km away.
A later Lake Hood recording returned a first choice 5.22 km away after a larger search.
The November 3 Lake Hood recording returned a first choice 6.78 km away.
Midway arrivals returned a first choice 26.66 km away with ADSB.lol data.
An earlier run with Chicago airport data returned a first choice 236 m away.
These runs used different builds and sources. Both results remain ambiguous.
A fixed follow-up with the main matcher and automatic Chicago data returned
the same 236 m first choice. Its second choice is 488 m away.
The other planned Chicago comparison is still running.

A separate search experiment improved the later Lake Hood result to 83 m.
Its next candidate is 479 m away. The result remains ambiguous.
Two new Lake Hood recordings test the same fixed experimental build.
The first returned a location 8.56 km away, with three matches. It failed.
The second is still running. The 83 m improvement has not generalized to the first new recording.

Two further one-hour Midway recordings were selected before testing.
Both returned no location on their planned test build.
They used the free Chicago flight data and no recording start times.

The latest changes have not completed this full benchmark.
A newer Southampton run returned one model match 7.6 km from the camera.
The current model checks can therefore accept a wrong location.

All 76 software tests pass locally and in
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42047220138).
Formatting, strict Clippy, documentation tests, and both release builds also pass.
The Chicago download and data checks pass.
Its first broad-area location test reached the search limit.
A run with a larger search limit produced the 236 m Midway result.
Software test success does not establish location accuracy.
See the [validation record](validation/README.md) for results, data sources, and failures.

## Requirements

- A video or audio recording between 20 seconds and two hours long.
- FFmpeg to read the audio.
- Flight position data for a possible recording date.
- A possible date or date range, if you use automatic downloads.
- Enough aircraft sounds and flight positions to support the same location and recording start time.

Use a stationary microphone. Use an unedited recording when possible.
Wind, changing engine speed, and missing flight positions can prevent a useful result.
The program cannot determine the recording date from every video.

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
For a recording near Chicago, you can select **Chicago: public airport replay**.
The Chicago source contains local airport radar data. It keeps about 90 days of history.
It can contain positions that are missing from the worldwide archive.
It has not yet passed the real-recording location tests.

To use existing flight data, clear the automatic-download option. Then select the flight data file or folder.

Select **Try a synthetic demo** to create a test recording and example flight data.
The demo does not require flight downloads. It does not prove accuracy with real recordings.

## Use the command line

Use `soundgeo-cli.exe` for scripts and scheduled tasks.
Processing commands do not open a window or request interactive input.
The `view` command opens the desktop application.

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
The program checks every 30-second part of the selected UTC day.
If a part is missing or invalid, the download fails and keeps the valid parts for the next attempt.
An archive can still have gaps in flight observations.

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
A request can contain up to 64 tiles. Each expanded tile uses about 3 MB or 26 MB.
Use a smaller search area if the request exceeds this limit.

The program checks the compressed data and tile size before it saves a tile.
It calculates SHA-256 hashes and checks cached tiles before use.
These hashes check local file integrity; they are not publisher-signed checksums.
A failed download stops the analysis. Valid earlier tiles remain in the cache.
The report records the source URL and file hash for each downloaded terrain tile.
Provider credits are saved as `TERRAIN-ATTRIBUTION.txt` in the cache.

Terrain gives ground height. It does not give the microphone height on a building or tower.
Missing height samples use the configured fallback elevation and add a report warning.
Terrain improved one local Chicago diagnostic. It did not remove competing locations.
That diagnostic does not establish broad-area location accuracy.

To use existing terrain without downloads, set `terrain` to its folder in the job file.
To allow missing tiles to download, also set `download_terrain` to `true`.
The desktop app has the same option under **Search area & terrain**.

### Settings and paths

Use a job JSON file to set the search area, terrain, search limits, and aircraft filters.
For example, save this as `region.json` for an unknown start time near Chicago Midway:

```json
{
  "media": "recording.mp4",
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
Set `bounds` to `null` to search the area covered by the imported flight data.
Large searches can take much longer than small searches.

If a search reaches its hypothesis limit, it stops without a partial result.
To retry with a larger limit, use `--max-hypotheses`:

```powershell
soundgeo-cli.exe analyze --config "job.json" --max-hypotheses 50000000 --output "report-larger-search.json"
```

The limit can be between 100 and 100,000,000.
This option changes the search budget for that run. It does not change the saved job,
search area, flight data, or recording time.
The report records the limit used. A larger limit can take much longer and does not guarantee a result.

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
This option checks the model status. It does not verify the real location.

| Report status | Meaning |
| --- | --- |
| `insufficient_evidence` | No location passed the model checks. |
| `ambiguous` | More evidence is necessary, or different answers fit the recording. |
| `candidates_found` | A result passed the current model checks. Accuracy still requires verification. |

| Exit code | Meaning |
| --- | --- |
| `0` | Analysis finished. This can include an empty result. |
| `1` | An input, processing, or file error occurred. |
| `2` | A command-line argument was invalid. |
| `3` | Analysis found no candidate, and `--require-match` was set. |
| `4` | A result remained ambiguous, and `--require-unambiguous` was set. |

The `--require-unambiguous` option also returns code `3` when there is no candidate.
The program writes the report before it returns code `3` or `4`.
The `--require-match` option checks for candidates. It does not check their accuracy or reject an ambiguous result.
Check the report status and supporting evidence in automation.
Do not treat `candidates_found` as a verified location.
A clock-rate fit with only three matched sounds remains provisional.

## Flight data

Automatic downloads use the [free ADSB.lol archive](https://www.adsb.lol/docs/open-data/historical).
The program can also read CSV files, readsb JSON files, compressed traces, and split TAR archives.
Select the first `.tar.aa` file when you use a split archive.

CSV files require these column names. Altitude values must use metres.
Time values can use Unix seconds or RFC3339 with a time zone.

```csv
aircraft,time,lat,lon,altitude_m
example,2026-09-08T10:00:00Z,35.0000,-110.0200,1200
example,2026-09-08T10:00:02Z,35.0000,-110.0184,1200
```

Each flight needs at least eight positions. Gaps between positions must not exceed 60 seconds.
Use WGS84 coordinates. Use geometric altitude when it is available.
Use consistent altitude units and reference levels.

A downloaded archive can still have gaps in its flight positions.
A successful download does not prove that the data includes each audible aircraft.
The program reports missing dates and damaged files. It can use a complete cache without an internet connection.

The archive uses the [ODbL 1.0 licence](https://opendatacommons.org/licenses/odbl/1-0/).
The downloader saves a licence notice. Follow the licence when you distribute flight data or derived databases.

See the [technical reference](docs/technical-reference.md) for format details, altitude rules, search limits, and the sound model.

## How the search works

1. Read the recording audio with FFmpeg.
2. Find frequency changes that could come from passing aircraft.
3. Compare those changes with historical flight positions.
4. Search for one location and one recording start time that explain the matched sounds.
5. Report the possible locations and their supporting evidence.

The model includes the time that sound takes to reach the microphone.
It can correct a small, constant clock-rate error in a long recording.
It cannot repair edits that remove or repeat parts of the recording.

Missing matches remain visible in the report. Error percentages describe model fit; they are not probabilities.
A map link sends the selected coordinates to OpenStreetMap when you open it.

## Build and test

Install Rust stable, the native build tools for your platform, and FFmpeg.
Windows requires the MSVC C++ build tools.
Linux requires X11, Wayland, and OpenGL development libraries, plus a file dialog portal.
The [CI configuration](.github/workflows/ci.yml) lists the platform dependencies.

```powershell
cargo build --release --locked
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --locked
```

The build creates `soundgeo` and `soundgeo-cli` in `target/release`. Windows adds the `.exe` extension.
Run `cargo run --release` to start the desktop application from source.

Tests include generated videos, unknown recording times, missing data, damaged downloads, cancellation, and command-line use.
Real-recording accuracy tests are described separately in the [validation record](validation/README.md).
