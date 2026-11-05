# Real-video validation

This is a failing acceptance benchmark, not a claim of reliable geolocation.
Ten recordings from three public cameras were captured locally on 2026-11-04.
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


## Fresh Lake Hood capture and coarse clock profiling

A tenth case, `lake-hood-nov1-later`, is a separate 60-minute November 1 capture
from the same public camera and free full-day Alaska reference cache. Its first
frame independently confirms the date. Live-playlist discontinuities were
avoided by deriving its DVR sequence from the earlier verified November 1
capture. The application still receives no recording clock. Under
`8559bd7c047a31f1d317d07a1a70729e1c142985`, it produced seven wrong ambiguous
candidates, the first **10,579.371 m** away. The later Midway case remained empty.
Both are failures. All 38 software tests and required checks passed on Windows
and Ubuntu for that revision.

The full nine-case diversity run at
`3b724b4d7ee406019a48f944fe48bd0a3278153a` retained the first Midway result at
82.526 m, but introduced additional wrong lower-ranked alternatives. All other
cases failed: arrivals' top candidate was 27,613.271 m away; seven cases returned
no candidate. `results.json` now explicitly records per-case latest revisions
and how many cases have been tested against the newest evaluated revision;
its combined latest-case totals do not imply a complete run of one executable.

A further regression reproduced an inconsistent model between stages: early
beam ranking assumed unit clock rate even when final refinement allowed drift.
Coarse scoring now profiles the same bounded shared rate, preserving the
anchor's received CPA. It uses 16 samples spread across each curve to bound
proposal-search cost; final ranking, refinement and all acceptance thresholds
still use every observed sample. The independent long-recording integration
regression remains in place. All 39 local software tests, formatting, strict
Clippy and release compilation pass. The completed ten-case evaluation is below.

## Additional free archive and reporting fixes

The automatic CLI downloader retrieved and integrity-checked the complete
November 1 ADSB.lol `prod-0` archive: **3,118,588,416 bytes**, two parts. Its
original Lake Hood regional export contains 1,046 segments / 178,704 points,
compared with 1,040 segments / 156,362 points from the free ADS-B Exchange sample.
Across that entire regional day, 456 ADSB.lol positions have no Exchange position
for the same aircraft within 30 seconds. This comparison does not establish
complete coverage or identify which aircraft produced the sounds.

Both Lake Hood November 1 clips were rerun against the alternate archive, still
with their original bounds and no recording clock. Both failed: the earlier
clip's top candidate was **10,782.065 m** away, and the later clip's top was
**4,948.460 m** away (nearest returned candidate: 3,637.415 m). Every result was
ambiguous. [free-reference-variants.json](free-reference-variants.json) records
these separate experiments, source-part hashes, media/report hashes and revision.
They do not replace failures in the fixed ten-case benchmark.

Two independent reporting bugs were then reproduced with failing regressions
and fixed in `1e8b23168837430617da79ecb0db72fab427d8cf`. Incomplete temporal
coverage was counted after an early return that made its warning unreachable.
Pressure-altitude warnings counted positions outside the selected region and
positions removed by input sampling. Both warnings now describe the applicable
input. These changes do not alter coordinates, detection, ranking or acceptance.
All **41** local software tests, formatting, strict Clippy and release compilation
pass. [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41840226683)
both passed the same checks on that revision.

An audio-only diagnostic also inspected the creator's linked raw challenge.
It has no independently established precise camera coordinate in this benchmark
and is not counted as a location-validation case. Adding 180-second slow-detection
windows did not improve the useful curves or the Lake Hood detections; that
experiment was reverted. A mandatory harmonic filter would also remove one
useful original-challenge event, so it was not adopted. No thresholds were
relaxed to turn these failures into apparent successes.

## Completed ten-case evaluation

All ten original jobs completed with the same executable from
`38f7713c5c82db78876cc18c8b25f8a8e062916b` (SHA-256
`902716cae1f1b207ab21bfb24b78954093907158d83c2045789433aab742e4cf`).
Every input retained its original broad region, full reference day, media hash,
and `start_utc: null`. No known aircraft identities or recording-clock hints
were supplied. The subsequent reporting-only revision above leaves this
matching behavior unchanged.

