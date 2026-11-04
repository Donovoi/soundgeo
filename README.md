# SoundGeo

A native Rust desktop workbench that takes video or audio, extracts aircraft-like Doppler sweeps, and ranks recording locations against historical flight tracks. Unknown recording time is supported: the search fits **one shared UTC start time** across the detected events.

**Research software.** It returns candidate coordinates and evidence, not a guaranteed location. A video alone cannot identify its date or location without suitable reference flight records. This is an independent implementation inspired by [colsto's demonstration](https://www.youtube.com/watch?v=UytYrhYLOKI); the creator did not release that pipeline's code.

## Desktop quick start

1. Run `soundgeo.exe` on Windows, or `cargo run --release` from source.
2. Choose or drop a video/audio file. FFmpeg must be on PATH, beside the executable, or selected in Advanced settings.
3. Choose a flight CSV, a readsb trace, a split archive's `.tar.aa` file, or a folder containing these files. The app can download an ADSB.lol archive for a possible recording date. Downloads are explicit and can be several GB per day.
4. Leave Recording time empty if unknown. Supply a UTC time and tolerance when available. If the date is unknown too, import a folder containing the dates you want searched.
5. Search area is optional: large extents automatically use indexed flight corridors. A smaller area makes searches faster. Optional SRTM terrain tiles improve the elevation model; otherwise enter an approximate ground elevation.
6. Click **Analyze recording**. Inspect candidate locations, inferred time, observed/predicted curves, and limitations. Export a JSON report.

**Try a synthetic demo** creates a noisy two-flyover recording and flight CSV locally. Click Analyze recording after it loads. It demonstrates the pipeline without flight-data downloads. Synthetic success does not establish real-world accuracy.

No AI API key is needed. Recordings are never uploaded. Only the explicit archive download and map/source links use the network. Opening a map sends the selected coordinates to OpenStreetMap.

## Flight data

CSV uses these exact headers. Time is Unix seconds or RFC3339 with a timezone; coordinates are WGS84 degrees and altitude is **metres**. Each aircraft needs at least eight points with gaps no larger than 60 seconds.

```csv
aircraft,time,lat,lon,altitude_m
example,2026-09-08T10:00:00Z,35.0000,-110.0200,1200
example,2026-09-08T10:00:02Z,35.0000,-110.0184,1200
```

Optional CSV columns `type_code` (e.g. `C172`) and `type_description` (ICAO engine description, e.g. `L1P`) enable aircraft-type filtering. By default, known jets/rotorcraft are excluded and unknown types are retained with a warning. Choose **Known propellers only** to exclude unknowns. ICAO `L/S/A` aircraft with piston (`P`) or turboprop (`T`) engines are propellers; helicopters/gyrocopters are classified separately. Type descriptions can be resolved using the small [Mictronics catalog](https://github.com/Mictronics/readsb-protobuf/blob/dev/webapp/src/db/types.json), downloadable in the app or with `soundgeo catalog`. Archive download obtains this catalog automatically; a neighboring `aircraft-types.json` is detected and recorded in the report configuration.

Prefer geometric altitude with a consistent vertical datum. Correct pressure altitude before importing a custom CSV. Keep independent flights under separate aircraft IDs if practical. Input order is normalized, duplicate timestamps are removed, and large gaps or implausible speeds split tracks.

Readsb JSON/gzip and ADSB.lol split TAR archives are read directly. Archives are streamed, **not extracted**. Absolute base timestamps are combined with per-point offsets. Geometric altitude is preferred; pressure-altitude fallback is reported. Ground/stale/missing-position records are skipped. Readsb data is reduced to a minimum two-second interval. A selected search area/time filters records during ingestion while keeping an acoustic buffer around the region.

The downloader tries `prod-0`, then `prod-1`, then `staging-0` if a release is absent. Published SHA-256 digests are verified when supplied; every completed part also gets a local SHA-256 cache stamp. Corrupt cached parts are not reused. Interrupted/invalid partial parts are discarded, and cancellation interrupts stalled body reads. The release manifest and attribution are saved beside the archive. This does not prove coverage completeness or combine replicas automatically. Directory import merges multiple days. Near midnight, include adjacent days to retain crossing flights.

References: [ADSB.lol historical archive](https://www.adsb.lol/docs/open-data/historical), [readsb trace schema](https://github.com/wiedehopf/readsb/blob/dev/README-json.md). The archive is licensed under [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/); a licence notice is saved with downloads. Respect attribution and applicable share-alike terms when redistributing derived databases.

## Method

- FFmpeg decodes the first audio stream to 4 kHz mono. Input files are unchanged.
- A Hann-window STFT (8192 samples, 0.5-second hop) exposes narrow tones. Viterbi tracing searches overlapping 90-second windows. Sustained signal contrast, sweep shape and a propagation-aware flyover fit reject weak/noisy traces and stationary tones.
- Overlapping ridges/harmonics count as one event. Up to eight independent events are retained.
- Regional searches use a spatial grid. Large-area searches index flight positions in time/ECEF buckets and generate observer hypotheses on both sides of aircraft corridors at a fixed spatial spacing, across several slant-distance estimates. Country extent does not force a coarser grid. Candidate flights for later events are retrieved from this index using both arrival-time and distance constraints. Unknown time comes from received closest approach, not the file's creation timestamp.
- Candidate basins are refined using interpolated WGS84 ECEF positions. The model solves `t_receive = t_emit + range(t_emit) / c`, computes radial velocity at emission, then fits one source tone per event and a common recording start. It never extrapolates across gaps.
- Quiet gaps between detected events provide a silence constraint with ten-second edge guards. A known propeller predicted to pass within 80% of the closest observed flyover's slant distance is recorded as a conflict. Default mode adds a ranking penalty; strict mode rejects conflicting candidates; off mode disables this evidence. Unknown types do not create silence conflicts. **Audibility is an assumption**, so strict mode can reject a true location.
- Reports retain input paths/configuration, observed ridges, matched segment IDs, modeled frequencies, errors, warnings and inferred time. Error percentages are residual fit metrics, not probabilities.

## Limits and scope

- Synthetic two-aircraft recovery and negative controls are tested. **No real-world location accuracy has been established.** Displayed decimal places and refinement spacing are not measured uncertainty radii.
- Sparse/noisy tracks, jets without stable tones, wind, engine RPM changes, moving microphones, spliced audio, and changing weather can defeat the model. Human review is required.
- One flyover with unknown time may admit many locations/times; even multiple flyovers can be ambiguous. The tool may return no candidate.
- Both search modes are bounded numerical searches. The corridor index covers the imported date/region extent but retains a finite set of candidate basins and samples several slant distances, so it is not a proof that every possible location has been excluded. It uses local time-window closest approaches for repeated passes. A hypothesis-budget overrun returns an error and **no partial location result**. Narrow the archive, raise the budget, or change spacing before rerunning. Regional-grid budget coarsening is explicitly reported.
- Optional SRTM `.hgt` tiles are 1201×1201 or 3601×3601, big-endian signed metres, named like `N35W110.hgt`. Nearest-neighbour terrain sampling is used. Missing/void tiles use the configured elevation and produce a warning. Geoid/pressure corrections and atmospheric ray tracing are not implemented. GNSS and SRTM vertical datums can differ by tens of metres.
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

Configuration paths are relative to the JSON file. `start_utc: null` means unknown time; `bounds: null` uses the retained flight-data extent. `demo/job.json` is a complete example. Analysis writes only the requested report; decoded audio is temporary. Closing the GUI requests cancellation.

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --locked
```

The integration test independently synthesizes two flyovers from emission-time phase, encodes an MP4 with AAC, analyzes that video with an unknown clock, and checks location/start-time recovery. A country-extent fixture adds 64 distant/differently timed flights and verifies indexed recovery without coarsening. Other tests cover aircraft catalogs/filtering, quiet-gap conflicts and strict rejection, propagation/Doppler signs, missing gaps, invalid inputs, trace units, split archives, terrain, budgets, stalled-network cancellation, checksums/corrupt caches, truncated downloads, and noise/hum/silence rejection. CI runs formatting, strict Clippy, every test and the release build on both Windows and Linux.
