# Technical reference

## Repeated archive searches

The importer saves decoded regional tracks in the flight cache's `decoded-tracks`
folder. Each entry is tied to source file hashes, the aircraft catalog, the
search bounds, the aircraft filter, and the importer code. The source archives
remain required. Changed source bytes or settings trigger a new import.
Damaged derived entries are kept with a `rejected` suffix and rebuilt locally.
The cache keeps exact floating-point values and applies track smoothing after
loading. Read-only flight archives can be used without saving a derived cache.

The passage timing setting `timing_options.timing_sigma_s` sets the Gaussian
timing width in seconds. It defaults to `1.0` and accepts `0.5` through `10.0`.
It is applied to clock proposals, sound-to-flight assignments, likelihood
normalization, and shuffled controls. This setting supports model experiments.

This page gives the detailed data and model rules. See the [README](../README.md) for operating instructions and [limitations](limitations.md) for model constraints and validation coverage.

## Flight data

CSV uses these exact headers. Time is Unix seconds or RFC3339 with a timezone; coordinates are WGS84 degrees and altitude is **metres**. Each flight segment needs at least three recorded points for passage timing, or eight for Doppler matching. Gaps must be no larger than 60 seconds. The importer uses recorded points.

```csv
aircraft,time,lat,lon,altitude_m
example,2026-09-08T10:00:00Z,35.0000,-110.0200,1200
example,2026-09-08T10:00:02Z,35.0000,-110.0184,1200
```