| Case | Top candidate error | Result |
| --- | ---: | --- |
| midway | No candidate | Insufficient evidence |
| southampton | No candidate | Insufficient evidence |
| midway-arrivals | 27,442.985 m | Wrong, ambiguous |
| midway-holdout | No candidate | Insufficient evidence |
| midway-nov1 | 92.250 m | Coordinate hit, ambiguous, four flights |
| midway-nov1-holdout | 26.670 m | Coordinate hit, ambiguous, three flights |
| southampton-nov1 | No candidate | Insufficient evidence |
| lake-hood | 8,788.903 m | Wrong, ambiguous |
| lake-hood-nov1 | No candidate | Insufficient evidence |
| lake-hood-nov1-later | 10,607.842 m | Wrong, ambiguous |

The second Midway clip now survives the original broad search using all regional
flights. Profiling the same shared clock model before beam pruning fixed that
miss; the improvement was not obtained by supplying the correct aircraft or a
smaller search area. Nevertheless, the overall result is **2/10 coordinate
hits, 0/10 accepted unambiguous locations**. Both hits are from the same camera.
The other locations and the two additional free-provider Lake Hood experiments
remain unsuccessful. Real-world reliability is not established; acceptance and
release readiness remain unmet.

## Later Lake Hood development result

Revision `5df0e544801be1a46556d11d8fc9d4474bfbe12b` removes persistent background tones.
It also searches the same distance range that its feature comparison permits.
Separate synthetic regressions reproduced both failures before the fixes.
All 51 software tests passed locally. [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41925244279) also passed.

The original broad `lake-hood-nov1-later` job then returned seven ambiguous candidates.
The first candidate was **270.461 m** from the published camera position.
The second was **188.920 m** away. Both matched the same three flight segments.
The first inferred start time was `2026-11-01T21:30:01.360Z`.
The job supplied no recording clock, camera coordinates, or aircraft identities.
It searched 7,900,065 hypotheses using the full regional reference day.
The executable SHA-256 was `9a3ce13e3f6d965828ed7a82c4d55c0f1f03a93301c3573e6a2572e78fdc8777`.

This remains a failed acceptance case. The first candidate exceeds the unchanged 250 m limit.
A nearby second candidate does not count as a first-choice success.
The remaining sounds and alternative locations also leave the answer ambiguous.

Separate diagnostics used the known camera area to inspect missed sound curves.
One spectral diagnostic also used the known recording clock.
These diagnostics are excluded from acceptance results.
The full-day known-area tests recovered three matched flights with two free providers.
They helped identify detector and search failures; they do not establish automatic location accuracy.

The latest revisions have not completed another full ten-case benchmark.
`results.json` records each evaluated revision and marks the current mixed development results.
Historical failures remain in that file.

The next detector change adds overlapping 40-second windows for brief propeller sounds.
A separate synthesized waveform reproduces interference from temporary background tones.
The old detector fails that regression. The revised detector passes it.
All 52 software tests pass locally, including the longer-curve and quiet-gap regressions.
The known-area Lake Hood diagnostic now finds four consistent flights with no recording clock supplied.
This is still a diagnostic. The original broad jobs must be evaluated separately.

The broad run on `d55a80f4260d8603fc76145b1b1f117d34dcc192` then found four matches in the later Lake Hood recording.
Its first candidate was **272.579 m** from the camera, so it still failed the 250 m criterion.
The result remained ambiguous. The other jobs on that revision were still running when this record was updated.

## Further detector checks

Longer generated-noise controls exposed weak false flyovers at the 6 dB short-pass default.
A selected 30-second excerpt now provides a regression input.
It contains generated Gaussian noise, changing wind-noise levels, and a constant tone.
It contains no recorded aircraft. Its generator and hash are in `tests/fixtures/README.md`.
The default short-pass contrast is now 8 dB. Explicit settings in saved jobs are preserved.

A separate fitting regression exposed incorrect speed and distance estimates for a clear short sound curve.
The fitter now solves the frequency coefficients directly while searching the curve center and width.
It uses the same physical model and parameter bounds.
The new regression uses independent propagation-delay calculations.
The longer-curve regression also caught a short-window selection problem; supported 40-second curves can now guide longer traces.

All 54 software tests, formatting, strict Clippy, and release compilation pass locally with these changes.
Real-location acceptance is still unmet. These checks do not replace the original broad, unknown-clock tests.


