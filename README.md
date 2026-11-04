# SoundGeo

A native Rust desktop workbench that takes video or audio, extracts aircraft-like Doppler sweeps, and ranks recording locations against historical flight tracks. Unknown recording time is supported: the search fits **one shared UTC start time** across matched events, with a bounded media clock-rate correction for long recordings.

**Research software.** It returns candidate coordinates and evidence, not a guaranteed location. A video alone cannot identify its date or location without suitable reference flight records. This is an independent implementation inspired by [colsto's demonstration](https://www.youtube.com/watch?v=UytYrhYLOKI); the creator did not release that pipeline's code.

**Real-video acceptance is not met.** Nine public-camera recordings from three known locations were tested with their recording clocks withheld. The completed clock-rate run returned a top candidate 83 m from the Midway camera, supported by four flights, but still ambiguous. Another Midway clip returned a wrong ambiguous candidate 27 km away. Both Lake Hood recordings returned no candidate; the earlier incorrect 3.1 km candidate remains rejected. No recording produced an accepted unambiguous location. Dates and broad regions were supplied. See [the validation record](validation/README.md). Passing software tests must not be presented as real-world geolocation reliability.

## Desktop quick start

1. Run `soundgeo.exe` on Windows, or `cargo run --release` from source.
2. Choose or drop a video/audio file. FFmpeg must be on PATH, beside the executable, or selected in Advanced settings.
3. Leave **Download missing flight data automatically** enabled, enter a possible UTC date or a range of up to 31 days, and choose a cache folder. Analysis downloads missing parts and verifies/reuses completed archives. Each uncached day is several GB. Alternatively disable it and choose a CSV, readsb trace, split archive's `.tar.aa` file, or folder.
4. Leave Recording time empty if unknown. Supply a UTC time and tolerance when available. A date range selects reference data without supplying the recording clock. If no date information exists, choose a finite range to investigate. The app cannot infer the date from an arbitrary video alone.
5. Search area is optional: large extents automatically use indexed flight corridors. A smaller area makes searches faster. Optional SRTM terrain tiles improve the elevation model; otherwise enter an approximate ground elevation.
6. Click **Analyze recording**. Inspect candidate locations, inferred time, observed/predicted curves, and limitations. Export a JSON report.

**Try a synthetic demo** creates a noisy two-flyover recording and flight CSV locally. Click Analyze recording after it loads. It demonstrates the pipeline without flight-data downloads. Synthetic success does not establish real-world accuracy.

No AI API key is needed. Recordings are never uploaded. Enabled archive/catalog downloads and map/source links use the network. Opening a map sends the selected coordinates to OpenStreetMap.

## Flight data

CSV uses these exact headers. Time is Unix seconds or RFC3339 with a timezone; coordinates are WGS84 degrees and altitude is **metres**. Each aircraft needs at least eight points with gaps no larger than 60 seconds.

```csv
aircraft,time,lat,lon,altitude_m
example,2026-09-08T10:00:00Z,35.0000,-110.0200,1200
example,2026-09-08T10:00:02Z,35.0000,-110.0184,1200
```

Optional CSV columns `type_code` (e.g. `C172`) and `type_description` (ICAO engine description, e.g. `L1P`) enable aircraft-type filtering. All aircraft types are included by default because both propeller and short jet tones are analyzed. Choose **Known propellers only** to exclude unknowns. ICAO `L/S/A` aircraft with piston (`P`) or turboprop (`T`) engines are propellers; helicopters/gyrocopters are classified separately. Type descriptions can be resolved using the small [Mictronics catalog](https://github.com/Mictronics/readsb-protobuf/blob/dev/webapp/src/db/types.json), downloadable in the app or with `soundgeo catalog`. Archive download obtains this catalog automatically; a neighboring `aircraft-types.json` is detected and recorded in the report configuration.

Prefer geometric altitude with a consistent vertical datum. Correct pressure altitude before importing a custom CSV. Keep independent flights under separate aircraft IDs if practical. Input order is normalized, duplicate timestamps are removed, and large gaps or implausible speeds split tracks.

Readsb JSON/gzip and ADSB.lol split TAR archives are read directly. Archives are streamed, **not extracted**. Absolute base timestamps are combined with per-point offsets. Geometric altitude is preferred; pressure-altitude fallback is reported. Ground/stale/missing-position records are skipped. Readsb data is reduced to a minimum two-second interval. A selected search area/time filters records during ingestion while keeping an acoustic buffer around the region.

The downloader tries `prod-0`, then `prod-1`, then `staging-0` if a release is absent. Published SHA-256 digests are verified when supplied; every completed part also gets a local SHA-256 cache stamp. Corrupt cached parts are not reused. Interrupted/invalid partial parts are discarded, and cancellation interrupts stalled body reads. The release manifest and attribution are saved beside the archive. A pinned manifest allows fully cached analysis offline. Date-range analysis loads only the requested dates, reports the exact reference paths, and fails if any requested day is unavailable; unrelated cached dates are not included. This does not prove coverage completeness or combine replicas automatically. Directory import merges multiple days. Near midnight, include adjacent days to retain crossing flights.

References: [ADSB.lol historical archive](https://www.adsb.lol/docs/open-data/historical), [readsb trace schema](https://github.com/wiedehopf/readsb/blob/dev/README-json.md). The archive is licensed under [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/); a licence notice is saved with downloads. Respect attribution and applicable share-alike terms when redistributing derived databases.

## Method

- FFmpeg decodes the first audio stream to 8 kHz mono. Input files are unchanged.
- A Hann-window STFT (16384 samples, 0.5-second hop) exposes low-frequency tones. A second scale (1024 samples, 0.025-second hop, 500–3900 Hz) handles fast jet sweeps. The original 500–1900 Hz band and the 1400–3900 Hz extension have separate tracing budgets. Extended-band detections additionally need a corresponding harmonic across the moving frequency pattern. Local spectral-median subtraction reduces interference from bass/wind. Viterbi tracing searches overlapping 90-second windows for slow curves and 10/20-second windows for short passes. Sustained signal contrast, sweep shape and a propagation-aware flyover fit reject weak/noisy traces and stationary tones. A detected curve and each reported flight match must reduce squared error by at least half compared with a freely fitted straight frequency drift on the same samples; a descending line alone is insufficient timing evidence.
- Overlapping ridges/harmonics count as one event. Up to 24 independent events are retained; the strongest eight seed the bounded search, and all retained events can corroborate them.
- Regional searches use a spatial grid. Large-area searches index flight positions in time/ECEF buckets and generate observer hypotheses on both sides of aircraft corridors at a fixed spatial spacing, across several slant-distance estimates. Country extent does not force a coarser grid. Candidate flights for later events are retrieved from this index using both arrival-time and distance constraints. Unknown time comes from received closest approach, not the file's creation timestamp.
- For three or more detections, at least three distinct flight segments must fit; smaller recordings require every event. Coarse ranking and exact refinement use the same minimum consensus, so numerous weak associations do not automatically outrank three precise ones. Additional associations must preserve the joint error threshold, so weaker fits cannot invalidate a passing three-flight core. Unmatched detections remain explicit outliers. Partial consensus is reported as ambiguous, and no per-event clock shifts are allowed.
- Flight positions are smoothed with a centered 10-second local linear regression before velocity estimation (`smooth_tracks: false` disables this). Timestamps, segment endpoints and gaps are preserved. Up to 512 distinct candidate basins are refined using interpolated WGS84 ECEF positions. The model solves `t_receive = t_emit + range(t_emit) / c` analytically within each constant-velocity segment, computes radial velocity at emission, then fits one source tone per event and a common recording start. It never extrapolates across gaps.
- Quiet gaps between detected events provide a silence constraint with ten-second edge guards. A known propeller predicted to pass within 80% of the closest observed flyover's slant distance is recorded as a conflict. Default mode adds a ranking penalty; strict mode rejects conflicting candidates; off mode disables this evidence. Unknown types do not create silence conflicts. **Audibility is an assumption**, so strict mode can reject a true location.
- Reports retain input paths/configuration, observed ridges, matched segment IDs and original event indices, modeled frequencies, errors, the linear-drift baseline residual, warnings and inferred time. Error percentages are residual fit metrics, not probabilities.

## Limits and scope

- Synthetic two-aircraft recovery and negative controls are tested. **Reliable real-world performance has not been established.** Displayed decimal places and refinement spacing are not measured uncertainty radii.
- Sparse/noisy tracks, jets without stable tones, wind, engine RPM changes, moving microphones, spliced audio, and changing weather can defeat the model. Human review is required.
- One flyover with unknown time may admit many locations/times; even multiple flyovers can be ambiguous. The tool may return no candidate.
- Both search modes are bounded numerical searches. The corridor index covers the imported date/region extent but retains a finite set of candidate basins and samples several slant distances, so it is not a proof that every possible location has been excluded. It uses local time-window closest approaches for repeated passes. A hypothesis-budget overrun returns an error and **no partial location result**. Narrow the archive, raise the budget, or change spacing before rerunning. Regional-grid budget coarsening is explicitly reported.
- Optional SRTM `.hgt` tiles are 1201×1201 or 3601×3601, big-endian signed metres, named like `N35W110.hgt`. Nearest-neighbour terrain sampling is used. SRTM heights are converted from EGM96 to WGS84 ellipsoid heights using the bundled public-domain NGA geoid grid ([source and license](assets/NOTICE.txt)). Missing/void tiles use the configured elevation and produce a warning. `ground_datum: "egm96"` converts a manual sea-level height; `"wgs84_ellipsoid"` preserves the original manual-height behavior and is the default for existing JSON jobs. New GUI jobs select EGM96. Candidate `ground_m` values are always WGS84 ellipsoid heights. Pressure-altitude corrections and atmospheric ray tracing are not implemented; EGM96 is not an exact conversion for every national height datum.
- Limits: recordings 20 seconds–2 hours, 10 million retained track points, 32 MB decoded per trace, at most 50,000 regional grid points, and a configurable indexed hypothesis budget (default 5 million, maximum 100 million). Split regions crossing the date line. Latitudes beyond ±85° are unsupported. Large archives need substantial RAM/time; type/region/time filtering is applied during ingestion.
- Cancellation is checked during decoding, spectrum generation, indexing, search, refinement and downloads. Archive body reads are cancellable within the network poll interval; small metadata/catalog requests have a 30-second timeout.

## Build and CLI

Rust stable (tested with 1.95), platform native build tools, and FFmpeg are required. The UI uses egui/eframe with OpenGL. Windows needs MSVC C++ build tools; Linux needs X11/Wayland/OpenGL development libraries and a file dialog portal (see CI).

```powershell
cargo build --release --locked
cargo run --release
cargo run --release -- demo --output demo
cargo run --release -- analyze --config demo/job.json --output reports/demo.json
cargo run --release -- view reports/demo.json
cargo run --release -- download --date 2026-09-08 --output cache
cargo run --release -- catalog --output cache/aircraft-types.json
```

Configuration paths are relative to the JSON file. `start_utc: null` means unknown time; `bounds: null` uses the retained flight-data extent. `demo/job.json` is a complete example. Analysis writes the requested report and, when enabled, caches downloaded references; decoded audio is temporary. Closing the GUI requests cancellation.

### Automation

The package includes **`soundgeo-cli.exe`**, a dedicated console executable for scripts, terminals and schedulers. Every processing command runs without a GUI or interactive prompts. With no arguments it prints help; only `view` opens a window. The desktop executable also accepts the same commands. Use `soundgeo-cli --help` or `soundgeo-cli analyze --help` for options. Build both executables with `cargo build --release --locked`, or run the console entry point with `cargo run --release --bin soundgeo-cli -- ...`.

```powershell
# Direct inputs, with unknown recording time and automatic search by default
soundgeo-cli.exe analyze --input "recording.mp4" --tracks "flight-data" --output "report.json"

# Download missing daily references automatically; no recording clock supplied
soundgeo-cli.exe analyze --config "region.json" --input "recording.mp4" --date 2026-11-03 --cache "flight-cache" --output "report.json"

# Search an inclusive date range with --date 2026-11-01 --date-to 2026-11-03
# Saved settings, replacing the recording per batch item; JSON only on stdout
soundgeo-cli.exe analyze --config "job.json" --input "next.mp4" --output - --quiet --require-match
```

`--input` and `--tracks` override the configuration and resolve relative to the caller's working directory. Other configuration paths resolve relative to the JSON file. Without `--config`, `--input` and either `--tracks` or `--date` are required; other settings use their defaults. `--date` enables automatic references and overrides a configured archive request. `--tracks` disables a configured automatic request unless `--date` is also given. `--cache` resolves from the caller's working directory and defaults to `flight-cache`. Use a job JSON to control terrain, search bounds/budgets, aircraft filtering and silence constraints.

`--output -` emits the complete report as JSON to stdout; progress goes to stderr. `--quiet` suppresses progress/success text, but preserves JSON and errors. Report fields include `status`, `candidates`, `events`, `warnings`, the effective `job`, exact `reference_sources`, and `version`. Each candidate includes coordinates, inferred UTC start and per-event evidence. Exit codes: **0** completed (including no candidates); **1** input, processing or I/O failure; **2** invalid CLI arguments; **3** completed without candidates when `--require-match` is set. Code 3 still writes the report. Candidate presence is not a confidence guarantee.

On Windows, use `soundgeo-cli.exe` for automation so the shell waits normally and exposes `$LASTEXITCODE`. For example: `$report = & .\soundgeo-cli.exe analyze --config .\Demo\job.json --output - --quiet | ConvertFrom-Json`. Python can use `subprocess.run([...], capture_output=True, text=True)` and parse stdout with `json.loads`.

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --locked
```

The integration test independently synthesizes two flyovers from emission-time phase, encodes an MP4 with AAC, analyzes that video with an unknown clock, and checks location/start-time recovery. A country-extent fixture adds 64 distant/differently timed flights and verifies indexed recovery without coarsening. Other tests cover aircraft catalogs/filtering, quiet-gap conflicts and strict rejection, propagation/Doppler signs, missing gaps, invalid inputs, trace units, split archives, terrain, budgets, stalled-network cancellation, checksums/corrupt caches, truncated downloads, and noise/hum/silence rejection. CI runs formatting, strict Clippy, every test and the release build on both Windows and Linux.

Automatic references can also be configured in JSON: `"archive": {"date_from":"2026-11-01", "date_to":"2026-11-03", "cache":"flight-cache"}`. Cache paths in JSON resolve relative to that file. Set `"start_utc": null` to withhold the recording clock. Short-pass detection is enabled by default; `"fast_flyovers": false` disables it.

The short-jet regression independently synthesizes fast fan tones under strong bass and stationary hum. The consensus regression retains the true unknown-clock location with an unrelated detection, then rejects it when one of the three supporting flights has inconsistent timing.

For recordings with at least three matched flyovers spanning ten minutes, the
solver can fit one shared clock rate, bounded by `max_clock_drift_ppm` (default
5000, maximum 5000). Set it to zero to use fixed-rate media time. Each candidate
reports `recording_clock_rate`: UTC elapsed seconds equal media elapsed seconds
multiplied by this rate. This handles uniform clock drift; it does not grant
individual flyovers independent timing offsets or repair discontinuous edits.