Optional CSV columns `type_code` (e.g. `C172`) and `type_description` (ICAO engine description, e.g. `L1P`) enable aircraft-type filtering. All aircraft types are included by default because both propeller and short jet tones are analyzed. Choose **Known propellers only** to exclude unknowns. ICAO `L/S/A` aircraft with piston (`P`) or turboprop (`T`) engines are propellers; helicopters/gyrocopters are classified separately. Type descriptions can be resolved using the small [Mictronics catalog](https://github.com/Mictronics/readsb-protobuf/blob/dev/webapp/src/db/types.json), downloadable in the app or with `soundgeo catalog`. Archive download obtains this catalog automatically; a neighboring `aircraft-types.json` is detected and recorded in the report configuration.

Prefer geometric altitude with a consistent vertical datum. Correct pressure altitude before importing a custom CSV. Keep independent flights under separate aircraft IDs if practical. Input order is normalized, duplicate timestamps are removed, and large gaps or implausible speeds split tracks.

Readsb JSON/gzip and ADSB.lol split TAR archives are read directly. Archives are read directly as streams. Absolute base timestamps are combined with per-point offsets. Geometric altitude is preferred; pressure-altitude fallback is reported. Ground/stale/missing-position records are skipped. Readsb data is reduced to a minimum two-second interval. A selected search area/time filters records during ingestion while keeping an acoustic buffer around the region.

The downloader tries `prod-0`, then `prod-1`, then `staging-0` if a release is absent. Published SHA-256 digests are verified when supplied; every completed part also gets a local SHA-256 cache stamp. Corrupt cached parts are not reused. Interrupted/invalid partial parts are discarded, and cancellation interrupts stalled body reads. The release manifest and attribution are saved beside the archive. A pinned manifest allows fully cached analysis offline. Date-range analysis loads only the requested dates, reports the exact reference paths, and fails if any requested day is unavailable; unrelated cached dates are not included. Directory import merges multiple days. Near midnight, include adjacent days to retain crossing flights.

References: [ADSB.lol historical archive](https://www.adsb.lol/docs/open-data/historical), [readsb trace schema](https://github.com/wiedehopf/readsb/blob/dev/README-json.md). The archive is licensed under [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/); a licence notice is saved with downloads. Respect attribution and applicable share-alike terms when redistributing derived databases.

## Method

- FFmpeg decodes the first audio stream to 8 kHz mono. Input files are unchanged.
- A Hann-window STFT (16384 samples, 0.5-second hop) exposes low-frequency tones. A second scale (1024 samples, 0.025-second hop, 500Ã¢â‚¬â€œ3900 Hz) handles fast jet sweeps. The original 500Ã¢â‚¬â€œ1900 Hz band and the 1400Ã¢â‚¬â€œ3900 Hz extension have separate tracing budgets. Extended-band detections additionally need a corresponding harmonic across the moving frequency pattern. Local spectral-median subtraction reduces interference from bass/wind. Viterbi tracing searches overlapping 40/90-second windows for slow curves and 10/20-second windows for short passes. Sustained signal contrast, sweep shape and a propagation-aware flyover fit reject weak/noisy traces and stationary tones. A detected curve and each reported flight match must reduce squared error by at least half compared with a freely fitted straight frequency drift on the same samples; a descending line alone is insufficient timing evidence.
- Overlapping ridges/harmonics count as one event. Up to 24 independent events are retained; the strongest eight seed the bounded search, and all retained events can corroborate them.
- Regional searches use a spatial grid. Large-area searches index flight positions in time/ECEF buckets and generate observer hypotheses on both sides of aircraft corridors at a fixed spatial spacing, across several slant-distance estimates. Candidate flights for later events are retrieved from this index using both arrival-time and distance constraints. Corridor lookups use local time-window closest approaches for repeated passes. The recording time is fitted from the received closest approach.
- For three or more detections, at least three distinct flight segments must fit; smaller recordings require every event. Coarse ranking and exact refinement use the same minimum consensus. Additional associations must preserve the joint error threshold and retain a passing three-flight core. Unmatched detections remain explicit outliers. Partial consensus is reported as ambiguous. All events use one recording clock.
- Flight positions are smoothed with a centered 10-second local linear regression before velocity estimation (`smooth_tracks: false` disables this). Timestamps, segment endpoints and gaps are preserved. Up to 512 distinct candidate basins are refined using interpolated WGS84 ECEF positions. The model solves `t_receive = t_emit + range(t_emit) / c` analytically within each constant-velocity segment, computes radial velocity at emission, then fits one source tone per event and a common recording start. It never extrapolates across gaps.
- Quiet gaps between detected events provide a silence constraint with ten-second edge guards. A known propeller predicted to pass within 80% of the closest observed flyover's slant distance is recorded as a conflict. Default mode adds a ranking penalty; strict mode rejects conflicting candidates; off mode disables this evidence. Quiet-gap checks apply to known propellers.
- Reports retain input paths/configuration, observed ridges, matched segment IDs and original event indices, modeled frequencies, errors, the linear-drift baseline residual, warnings and inferred time. Error percentages measure residual fit.

## Terrain

Optional SRTM `.hgt` tiles are 1201Ãƒâ€”1201 or 3601Ãƒâ€”3601, big-endian signed metres, named like `N35W110.hgt`. Nearest-neighbour terrain sampling is used. SRTM heights are converted from EGM96 to WGS84 ellipsoid heights using the bundled public-domain NGA geoid grid ([source and license](../assets/NOTICE.txt)). Missing/void tiles use the configured elevation and produce a warning. `ground_datum: "egm96"` converts a manual sea-level height; `"wgs84_ellipsoid"` preserves the original manual-height behavior and is the default for existing JSON jobs. New GUI jobs select EGM96. Candidate `ground_m` values are always WGS84 ellipsoid heights.

## Cancellation

Cancellation is checked during decoding, spectrum generation, indexing, search, refinement and downloads. Archive body reads are cancellable within the network poll interval; small metadata/catalog requests have a 30-second timeout.

## Advanced settings

Automatic references can also be configured in JSON: `"archive": {"date_from":"2026-11-01", "date_to":"2026-11-03", "cache":"flight-cache"}`. Cache paths in JSON resolve relative to that file. Set `"start_utc": null` to withhold the recording clock. Short-pass detection is enabled by default; `"fast_flyovers": false` disables it.

The short-jet regression independently synthesizes fast fan tones under strong bass and stationary hum. The consensus regression retains the true unknown-clock location with an unrelated detection, then rejects it when one of the three supporting flights has inconsistent timing.

For recordings with at least three matched flyovers spanning ten minutes, the
solver can fit one shared clock rate, bounded by `max_clock_drift_ppm` (default
5000, maximum 5000). Set it to zero to use fixed-rate media time. Each candidate
reports `recording_clock_rate`: UTC elapsed seconds equal media elapsed seconds
multiplied by this rate. This handles uniform clock drift.

## Candidate selection

Both search modes compare sampled sound curves before they remove proposals.
A proposal keeps up to three plausible flights for each detected sound.
The model compares these alternatives with one shared recording clock.
Each flight segment supports at most one detection per proposal.
Selection finds the least-error independent set required for the coarse score, using up to three matches.
This resolves conflicts where one early choice would block another event's only available flight.
Other independent matches can provide additional support during refinement.
Final refinement and acceptance use every retained sample in each sound curve.
Corridor seeds sample the distance-ratio range allowed by the existing feature-error limit.
Flight queries use the same range bound. The maximum reference distance remains 40 km.

## Long sound curves

The first scan uses overlapping 40-second and 90-second windows for slow flyovers.
The shorter window limits interference from sounds before and after a brief propeller pass.
Each slow window subtracts the positive time median at each frequency before tracing.
This prevents persistent tones from consuming the tracing budget or attracting a crossing curve.
The short-jet scan is unchanged.
A detected curve of at least 30 seconds can guide a second trace through up to 180 seconds of audio.
The second trace uses the original spectrum. It must retain sufficient signal strength and fit quality.
When slow curves overlap, the primary trace favors evidence before and after the closest approach.
The closest approach must lie inside the supported sound interval during both initial fitting and refinement.
These checks reduce errors from short fragments.

Adjacent slow fragments count as one sound when their frequency is continuous across a gap of at most five seconds.
The frequency difference must be within 2 Hz or 1%, whichever is larger.
The report preserves the full interval in `activity_span_s`.
Quiet-gap checks exclude that full interval. Grouping occurs before the 24-event limit, so repeated fragments do not fill the list.
Short jet detections retain their separate timing rules.

Overlapping traces form one activity group, including traces connected through another overlapping trace.
Each group keeps a primary trace and up to three alternatives selected from the audio.
Longer supported traces can remain available when a short fragment has a better standalone fit.
A short trace is removed when a trace at least 25% longer covers 90% of it and contains the same measured curve.
This comparison allows integer harmonics and requires less than 0.5% relative curve error.
The app compares these alternatives during flight lookup and curve matching.
It counts at most one match per group and retains each measured curve in full.

Reports store the other traces in the event's `alternatives` array.
Each match's `event_variant` selects its observation: 0 means the primary trace, and 1 means the first alternative.
Older reports without these fields remain readable.
The desktop plot displays the selected observation and its matching prediction.

## Sound-curve fitting

The fitter solves the same propagation-aware Doppler model with a different parameter form.
It fits the center and width of the curve. At each trial, it solves the two frequency coefficients directly.
It then recovers the source frequency, speed, distance, and received closest-approach time.
The original speed, distance, and closest-approach bounds still apply.

This avoids a failure in separate speed and distance updates.
Those updates could stop at a fast, distant solution even when a nearby, slower solution fitted much better.
A regression uses independently generated propagation delays to check both parameter recovery and fit error.
The reference-coverage and final location-acceptance limits are unchanged.

Short-flyover detection requires 8 dB of signal contrast by default.
The former 6 dB default admitted occasional curves from generated wind and Gaussian noise.
Explicit contrast settings in saved jobs are retained.
Signal contrast controls detection.

The regional search indexes closest-approach times once per grid point.
Each observation then reads only the existing 45-second matching window from that index.
The index preserves the reference dates and matching gate.

## Chicago public replay

Set `archive.provider` to `webtrak_chicago`, or use `--provider webtrak-chicago`
with `--date`. The default provider remains `adsb_lol`.
Each job uses its selected provider.

Each UTC day contains 2,880 half-minute replay handles. The program checks the
handle and the Chicago local start and end times of each response. The UTC
handle resolves the repeated hour at the end of daylight saving time.
It checks each point against its owning interval. Downloaded parts have SHA-256
checksums. The final CSV and manifest are written only after all parts pass.
The downloader uses two workers, at most two requests per second, bounded
responses, and bounded retries. A later attempt can reuse valid parts.
Cancellation is checked while waiting for response headers and body chunks,
including a stalled connection. Small catalog requests retain a 30-second timeout.

Radar field 18 is altitude in metres. Field 10 is the client's height field.
The importer adds EGM96 geoid undulation
to the MSL altitude to produce WGS84 ellipsoid heights. Reports identify
intervals whose radar status is offline.

The public source provides about 90 days of replay. Older complete local caches
remain usable. The downloader verifies the requested interval and file integrity.
Source: [Chicago Department of Aviation flight tracker](https://www.flychicago.com/community/ordnoise/flighttracker/pages/default.aspx).

## Date batches and offline analysis

An archive request can contain any ordered date range supported by its source.
An empty `date_from` and absent `date_to` select dates found in the provider's
cache folders. Each selected cache is verified before analysis.

Multi-date analysis decodes the media once. It searches pairs of adjacent days,
with a one-day overlap between pairs. Isolated cached days form separate batches.
Doppler candidates are ranked together and duplicate fits are removed. Passage
timing runs the same 19 sound permutations for every batch, retains the maximum
score for each control across batches, and checks all competitive locations.
The final screen is applied after the batches are combined. Long imported
reference files also split timing-histogram calculations into daily windows.

Reports include `date_search.dates`, `completed_batches`, and `complete` for a
batched run. Track counts include repeated tracks in overlapping batches.
An error or cancellation stops the run before a completed report is returned.
Verified downloads remain cached for the next attempt.

Set `offline: true` in a job or pass `--offline` on the command line. The setting
blocks archive, catalog, and terrain requests. FFmpeg is restricted to local
file and pipe protocols. Cached files still undergo their integrity checks.
The free sample cache preserves recorded unavailable traces during offline reuse.
Online preparation retries those traces. A missing downloaded object fails offline verification.
The `download` command accepts `--date-to` and optional `--terrain-cache` with
bounds to prepare data before disconnecting.