## Alternative sound observations

An improved fit recovered the earlier Lake Hood curves in a known-area diagnostic.
It also exposed a selection problem in the later clip: a short fragment could displace a useful longer trace.
The detector now retains up to four observations per activity group.
The flight search can use any retained observation, but the group can support only one match.
The original coverage, curvature, and location-acceptance checks remain in place.

A software regression then caught a misleading location caused by a short subset of a complete sound curve.
The detector now removes redundant short subsets by comparing their measured frequency curves, including harmonics.
This decision uses no flight positions, recording clock, or camera coordinates.
The failing quiet-gap test passes after that fix.
All **56** software tests, formatting, strict Clippy, and release compilation pass locally.
The new real-recording diagnostics and negative controls are recorded separately from location acceptance.


The two Lake Hood known-area diagnostics now each match five flight segments.
Their first candidates are 17.19 m and 209.31 m from the published camera pin.
Both results remain ambiguous. The supplied camera area makes these diagnostics unsuitable as location-accuracy tests.
The earlier Lake Hood report is exactly unchanged after the regional time-index optimization.

Twelve independent ten-minute generated-noise controls produce no detections at the 8 dB default.
This totals two hours of noise. It does not measure false alarms on real traffic or aircraft recordings.
The control summaries, input hashes, and diagnostic limitations are in [observation-controls.json](observation-controls.json).
To reproduce a full control, use `python validation/generate_noise_fixture.py --seed 731 --full-control --output control.wav`.
The selected 30-second regression and the full seed-731 WAV both reproduce their recorded hashes.

All 56 release software tests and required local checks pass on `e1aa6bc03295aca469aa35667e49393fa5c743fb`.
[Windows and Ubuntu CI also pass](https://github.com/Donovoi/soundgeo/actions/runs/41940895711).
The original ten broad-area cases are running on that executable.
The older 52-test benchmark stopped after eight completed cases; two slow Midway runs were cancelled to free CPU time.
The old, unindexed main-Midway known-area diagnostic was also cancelled and replaced by a run on the current executable.
Cancelled runs are incomplete. Their logs and metadata remain available; they do not count as completed tests.

## Additional free airport sources

These checks are diagnostics. They do not count as successful location tests.
The recording start time was not supplied to either Southampton search, but
the search areas were narrowed around the known cameras.

The [Chicago Department of Aviation](https://www.flychicago.com/community/ordnoise/flighttracker/pages/default.aspx)
provides public radar replay through WebTrak. A partial-day coverage audit
found positions 37 to 40 seconds before closest approach for five inspected
Midway departures. The existing ADSB.lol traces usually began at or after
closest approach. This source therefore fills a measured reference-data gap.
The full-day download and the location checks are still in progress.

The new native Rust provider checks every half-minute part, UTC/local time
agreement, point coordinates, and cache checksums. Tests cover the repeated
daylight-saving hour, the altitude field, missing parts, cancellation, cache
corruption, and command-line analysis with no recording clock.
All 61 software tests pass locally, with formatting and strict Clippy checks.
The detector and location matcher are unchanged from `e1aa6bc`.

The [Southampton Airport noise page](https://www.southamptonairport.com/about-us/aircraft-noise/)
links its public Volans tracker. Its November 1 and November 3 data produced no
candidate in either known-area diagnostic. November 3's published range ended
at 23:27:46 local time, with some returned track tails extending later. It is
not claimed as complete coverage through midnight. The original altitude
sensor and pressure correction are unknown. This source is not integrated
into the Rust app.

A separate local YAMNet sound-classification diagnostic suggested water and
wave noise in the Southampton detections. It also missed all ten observations
used in the two Lake Hood known-area fits, even after level normalization.
It is therefore not used to reject detections. These predictions are not
ground-truth labels. The model was downloaded from Google's public
[MediaPipe model release](https://ai.google.dev/edge/mediapipe/solutions/audio/audio_classifier/index);
recordings were processed locally.

Compact evidence is in [airport-source-diagnostics.json](airport-source-diagnostics.json).
The completed Midway November 1 regional diagnostic has a first candidate
42.4 m from the camera with five matches. It remains ambiguous, uses the known
camera area, and is excluded from acceptance. Its summary is recorded in
[observation-controls.json](observation-controls.json).
