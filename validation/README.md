# Real-video validation

This is a failing acceptance benchmark, not a claim of reliable geolocation.
Nine recordings from three public cameras were captured locally on 2026-11-04.
The known UTC reference day and an approximately 40 km regional search box are
provided; the recording clock, camera coordinates, aircraft identities, tone
bands and flyover intervals are not supplied to the application. Every solver
job has `start_utc: null`. This is not an unrestricted worldwide/date-unknown test.

## Sources and integrity

- [StreamTime LIVE, Chicago Midway](https://www.youtube.com/watch?v=67BCsiW-1Io):
  three clips, 30, 20 and 30 minutes. The owner publishes camera coordinates
  41.77844250226028, -87.74277810405209. The case ID `midway-arrivals` is retained
  for reproducibility; the clip includes departing aircraft.
- [Southampton ferry camera](https://www.youtube.com/watch?v=QO-hO_kwwmY):
  one 60-minute clip. Its published host is La Regata opposite the ferry terminal;
  50.89677, -1.40477 is an approximate building centroid (about 15 m uncertainty).
- Each input is a continuous sequence of five-second DVR segments, with audio
  and video remuxed locally and metadata removed. SHA-256 hashes, durations,
  public sources and ground truth are in `cases.json`. Media is retained locally
  for the user and is not redistributed in this repository.
- Stream timestamps are not independently accurate recording clocks: one
  Midway visible clock differed from the manifest-derived timestamp by about
  50 seconds. Exact inferred-clock accuracy therefore is not an acceptance claim.
- `midway-holdout` was first evaluated after the short-pass detector was developed
  on the other recordings. It is a single same-site holdout, not independent
  validation across additional locations. Later development runs are not new holdouts.

## Reference data

The app downloaded the complete 2026-11-03 ADSB.lol `prod-0` archive
(3,531,563,008 bytes) and the alternate `staging-0` archive
(3,491,936,768 bytes). Both split archives were integrity checked.
The alternate replica had the same first-position times and point counts for
five inspected Midway flights, so it did not repair those coverage gaps.
These archives are [ODbL data](https://www.adsb.lol/docs/open-data/historical).

Repeated development runs used CSV exports of **every** full-day regional flight
retained by the importer: 4,947 segments/703,295 points for Midway and 4,336
segments/529,157 points for Southampton. They were not restricted to the true
recording clock or handpicked aircraft. `examples/export_reference.rs` refuses
a known-clock job. An integrated run also exercised `analyze --date --cache`
against the original split archive, with the recording clock still absent.

A known-position diagnostic was used to inspect the failure, never counted as
blind geolocation. At a plausible shared clock, the original five short detected
windows had only about 7–68% modeled flight coverage at the camera. Several
traces began after the audible pass had already started. The model deliberately
does not invent positions beyond the recorded track. A separate historical
provider's direct HTTP endpoint returned 403. Its documented free S3 sample
interface subsequently worked for November 1; see below.

## Changes prompted by failures

The original detector missed short fan tones. The revised detector adds a short
FFT and overlapping 10/20-second windows, local spectral whitening, and includes
jets in the default aircraft filter. It recovers five short sweeps in the
20-minute Midway clip. Detection alone is not geolocation.

The matcher now considers every detected event as a possible clock anchor and
uses a minimum of three independent matches when there are three or more
detections. Extra unmatched detections are explicitly identified; partial
consensus remains ambiguous. Recordings with fewer detections require all of
them. The 90% track-coverage, joint residual and per-event residual requirements
remain enforced. Unmatched sounds and incomplete flight positions are not
counted as corroboration.

Automatic archive retrieval is available in both the GUI and CLI. It verifies
cached parts, fetches missing/corrupt parts, supports an inclusive date range,
works offline with a complete pinned cache, and fails if any requested day is
unavailable. It does not imply that all aircraft were received by that network.

## Reproduction and acceptance

`run.py` requires Python 3.11+ and the original local captures. It verifies media
hashes, constructs solver jobs without clock/ground-truth fields, downloads or
reuses full-day flight data, and evaluates coordinates only after the solver has
finished. It exits 1 if any case lacks an unambiguous top result within 250 m.
This threshold defines the evaluation; it is not an estimated accuracy radius.

```powershell
python validation/run.py --cli target/release/soundgeo-cli.exe `
  --media-root ../real-validation --cache ../real-validation/flight-cache `
  --sample-cache ../real-validation/free-exchange-2026-11-01 `
  --type-catalog ../real-validation/flight-cache/2026-11-03/aircraft-types.json `
  --output ../real-validation/reproduction
```

Use `--case midway-arrivals` for one case. Downloaded data can be several GB per
day. `results.json` records actual development runs, including failures. No real
media/network benchmark runs automatically in CI; normal CI runs the deterministic
software regressions. Passing those regressions does not pass this acceptance test.

## November 3 results

All four final consensus runs returned `insufficient_evidence`, with no candidate. Acceptance: **0/4**. No real-location error can be reported when no location was returned.

| Case | Minutes | Detected events | Flight segments | Hypotheses | Result |
|---|---:|---:|---:|---:|---|
| midway | 30 | 5 | 4947 | 1780948 | insufficient_evidence |
| southampton | 60 | 3 | 4336 | 794649 | insufficient_evidence |
| midway-arrivals | 20 | 5 | 4947 | 2281111 | insufficient_evidence |
| midway-holdout | 30 | 4 | 4947 | 1344934 | insufficient_evidence |

Software checks: all 25 deterministic tests, strict Clippy, formatting and release compilation pass locally on Windows. Desktop checks covered automatic-download/date-range controls, manual import, partial-match counts, and correct event-to-curve association in a clearly labelled synthetic UI fixture. These checks do not establish real-video accuracy.

## Free November 1 reference data

The additional three recordings are two one-hour Midway clips and one one-hour
Southampton clip. The user requires free sources only. ADS-B Exchange publishes
a [free first-of-month sample](https://www.adsbexchange.com/data-products/sample-data/)
and [public demo S3 access](https://www.adsbexchange.com/pull-data/). The documented
sample interface worked without a private account. No paid data was used.

All 288 worldwide five-minute snapshots for November 1 were downloaded. Aircraft
were discovered throughout the entire UTC day in broad regions (Midway:
41.00–42.66, -88.9–-86.6; Southampton: 50.15–51.81, -2.6–-0.08), followed by their
full-day traces. An audit preserving non-ICAO `~` prefixes found 4,483 identifiers:
4,466 trace files were available and 17 returned `NoSuchKey`. There were no extra
or substituted identities. Flights entirely between discovery snapshots may
still be absent. Reference coverage is therefore not claimed complete.

The complete cache is a root folder containing `traces/` and `supplement/`; both
must be supplied, not just the first directory. Pass this root as `--sample-cache`
to `run.py` when reproducing November 1 cases. The application can import these
readsb files; its integrated automatic downloader currently retrieves ADSB.lol.
The supplemental sample download was a separate validation operation. Provider
data and camera media are retained locally and are not redistributed here.

The prior eight-event matcher found no candidates in all three full-reference
runs. The name `midway-nov1-holdout` denotes a later same-site capture; repeated
development evaluation does not make it a fresh independent holdout.

## Diagnostics and subsequent fixes

Known-position checks are explicitly excluded from acceptance. On the first
Midway November 1 recording, the source clock advances 3,600 seconds between the
first and last sampled segment starts while audio/video PTS advances about
3,593.2 seconds. Audio decoding with timestamp compensation retained the same
duration. Early and late independently fitted passes prefer clocks about seven
seconds apart. The matcher still requires a single clock and does not silently
shift individual events to force a result.

The ground-height input also needed a datum conversion: PROJ independently
calculates about -33.808 m of EGM96 geoid undulation at this Midway camera. The
app now applies the global bundled grid at each hypothesis when EGM96 is selected,
and always converts SRTM heights. A centered ten-second local linear fit reduces
flight-position quantization before estimating velocity; gaps are preserved.

The detector now retains up to 24 events, with eight strongest search anchors.
Coarse selection uses the same minimum consensus as final fitting. The bounded
refinement pool was expanded from 48 to 512, and an analytic propagation solution
replaces iterative bisection; an independent bisection regression verifies it.

With the height/noise fixes, a **diagnostic** search restricted to a known
2.2 × 2.5 km region returned an ambiguous candidate 88.2 m from the Midway camera,
matching three of 17 detections. The recording clock was absent, but the small
region was chosen using ground truth. This is not a successful blind benchmark
and is not included in the acceptance count. Only the original broad-region
jobs qualify in `results.json`.


## Full-region rerun after propagation and search fixes

The complete seven-case rerun used the original broad bounds, no recording
clock, corrected height datums, smoothed references and the 4 kHz detector.
The earlier narrow-region diagnostic is still excluded. The first Midway
November 1 clip returned the same candidate in its original broad region:
**88.184 m from the camera**, three matches among 17 detections, 0.930% relative
fit error, inferred start 2026-11-01T17:57:37.345Z. It is explicitly **ambiguous**.

| Case | Events | Flight segments | Hypotheses | Result |
|---|---:|---:|---:|---|
| midway | 5 | 4947 | 1329717 | insufficient_evidence |
| southampton | 3 | 4336 | 814840 | insufficient_evidence |
| midway-arrivals | 5 | 4947 | 1792967 | insufficient_evidence |
| midway-holdout | 4 | 4947 | 963091 | insufficient_evidence |
| midway-nov1 | 17 | 5381 | 7214462 | ambiguous; 88.184 m |
| midway-nov1-holdout | 10 | 5381 | 4088117 | insufficient_evidence |
| southampton-nov1 | 8 | 4134 | 162775 | insufficient_evidence |

Coordinate hits within 250 m: **1/7**. Accepted unambiguous locations: **0/7**.
The matched coordinates alone do not override the ambiguity flag. All case
hashes were verified, and decoded durations replace nominal capture durations
in the manifest. Previously named holdouts are now development retests because
they have been inspected repeatedly.

The parallel corridor implementation produced the same first-Midway candidate
as the serial implementations. Windows and Ubuntu passed all 30 software tests,
formatting, strict Clippy and release compilation at commit
`3707cee64e28e0ffcbf90aa69685842d81ca4791`. This is software validation, not
successful real-video acceptance.

## Wider-band development

The decoder now preserves frequencies up to 4 kHz. A simple widened search
initially introduced false extra detections and failed a synthetic CLI test.
Independent low/high tracing bands and harmonic consistency for the high band
fixed that regression. A separate score regression demonstrated that an
individually plausible fourth fit could invalidate a passing three-flight core;
additional matches now must also preserve the joint error limit. All original
coverage and error thresholds remain unchanged. Real-video reruns of these
changes are tracked separately from the preceding 4 kHz results.


## Completed 8 kHz run, including Lake Hood

All eight cases used their original broad region and `start_utc: null`.
The first Midway November 1 result improved to **74.706 m**, four independently
matched flights out of 19 detections, and 0.730% joint fit error. Its second
candidate was 385.129 m away. Both remained ambiguous. Lake Hood was added from
[the public camera](https://www.youtube.com/watch?v=61pi8UjLOlo) at the Alaska
Aviation Museum, using [the owner's map pin](https://goo.gl/maps/cKpcc57ES22d2cwn6)
(61.179633, -149.9719665, not a survey). A full hour from November 3 was evaluated;
the complete daily ADSB.lol archive was verified and loaded automatically.

Lake Hood's top candidate was **wrong by 3133.306 m**, with five other wrong
candidates farther away. All six were marked ambiguous. This failure is retained
and must not be described as location recovery. A St. Barts screening clip was
digitally silent and was excluded as unsuitable for audio geolocation.

| Case | Events | Flight segments | Hypotheses | Result | Top error (m) |
|---|---:|---:|---:|---|---:|
| midway | 4 | 4947 | 1209623 | insufficient_evidence | - |
| southampton | 3 | 4336 | 702289 | insufficient_evidence | - |
| midway-arrivals | 6 | 4947 | 1925357 | insufficient_evidence | - |
| midway-holdout | 1 | 4947 | 488250 | insufficient_evidence | - |
| midway-nov1 | 19 | 5381 | 6773575 | ambiguous | 74.706 |
| midway-nov1-holdout | 11 | 5381 | 5040888 | insufficient_evidence | - |
| southampton-nov1 | 11 | 4134 | 162775 | insufficient_evidence | - |
| lake-hood | 24 | 1430 | 4825799 | ambiguous | 3133.306 |

Coordinate hits within 250 m: **1/8**. Accepted unambiguous locations: **0/8**.
The exact executable and report hashes are in `results.json`. Windows and Ubuntu
passed all 32 software tests, formatting, strict Clippy and release builds at
`8916066524c00611b44adfe5ef28b3febe24d4d7`. This later commit also fixes CSV type
metadata appearing after an aircraft's initial positions; the real-run executable
is recorded separately as commit `8dbe92fe76fa7187abb2e97adefdfe41cc8401fb`.

## Curvature qualification fix under evaluation

Some wrong Lake Hood matches explained the observed frequency change worse than
a freely fitted straight line. Two regressions reproduced this: a linear chirp
was accepted as a detected flyover, and an unrelated flight could corroborate a
linear frequency drift merely through small relative tone error. Detected curves
and final flight associations now must halve the straight-line squared residual
on exactly the same observations. Optimization remains finite outside this
acceptance boundary so nearby valid solutions can still be refined. The baseline
RMSE is retained in each new JSON match; older reports remain readable.

All 34 software tests, formatting, strict Clippy and release compilation passed
on Windows and Ubuntu at `dc30a884e395c864511504bf5e28defd07dc00df`.
All nine curvature runs are complete: the first Midway November 1 result remains
74.706 m away, four matches, ambiguous; every other case returns no candidate.
Coordinate hits: **1/9**. Accepted locations: **0/9**. Rejecting incorrect candidates
does not satisfy real-location acceptance. No acceptance threshold was relaxed.


## Second Lake Hood reference day and media clock regression

A ninth case, `lake-hood-nov1`, retains a separate 60-minute public-camera capture
and a full-day free ADS-B Exchange Alaska sample. All 288 five-minute global
snapshots were inspected across a broad Alaska region, without a recording-clock
filter: 570 identifiers were discovered, 568 traces were available, and two
returned NoSuchKey. Non-ICAO prefixes are preserved. The original reference
cache was not changed. Use `--alaska-sample-cache` alongside `--sample-cache`
when reproducing all cases with `validation/run.py`; `--type-catalog` supplies
the shared aircraft type catalog. This ninth case returned no candidate under
the curvature-qualified matcher. The November 3 Lake Hood rerun also returned
no candidate, removing the earlier incorrect 3.1 km result without recovering
the camera.

A diagnostic at the known Midway camera found that three later flyovers fit
one shared clock-rate correction of about +2078 ppm with 0.788% joint error.
That diagnostic used the known location and clock, so it is not an acceptance
result. A regression based on independently synthesized emission-phase audio
reproduced failure of the fixed-rate solver over an 86-minute recording. The
bounded affine clock now recovers it, while rejecting an inconsistent flight
time and preserving fixed-rate operation when disabled. Real runs of this
change are recorded separately; software regression success is not accuracy
validation.

The affine-clock change passed all 36 software tests, formatting, strict Clippy
and release compilation on Windows and Ubuntu at
`15edcdfb74550820af1e0d714b9cd9e72e618077`. Its broader Midway holdout run still
returned no candidate, despite a separate known-area diagnostic recovering a
nearby solution. That diagnostic is excluded from acceptance. The arrivals clip
also produced a wrong ambiguous candidate about 27 km away, retained as a
failure. The remaining runs are being completed.

A coarse-search regression reproduced how thousands of repeated proposals could
evict every alternative before detailed fitting. Intermediate beams now retain
distinct position/time/flight groups; surviving proposals are ranked by full
curve error before the refinement cap. All 37 local software tests, formatting,
strict Clippy and release compilation pass. Another nine-case real rerun is in
progress; this change has not yet established real-world accuracy.

Additional free-source checks found that [ADSBiq's published archive](https://github.com/Sky-Power-Services/adsbiq-data) ends on
2026-10-10, before these cases. Direct historical requests to ADSB.fi and
Airplanes.live did not yield usable public traces (403); TheAirTraffic returned
non-JSON content. These probes do not establish that those services lack data,
and no access restrictions were bypassed.


## Completed affine-clock run and earlier scoring

All nine affine-clock cases are complete for
`15edcdfb74550820af1e0d714b9cd9e72e618077`. The first Midway November 1 top
candidate is 82.526 m away (four matches, ambiguous). Midway arrivals returns
one incorrect ambiguous candidate 27,470.586 m away. The other seven cases
return insufficient evidence. Coordinate hits: **1/9**; accepted locations:
**0/9**. These failures remain in `results.json`.

The diversity-only revision still missed the later Midway recording in the
original broad region. A second regression exposed that full-curve scoring
happened after earlier beam truncations. It now runs before both worker and
global beam pruning. All 38 local software tests, formatting, strict Clippy
and release compilation pass; the affected broad-region case is running before
another complete real-video evaluation.

A separate Lake Hood diagnostic used the [free IEM PANC weather archive](https://mesonet.agron.iastate.edu/request/download.phtml) for the
entire November 1 UTC day. A lower sound speed and an approximate QNH correction
for pressure-altitude fallbacks improved one clear event but did not establish
three jointly consistent flyovers. It used the known location/time for diagnosis,
is excluded from acceptance, and has not changed the production weather model.
