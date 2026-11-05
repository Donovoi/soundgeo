# Technical reference

This page gives the detailed data and model rules. See the [README](../README.md) for operating instructions.

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
- A Hann-window STFT (16384 samples, 0.5-second hop) exposes low-frequency tones. A second scale (1024 samples, 0.025-second hop, 500–3900 Hz) handles fast jet sweeps. The original 500–1900 Hz band and the 1400–3900 Hz extension have separate tracing budgets. Extended-band detections additionally need a corresponding harmonic across the moving frequency pattern. Local spectral-median subtraction reduces interference from bass/wind. Viterbi tracing searches overlapping 40/90-second windows for slow curves and 10/20-second windows for short passes. Sustained signal contrast, sweep shape and a propagation-aware flyover fit reject weak/noisy traces and stationary tones. A detected curve and each reported flight match must reduce squared error by at least half compared with a freely fitted straight frequency drift on the same samples; a descending line alone is insufficient timing evidence.
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
- Optional SRTM `.hgt` tiles are 1201×1201 or 3601×3601, big-endian signed metres, named like `N35W110.hgt`. Nearest-neighbour terrain sampling is used. SRTM heights are converted from EGM96 to WGS84 ellipsoid heights using the bundled public-domain NGA geoid grid ([source and license](../assets/NOTICE.txt)). Missing/void tiles use the configured elevation and produce a warning. `ground_datum: "egm96"` converts a manual sea-level height; `"wgs84_ellipsoid"` preserves the original manual-height behavior and is the default for existing JSON jobs. New GUI jobs select EGM96. Candidate `ground_m` values are always WGS84 ellipsoid heights. Pressure-altitude corrections and atmospheric ray tracing are not implemented; EGM96 is not an exact conversion for every national height datum.
- Limits: recordings 20 seconds–2 hours, 10 million retained track points, 32 MB decoded per trace, at most 50,000 regional grid points, and a configurable indexed hypothesis budget (default 5 million, maximum 100 million). Split regions crossing the date line. Latitudes beyond ±85° are unsupported. Large archives need substantial RAM/time; type/region/time filtering is applied during ingestion.
- Cancellation is checked during decoding, spectrum generation, indexing, search, refinement and downloads. Archive body reads are cancellable within the network poll interval; small metadata/catalog requests have a 30-second timeout.


## Advanced settings

Automatic references can also be configured in JSON: `"archive": {"date_from":"2026-11-01", "date_to":"2026-11-03", "cache":"flight-cache"}`. Cache paths in JSON resolve relative to that file. Set `"start_utc": null` to withhold the recording clock. Short-pass detection is enabled by default; `"fast_flyovers": false` disables it.

The short-jet regression independently synthesizes fast fan tones under strong bass and stationary hum. The consensus regression retains the true unknown-clock location with an unrelated detection, then rejects it when one of the three supporting flights has inconsistent timing.

For recordings with at least three matched flyovers spanning ten minutes, the
solver can fit one shared clock rate, bounded by `max_clock_drift_ppm` (default
5000, maximum 5000). Set it to zero to use fixed-rate media time. Each candidate
reports `recording_clock_rate`: UTC elapsed seconds equal media elapsed seconds
multiplied by this rate. This handles uniform clock drift; it does not grant
individual flyovers independent timing offsets or repair discontinuous edits.

## Candidate selection

Both search modes compare sampled sound curves before they remove proposals.
A proposal keeps up to three plausible flights for each detected sound.
The model compares these alternatives with one shared recording clock.
A flight segment cannot support two detections in the same proposal.
Selection finds the least-error independent set required for the coarse score, using up to three matches.
This resolves conflicts where one early choice would block another event's only available flight.
Other independent matches can provide additional support during refinement.
Final refinement and acceptance use every retained sample in each sound curve.
These bounded choices do not make the search exhaustive.
Corridor seeds sample the distance-ratio range allowed by the existing feature-error limit.
Flight queries use the same range bound. The maximum reference distance remains 40 km.
The straight-flight audio estimate can be inaccurate for partial curves or turning aircraft.
The wider proposal range can increase computation and consume the hypothesis budget sooner.

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
These checks reduce errors from short fragments. They do not prove that a detected tone came from an aircraft.

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
It does not compare their fitted closest-approach times, which can be biased on a short fragment.
The app compares these alternatives during flight lookup and curve matching.
It counts at most one match per group. It does not trim curves to fit the available flight data.
This conservative grouping can combine two real aircraft that overlap in time.

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
Signal contrast is a detector setting, not a probability that the sound came from an aircraft.
