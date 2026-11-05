# Real-video validation

This is a failing acceptance benchmark, not a claim of reliable geolocation.

The completed fixed-build benchmark has nine reports and one search-limit error.
Two first choices are within 250 m. Both remain ambiguous. Acceptance is **0/10**.
The failure does not inherit an older successful coordinate result. See
[qualified-batch-results.json](qualified-batch-results.json).

A separate experimental search improved the later Lake Hood first choice to
82.787 m. It also found a five-match alternative 479.261 m away. It remains
ambiguous. Two preselected Lake Hood recordings are testing that fixed build.
The current main app passes 71 software tests locally and in Windows and Ubuntu
CI. This does not pass the real-video benchmark.

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
The native provider downloaded all 2,880 parts for November 3.
All 560,786 positions match an independent Python decoder in every compared field.
The radar was not marked online in 25 parts. A complete download does not mean
that every aircraft was recorded.

Three known-area searches used that full day without recording start times.
The 20-minute Midway clip produced a first candidate 50.1 m from the camera,
with five matched sounds out of six. A second candidate also matched five
sounds. The result remains ambiguous. The other two Midway clips produced no
candidate. These searches used the known camera area and do not pass the
broad-area acceptance test. The broad search is still running.

The new native Rust provider checks every half-minute part, UTC/local time
agreement, point coordinates, and cache checksums. Tests cover the repeated
daylight-saving hour, the altitude field, missing parts, cancellation, cache
corruption, and command-line analysis with no recording clock.
All 62 software tests pass locally, with formatting and strict Clippy checks.
[Windows and Ubuntu CI pass](https://github.com/Donovoi/soundgeo/actions/runs/41949614611).
The detector and location matcher are unchanged from `e1aa6bc`.

A later download stopped when the server closed a response early. The app now
retries transient connection failures up to three attempts. It keeps valid
cached parts for reuse. A local server test covers the truncated response and
confirms that a forbidden response is not retried.

The search now scores independent proposals on its existing four-worker pool.
Serial and parallel scoring produce bit-for-bit equal values in the regression
test. A separate real-recording comparison also preserves every event,
candidate, score, clock, match, and hypothesis count. It examines 319,745
hypotheses in a known-area corridor search. Runtime fell from 163.1 s to 94.4 s.
This is one timing sample with other searches active, not a controlled benchmark.
All 63 software tests, formatting, strict Clippy, and both release builds pass
locally and in [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41952966274).

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

## False support from a clock-rate fit

Four broad-area runs on `163c76f` are complete. None passes acceptance.
The November 1 Southampton run returned one candidate with three matched
sounds and a fitted clock rate of 1.004296875. Its first location was 7,576 m
from the camera. The report status was `candidates_found`.

With the clock rate fixed to 1, the same recording, full-day references, and
original broad area produce no candidate. The recording start time remains
unknown in both runs. This is evidence of a false match; it does not prove
that every false match requires clock drift.

A fitted rate supported by only three sounds now keeps an ambiguity flag.
Three sounds are the minimum fitting consensus. Additional independent support
is required before that flag can be removed. This guard does not change the
location estimate and does not establish the accuracy of four-sound results.
The three- and four-flight synthetic controls preserve clock-rate recovery.
Evidence is in [clock-support-diagnostic.json](clock-support-diagnostic.json).

The completed replay on `35ba97d` keeps every candidate and score unchanged,
but marks the wrong Southampton result `ambiguous`. The location error remains.
All 63 software tests and the required local checks pass on this revision.

## Completed November 1 airport download

The resumed native Chicago download saved all 2,880 parts and 607,245 positions.
Every compared field matches the independent Python decoder exactly, including
times across the daylight-saving transition. The radar was not marked online
in 34 intervals. The archive can still omit flights.

The main Midway known-area diagnostic returns a first candidate 111.3 m from
the camera with four matches. The other recording returns one candidate
26.9 m away with three matches. Each recording has eleven detected sound
groups. Both results remain ambiguous and are excluded from acceptance.

The older broad Midway holdout run on `e1aa6bc` reached its 20-million-hypothesis
budget before finishing. It returned an error and no partial result. Its failed
run and logs are retained. A larger search budget must be recorded as a new run.

The broad November 3 Chicago airport-source run also reached its original
20-million-hypothesis limit. It stopped after 5,921.6 seconds with no report.
It used the original search area, the full reference day, and no recording
start time. This is a failed search, not an accuracy result. Its source and
failure record are in [airport-source-diagnostics.json](airport-source-diagnostics.json).

## Conservative distance filter

The flight index now rejects tracks whose position bounds are outside the
sound's allowed range. The bounds include movement between position fixes.
Cached queries are checked again for the exact observer position and radius.
Tests cover crossings between fixes, minute boundaries, missing-data gaps,
and nearby queries that share a cache entry.

The Midway known-area corridor diagnostic keeps every compared event,
candidate, score, clock, and hypothesis count unchanged. It still examines
319,745 hypotheses. Runtime changed from 94.41 seconds to 64.60 seconds in
these two runs. Other searches were active; this is not a controlled timing
test or evidence of improved location accuracy. See
[index-envelope-diagnostic.json](index-envelope-diagnostic.json).
All 65 software tests, formatting, strict Clippy, and both release builds
pass locally and in [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41962057606).

The CLI also accepts `--max-hypotheses` to retry a search with a different
resource limit. Its checks verify the report's effective limit, preservation
of the saved job, and rejection of invalid values before reading input files.
All 65 software tests and required local checks pass with this option.
The same revision passes [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41963574130).

## Completed broad Lake Hood run

The earlier November 1 Lake Hood recording now has a first candidate 45.749 m
from the camera on revision `163c76f`. The run used the original broad bounds,
the full reference day, the original recording, and `start_utc: null`.
It examined 19,814,757 hypotheses from 1,040 flight segments in 5,956.5 seconds.
No camera coordinates, recording clock, or aircraft identities were supplied.

Five sound groups match five flight segments from three aircraft at the first
location. Each of the other nine returned candidates has three matches.
Nineteen detected groups remain unmatched. The report status is `ambiguous`.
The result meets the 250 m coordinate target, but it does not pass the
unambiguous-result requirement. The correct location survived the broad search;
this case does not support changing pruning to fix a missing location.

The development record now contains three coordinate hits from two cameras,
with zero accepted unambiguous results. These are mixed development revisions,
not a completed benchmark of the latest code. The complete result and hashes
are summarized in [results.json](results.json). A replay with the faster index
retains the original job and 20-million-hypothesis budget.

The later Lake Hood run on `e1aa6bc` reached that budget after 12,194.2 seconds
and returned no report. A separate retry raises only the resource limit to
50 million. The newly started older-code duplicate for the earlier clip was
cancelled after the newer `163c76f` run finished; its cancellation record and
exit code remain in the development results. The current-code replay continues.

A separate sensitivity diagnostic uses only the app's first Lake Hood candidate,
its five matched observations, and the reference tracks. It supplies no camera
coordinates or external recording clock. It samples 41 nearby positions and
adjusts the inferred clock while holding the flight associations and observation
variants fixed. Some sampled points retain all five matches at one clock setting
but lose a curvature check at a setting with lower numerical error.

This is a conditional diagnostic, not another blind location test. Sparse local
sampling cannot bound the full uncertainty region. The app can also select
alternative observations, so these fixed-variant results alone do not establish
a refinement bug. Evidence is in [lake-hood-sensitivity.json](lake-hood-sensitivity.json).
The diagnostic can be reproduced with `cargo run --release --example profile_candidate -- report.json sensitivity.json`
when the report's original reference data is available.

## Completed broad Chicago retry

The November 3 `midway-arrivals` retry on `7f95301` finished in 3,393.53 seconds.
It used the full free Chicago airport archive, original bounds, and no recording
start time. The explicit resource limit was 50 million hypotheses; the failed
20-million run remains recorded. Its first candidate is 235.572 m from the
camera with five matches from six detected groups. The next candidate has four
matches. The result remains ambiguous and does not pass full acceptance.

This run gives another coordinate hit, but does not establish reliability.
The result is recorded in
[results.json](results.json) and [airport-source-diagnostics.json](airport-source-diagnostics.json).

## Later Midway holdout result

The November 1 Midway holdout retry on `35ba97d` finished in 6,580.7 seconds.
It used the original bounds, full reference day, and no recording start time.
The search limit was explicitly increased to 50 million hypotheses after the
20-million run failed. It returned five ambiguous candidates, each with three
matches. The first was **24,606.237 m** from the camera.

The earlier 26.67 m result did not survive this later run. It remains in the
history, but the latest completed result for this case is a failure. The mixed
development record now has three coordinate hits and zero accepted results
across ten recordings. This is not a completed benchmark of one revision.

## Support lost during refinement

A further diagnostic used the actual fitting function with all observation
variants enabled. At a fixed position 250 m north of the app's Lake Hood
estimate, adjusting the inferred clock reduced the core error from 0.5275%
to 0.3173%, but reduced the valid curved matches from five to three.
No camera coordinates or external recording time were supplied.

An independent synthetic control reproduces the same trade-off. Four curves
pass at one position. Moving 500 m makes three curves fit almost exactly but
loses the fourth. With intermediate-result retention disabled, the full search
regression returns three matches and fails. With retention enabled, it returns
four matches within 250 m of the fixture's first position.

Refinement now retains its strongest valid intermediate fit when it has extra
matches beyond the minimum fitting consensus. The original final point remains
available. An intermediate point is added only if it has more valid matches
than that final point. All coverage, curvature, clock-span, residual, and
quiet-gap checks remain in force. Search generation and the fitting objective
are unchanged. This addresses discarded supporting evidence; real-video
accuracy still requires validation. See
[refinement-support-diagnostic.json](refinement-support-diagnostic.json).

Revision `22393e3` passed all 66 release software tests, formatting, strict
Clippy, documentation tests, and both release builds locally.
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41970417866)
also passed. Software checks do not establish location accuracy.

The full Midway, Midway holdout, and November 1 Southampton controls returned
exactly the same candidates, status, detections, and hypothesis counts as their
preceding runs. The two Midway controls returned no candidates. Southampton
still returned the wrong ambiguous candidate 7.6 km away. The known-area
Chicago corridor comparison also preserved its results exactly. These are
regression checks, not additional accuracy cases.

The separate Lake Hood replay on `c5c9f59` reproduced all ten candidates and
19,814,757 hypotheses exactly. Its first result remains 45.749 m from the camera
with five matches and an ambiguous status. Evidence is in
[refinement-real-comparisons.json](refinement-real-comparisons.json).

## Competing flight choices

The previous near-camera Midway holdout candidate still passes the current
fitter with three matches and 0.683% joint error. This check supplied the prior
app-selected position, clock, and flight associations. It did not supply an
external clock or camera coordinates. A second diagnostic restricted the full
reference day to the three previously selected aircraft. It recovered a nearby
candidate over the original broad bounds with no recording clock. Neither
diagnostic counts as an accuracy pass.

A regression test exposed a separate error in proposal construction. An early
sound could take a flight that was the only option for a later sound. The
proposal was then rejected before the alternative assignments were compared.
The test fails with the old construction. The fix keeps the competing choices
until independent assignment. The test also checks a different sound order and
rejects a proposal that needs to count one flight twice.

The contribution of this error to the broad Midway failure is not yet known.
See [proposal-assembly-diagnostic.json](proposal-assembly-diagnostic.json).

Revision `c16c428` passed all 67 software tests, formatting, strict Clippy,
documentation tests, and both release builds locally.
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41972135344)
also passed. It preserved the same three full controls and the known-area
Chicago corridor results exactly. Its effects on the larger searches still
require testing.

## Clock intervals skipped before refinement

Fifteen local proposals around the prior Midway app candidate were checked
against the full reference data. The competing-flight construction fix did not
change these proposals. All three expected flight options remained available.
This diagnostic does not attribute the Midway failure to that construction bug.

The clock search had a separate problem. It refined only the best coarse clock
choice. A narrow interval with a better fit could start with a worse coarse
score and never receive refinement. Refining each coarse start separately
recovered the prior three-flight fit at the center point. The joint error fell
to about 0.68%. This was a seeded diagnostic, not a new location test.

A synthetic regression uses three close flyovers and three slightly wider
decoy tracks. The old search selects the decoy clock rate and fails the test.
The new search recovers the generated rate and true flight associations. It
also reuses completed coarse clock searches when unchanged proposals pass
through repeated pruning. Serial and parallel output checks pass.

Revision `2988d77` passed all 68 release software tests, formatting, strict
Clippy, documentation tests, and both release builds locally.
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41973802326)
also passed. The production fitter exactly reproduced the diagnostic's improved
scores and match lists on all 15 proposals. See
[clock-basin-diagnostic.json](clock-basin-diagnostic.json).

The three full real controls kept their previous outcomes. The two Midway
controls returned no candidates. Southampton still returned an ambiguous
candidate 7.6 km away. The known-area Chicago check kept its four candidate
locations and matches; one clock rate changed by one floating-point rounding
step. It took 137.84 seconds, compared with 78.55 seconds for the preceding build.
Other searches were active, so this is not a controlled speed test. See
[clock-corridor-comparison.json](clock-corridor-comparison.json).

The larger searches have not finished with this revision. Two further hour-long
recordings from November 4 at the same Midway camera were selected before
capture and analysis. Their source dates have been checked independently.
They are separate from the ten development cases and do not add a new camera
location. Their full free reference day is being downloaded. Recording clock
readings remain in validation notes and are not supplied to the app.
The fixed [capture plan](fresh-cases-plan.json) and
[prepared media hashes](fresh-cases-prepared.json) are recorded before analysis.

The earlier Lake Hood replay on `22393e3` finished in 4,292.6 seconds.
It exactly reproduced the preceding report's job, sounds, ten candidates,
status, and hypothesis count. The first location remains 45.749 m from the
camera, with five matches and an ambiguous status. Retaining supported
intermediate fits did not change this case. The repeat on `2988d77` is running.
See [refinement-real-comparisons.json](refinement-real-comparisons.json).

## Later Lake Hood retry and clock-fit cache

The later Lake Hood retry on `c5c9f59` completed 21,459,186 hypotheses in
6,168.9 seconds. The explicit limit was 50 million. Its first candidate is
5,384.709 m from the camera, with four matches. It returned ten ambiguous
candidates. The earlier 272.579 m result remains in the history, but the new
result is the latest failure for this recording. The 250 m target is unchanged.

The 16 detected sound groups exactly match those in the earlier known-area
diagnostic that found five supporting flights. The broad search did not retain
that five-match solution. The known-area diagnostic supplies a location hint
and cannot count as a successful location test.

Clock-rate evaluation now caches repeated rates within each fixed proposal.
The key uses the exact floating-point bits. All 68 software tests, formatting,
strict Clippy, documentation tests, and both release builds pass locally.
The cached Chicago diagnostic exactly preserves its job, sounds, candidates,
status, track count, and hypothesis count. It took 267.49 seconds. An adjacent
baseline repeat took 334.50 seconds; an earlier baseline took 137.84 seconds.
Other searches were active, so these times do not establish a general speedup.
See [clock-cache-comparison.json](clock-cache-comparison.json).

Revision `e947120` also passes
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41979243404).
The November 4 Chicago download completed with 575,365 positions. All 2,880
requested parts were saved and parsed; 43 parts were not marked online by the
source. Complete file coverage does not establish complete flight observations.
The two preselected recordings are running on the planned `2988d77` build.

Further Lake Hood diagnostics use the prior known-area candidate as an explicit
hint. With the current model, its five flights still fit the unchanged sounds
at 0.8233% joint error. Actual corridor generation for three of those aircraft
produced a proposal about 30 m from that prior app estimate. Profiling nearby
proposals gave at most four valid matches. Passing those 363 nearby proposals
to the unchanged final refinement step recovered three five-match candidates.

This supports a proposal-selection problem. It does not prove which pruning
stage caused the full-search failure. The latest build must complete the broad
case before another selection change is justified. That retry is queued after
the native Chicago holdout, with the same media, broad bounds, full reference
day, and unknown recording clock. Its resource limit is explicitly 50 million.
See [lake-later-search-diagnostics.json](lake-later-search-diagnostics.json).
None of these hinted diagnostics counts as a location-accuracy pass.

The full native Chicago holdout run on `22393e3` finished in 6,123.14 seconds.
It used the original Midway November 1 holdout recording, broad bounds, complete
reference day, and no recording start time. The resource limit was 50 million.
It returned six ambiguous candidates with three matches each. The first was
26.9 km from the camera. The denser source did not fix this recording.
The latest mixed record still has three coordinate hits and zero accepted cases.

The independent Python decoder also checked the complete November 4 Chicago
archive. All 575,365 positions and all nine compared fields matched exactly.
The check includes file hashes for all 2,880 source parts and confirms the 43
parts that were not marked online. See
[airport-source-diagnostics.json](airport-source-diagnostics.json). This checks
the downloader and parser, not location accuracy.

## Reuse of travel-time calculations

Clock trials at one fixed observer now reuse each flight's sample arrival
times. The cache is discarded before the observer or sound speed changes.
A regression checks exact Doppler values and coverage decisions at sample
boundaries, inside gaps, on empty and single-point tracks, and at different
observer positions and sound speeds. All 69 local software tests, formatting,
strict Clippy, documentation tests, and both release builds pass.

The Chicago corridor diagnostic preserves every compared report field exactly,
including all candidate values. It took 235.66 seconds, compared with 267.49
seconds for the preceding cache build. An alternating comparison in one
process also checks ten real-data proposals. All six paired rounds gave the
same fits and a lower runtime for the prepared travel-time calculations.
Other searches were active. The routine timing does not establish a general
end-to-end speedup. See [arrival-cache-evidence.json](arrival-cache-evidence.json).

Revision `a25bd0d` also passes all 69 tests in
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41983866778).
All three complete-area controls exactly preserve the preceding results.
The two Midway controls return no candidates. Southampton still returns one
ambiguous candidate 7.6 km away. See
[arrival-cache-controls.json](arrival-cache-controls.json).

Four obsolete development runs were stopped after those checks passed. Their
logs, hashes, terminal exit codes and cancellation records are preserved.
They are not counted as successful results. The remaining seven development
cases are now running or queued on the same qualified build, with four workers.
They keep their original recordings, full references, broad bounds and unknown
clocks. The resource limit is explicitly 50 million hypotheses per case.
See [qualified-batch-consolidation.json](qualified-batch-consolidation.json).
The preselected November 4 recordings keep their original planned build and
acceptance criteria.

## Completed preselected November 4 recordings

Both new one-hour Midway recordings finished on their planned revision,
`2988d77`, without a recording start time. They used the original broad box,
the complete downloaded Chicago reference day, and a 50-million-hypothesis
limit. Both returned `insufficient_evidence`, with no candidates. Acceptance
is **0/2**, with no coordinate hits. A complete download does not establish
that every audible aircraft has complete position data.

| Recording | Detected events | Hypotheses | Runtime | Candidates |
| --- | ---: | ---: | ---: | ---: |
| midway-nov4-a | 9 | 19,227,476 | 6,389.21 s | 0 |
| midway-nov4-b | 4 | 7,964,629 | 710.46 s | 0 |

These are new recordings from the existing camera, not a new location.
Their plan, source files, dates, bounds and executable were fixed before
evaluation. The second run finished before the combined results were reviewed.
See [fresh-cases-results.json](fresh-cases-results.json). These failed results
do not replace any of the original ten cases.

## First result from the consolidated benchmark

The Midway November 1 holdout finished on `a25bd0d` with the original broad
area, complete regional reference, no recording clock and an explicit
50-million-hypothesis limit. Its first candidate is **28.393 m** from the
camera. All five returned candidates have three supporting sounds; the app
still reports `ambiguous`. This is a coordinate hit, not an acceptance pass.

The run examined 23,596,939 hypotheses and took 6,586.9 seconds while other
searches were active. Four of the original ten cases have now completed on
this revision. The latest mixed-revision record has four coordinate hits and
zero accepted results. The remaining consolidated runs continue unchanged.
See [results.json](results.json) for the executable hash, report hash and
preserved earlier failures.

## Further checks on the November 4 recordings

The two Midway audio channels are almost identical. Mixing them to mono does
not cancel the sound. Checks also found no source-offline flags during either
recording. These checks do not establish complete coverage of each aircraft.

A separate experiment used the times of loud broadband sounds. It searched
the full regional area and reference day without the camera position or
recording clock. After local refinement, its first choices were 317 m and
312 m from the camera. Both missed the 250 m target. Similar scores also
occurred at other positions, including a distant alternative for the first
recording. This method was not added to the app. See
[fresh-midway-audio-diagnostics.json](fresh-midway-audio-diagnostics.json).

## New Lake Hood recordings

Two further one-hour Lake Hood recordings from November 4 were selected and
captured before evaluation. Their source dates were checked from visible
camera clocks. Those clocks are kept in the local capture record and are not
supplied to the app. The app downloaded and verified the free full-day ADSB.lol
archive through its command line.

The executable, original broad area, 50-million-hypothesis limit and 250 m
target are fixed in [fresh-lake-cases-plan.json](fresh-lake-cases-plan.json).
The experimental executable is `a5e0a99`; it preserves additional proposals
supported by at least four complete curves. Its 72 local software tests pass.
It has not completed real-video qualification. Its existing Lake Hood run
has finished. The two new recordings are now running, with no recording clock supplied.
See [fresh-lake-cases-prepared.json](fresh-lake-cases-prepared.json).

## Further results from the same benchmark build

All ten original attempts have now finished on `a25bd0d`. Nine returned reports.
The first Midway November 1 run reached the 50-million-hypothesis limit.
It returned no partial location. The earlier
November 1 Lake Hood run again returned a first candidate 45.749 m from the
camera, with five matching sounds. It remains ambiguous. The later
November 1 Lake Hood run returned a first candidate 5,215.866 m from the
camera, with four matching sounds. The November 3 Lake Hood run returned
a first candidate 6,783.075 m away. Both results are ambiguous and fail the
250 m target. Southampton also remains ambiguous, with one matching sound
and a first candidate 13,621.587 m away.

Midway arrivals returned a first candidate 26,658.142 m away, with three
matches. This run used the original ADSB.lol reference data. The earlier
236 m result used Chicago airport replay data and a different build.
Both results remain ambiguous. The earlier result is preserved; it does not
replace this failure. Two separate comparisons now use the current main
build and Chicago data, with the original regions and no recording clocks.

This fixed-build benchmark has two coordinate hits and zero accepted results.
The older 92 m Midway result does not replace the failed current attempt.
See [qualified-batch-results.json](qualified-batch-results.json) for every planned outcome.
The historical mixed-revision record still has three hits among completed reports.
It keeps the failed attempt separately. See [results.json](results.json).

## Sound timing as a search aid

A separate experiment used broadband sound peaks to propose a location and
clock for the existing Doppler search. The input was the original broad
region, full reference day and audio. No camera position or recording clock
was supplied. The follow-up searches used only proposals computed from those
inputs. This Python experiment is not a feature of the Rust application.

The first November 1 Midway recording produced two qualifying proposals.
Their follow-up searches returned first candidates 19.741 m and 38.573 m from
the camera, each with five matching curves. Both reports remain ambiguous.
The second Midway recording and both older Lake Hood recordings produced no
proposals that passed the fixed timing requirements.

Timing checks at the final Midway candidates did not resolve the ambiguity.
Some wrong nearby candidates had better timing scores than the correct ones.
These results do not justify a new acceptance rule. The experiment did not
replace the original benchmark results or change the fresh Lake Hood tests.
See [timing-seed-results.json](timing-seed-results.json),
[lake-timing-seed-results.json](lake-timing-seed-results.json), and
[fixed-candidate-timing-support.json](fixed-candidate-timing-support.json).

## Reuse of flight segment geometry

Revision `0c2565c` reuses fixed geometry while it tries different recording
clocks at one position. Each track keeps at most 64 cached segments. Replacing
a cache entry repeats the original calculation. A regression checks exact
floating-point results after cache collisions, missing intervals and invalid
speeds, with different positions and sound speeds.

All 70 local tests pass, with no ignored tests. Formatting, strict Clippy,
documentation tests and both release builds also pass. The same code passes
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42017950114).
The two complete-area controls preserve all compared report fields exactly.
They still return no candidate for the Midway holdout and an ambiguous wrong
candidate for Southampton. This change improves reuse of calculations; it
does not establish location accuracy. The running benchmark keeps its original
executable. See [segment-cache-evidence.json](segment-cache-evidence.json).

## Fitting all accepted curves together

A further experiment refined every accepted curve together, while keeping
the original observations, flight assignments and acceptance checks. It kept
the original candidates as alternatives. This code is on a separate branch
at `6ce0753`; it is not part of the main application.

Its 74 local software tests and Windows and Ubuntu CI checks pass. The two
complete-area controls remain exactly unchanged. Three further diagnostics
used regions and clocks computed by earlier searches, with the same inputs
for both versions. These are not broad-area acceptance tests.

| Diagnostic | Previous first error | New first error | Result |
| --- | ---: | ---: | --- |
| Midway | 19.741 m | 39.309 m | Ambiguous |
| Earlier Lake Hood | 20.521 m | 28.933 m | Ambiguous |
| Later Lake Hood | 5,347.023 m | 5,322.713 m | Ambiguous |

The curve error fell, but the two nearby first candidates moved farther from
the camera. The wrong Lake Hood result stayed more than 5 km away. This did
not establish better location accuracy. See
[full-consensus-experiment.json](full-consensus-experiment.json).

## Full-area result from the additional-proposal experiment

The separate `a5e0a99` search finished the later November 1 Lake Hood case.
It kept the original recording, full reference data and broad search area.
No recording clock was supplied. The budget stayed at 50 million hypotheses.
The detected events, job settings and reference paths match the baseline exactly.

Its first candidate is **82.787 m** from the camera, with five matching sounds.
The previous first candidate was 5,215.866 m away, with four matches.
The experimental search preserves that previous candidate as its third choice.
The new second choice is 479.261 m away and also has five matches.
The report remains `ambiguous`. This is a coordinate hit, not an acceptance pass.

The three control cases remain unchanged. The two preselected November 4
Lake Hood recordings have started on the same frozen experimental executable.
This experiment is separate from the main benchmark and is not merged into
the main app. See [corroboration-probe-result.json](corroboration-probe-result.json).

A follow-up used a region and clock computed from both five-match candidates.
It supplied no camera position or external recording clock. Both versions
received the same local job. Joint fitting moved the first candidate from
108.563 m to 83.034 m from the camera. The second moved from 473.175 m to
303.134 m. Both reports remain ambiguous, with further alternatives outside
250 m. This computed-proposal diagnostic does not count as another blind
test. See [lake-five-curve-followup.json](lake-five-curve-followup.json).

## Local sound-classifier diagnostic

A separate diagnostic tested the free
[AST AudioSet model](https://huggingface.co/MIT/ast-finetuned-audioset-10-10-0.4593).
It ran locally. No recording was uploaded. It used the raw sound and a second
version with volume normalization. The model received no flight data or location.

The first run classified primary observations. Some flight matches used
different intervals retained as alternatives. A pinned supplement checked all
11 selected alternatives. The original results remain preserved. All 158
case/observation/volume results were checked against their saved score matrices.

At the descriptive 0.5 threshold, both volume settings gave aircraft scores
above that threshold for eight of 11 observations in the first Midway clip
and six of 11 in the second. None of the ten model-selected Lake Hood
observations reached it. Neither reported flyover from the source recording
reached it. These comparisons are not independent audio labels; they do not
measure classification accuracy.

The classifier could remove useful quiet observations. It was not added to
the app. No detection, ranking or acceptance rule changed. See
[audio-classifier-ast-diagnostic.json](audio-classifier-ast-diagnostic.json).

## Command-line rejection of ambiguous results

Revision `2a0952e` adds `--require-unambiguous`. It saves the report before
returning code 3 for no candidates or code 4 for an ambiguous result.
The existing `--require-match` behavior remains available. This is a status
check for scripts, not proof of a correct location.

The new regression runs real CLI processes on generated audio. It checks
an empty result, a one-flyover ambiguous result, and the two-flyover demo.
It also checks that the two exit policies preserve the same report evidence.
Report creation timestamps are excluded from that comparison.

A first qualification reused artifacts from the separate experimental branch
through a shared Cargo target directory. Its 75-test result was rejected.
The app's release artifacts were cleared and rebuilt from the main source.
The clean run passed all 71 tests, with no failures or ignored tests.
Documentation tests, strict Clippy, formatting and both release builds pass.
The frozen executables used by the running video tests stayed unchanged.
Each checkout must now use its own target directory, as recorded in `AGENTS.md`.
The same revision passed all 71 tests in both Windows and Ubuntu CI. The CI
logs were checked for the expected count and source test names. See
[strict-cli-evidence.json](strict-cli-evidence.json).

## Additional spectral evidence

A diagnostic compared all matched curves in two completed Lake Hood reports
with the raw spectrogram. It checked the primary tone and related harmonics.
It preserved match-count priority and used no camera position to score them.

The extra spectral score favored the later recording's 479.261 m candidate
over its 82.787 m candidate. The earlier recording's first choice stayed at
45.749 m. This rule made one first choice worse, so it was not added to the
app. No result was accepted. See
[candidate-spectral-support.json](candidate-spectral-support.json).

## Fixed main-matcher Chicago comparison

The automatic Chicago archive run finished the Midway arrivals recording.
It used revision `0c2565c`, the original broad area, the full free reference day,
and no recording clock. It evaluated 34,535,596 hypotheses in about 80 minutes.
Its first choice is 235.572 m from the camera, with five matching sounds.
The second choice is 488.292 m away, also with five matches.
The report remains ambiguous. The other planned case, Midway November 1,
reached the 50-million-hypothesis limit after 13,212 seconds. It saved no partial
location. Both planned attempts are finished, with no acceptance passes.
This comparison does not replace the ten-case benchmark. See
[native-main-cache-results.json](native-main-cache-results.json).

## Terrain sensitivity

A fixed diagnostic compared the existing constant-height model with free
Mapzen Skadi terrain. The test boxes and clock intervals came from all
maximum-match candidates in the prior reports. They did not use the camera
coordinates or an external recording clock. The checks are local diagnostics,
not broad-area acceptance tests. Both versions used the same events and data.

The Lake Hood first choice stayed 108.563 m away. Its second choice moved
from 473.175 m to 346.097 m. The Chicago first choice moved from 248.298 m
to 70.411 m, with a competing choice 322.803 m away. Both remain ambiguous.
All planned comparisons finished. See [terrain-followup.json](terrain-followup.json).

The app now has an optional free terrain downloader. This removes a manual
data-preparation step. It does not change the location acceptance rules.
All 76 local tests pass, with no failures or ignored tests. The five new tests
cover tile selection, cache integrity and repair, invalid downloads, cancellation,
the date-line boundary, and CLI use. Automatic and manual terrain input produce
the same candidate evidence in the CLI test. Formatting, strict release Clippy,
documentation tests and both release builds pass.
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42047220138)
each passed all 76 tests. The logs were checked for the expected source test names.

The native CLI downloaded all six real terrain tiles from an empty cache.
Their bytes match the independently downloaded files. Two paired real-video
diagnostics then produced identical events, candidates, hypotheses, status,
and flight references with automatic and manual terrain input.
An initial test-script assertion incorrectly required a null clock in these
computed-proposal jobs. The corrected check preserves the original inferred
clock. The first log and completed runs remain saved; they were not rerun.
See [terrain-download-evidence.json](terrain-download-evidence.json).

## Whole-day temperature diagnostic

The app uses 15 °C unless another value is supplied. A separate diagnostic
downloaded free routine weather reports from the Iowa Environmental Mesonet.
It used all stations within each original broad area and the whole UTC day.
It calculated hourly station medians, daily station medians, and a median
across stations. It used no camera position or recording clock to select weather.

The Lake Hood reference was 1.389 °C. With that temperature, its local first
choice moved from 108.563 m to 233.326 m. Its next choice was 607.289 m away.
The Chicago reference was 17.778 °C. Its first choice moved from 70.411 m to
69.059 m, with a competing choice 327.588 m away. Both remain ambiguous.
Temperature changes also changed the fitted sound observations.
Surface temperature is not the average temperature along the sound path.
Automatic weather selection was not added. These local diagnostics do not
count as location acceptance. See [weather-followup.json](weather-followup.json).

## New Lake Hood recordings on the fixed experimental build

The first preselected November 4 recording finished in about 105 minutes.
The `a5e0a99` build used the original broad bounds, full free ADSB.lol day,
and no recording clock. Its first choice is 8,560.765 m from the camera.
All five returned candidates have three matches. The result is ambiguous
and fails the 250 m target. The second preselected recording is still running.
The failed run remains part of the test set. The earlier 83 m improvement
does not establish reliability. See [fresh-lake-cases-results.json](fresh-lake-cases-results.json).

## Further checks on the failed November 4 Lake Hood recording

A diagnostic searched a small area around the known camera position, without
its recording clock. It used the same frozen experimental build and detected
sounds as the failed broad-area run. It returned no candidates. This check
uses the answer position, so it is not an acceptance test. See
[fresh-lake-a-point-diagnostic.json](fresh-lake-a-point-diagnostic.json).

A separate check used both the known position and the visible camera clock.
It compared every retained sound with the available flight tracks over 2,169
clock-offset and clock-rate combinations. No combination passed the three-curve
check. A run with only the slow sound detector restored three omitted observations,
but still found no passing combination. This finite scan does not rule out every
possible fit. The alternate free ADSB.lol archive added no positions more than
30 seconds from a position already in the production archive. These diagnostics
did not justify a change to the acceptance limits. See
[fresh-lake-a-support-diagnostics.json](fresh-lake-a-support-diagnostics.json).

## Check search budgets before matching

Revision `c29f646` counts the proposed corridor locations before it compares
sound curves. A search that needs more hypotheses than its limit now stops early.
It reports no partial location. The proposal geometry and scoring rules are unchanged.

The original failed Midway November 1 job used the same inputs and 50-million
limit. It now stopped in 87.71 seconds, before matching. The original attempt
took 21,838.6 seconds. Both failures remain recorded. Two completed real-recording
controls produced identical events, candidates, status, hypotheses, flight counts,
search settings, duration, and warnings. The budget count matched each report.
These are software checks, not new location-accuracy passes. See
[budget-precheck-evidence.json](budget-precheck-evidence.json).

All 76 local tests pass. The extended regressions check exact budget limits,
early rejection on serial and parallel paths, and unchanged terrain warning counts.
Formatting, strict release Clippy, documentation tests, and both release builds pass.
The preceding terrain-file protection fix also passed all 76 tests on Windows and
Ubuntu. See [terrain-guard-evidence.json](terrain-guard-evidence.json).

The budget-check revision passed all 76 tests in
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42061528743).
Both logs were checked for the expected test count and regression names.

## Fixed larger-budget Chicago follow-up

A separate November 1 Midway run uses the same recording, full free Chicago
reference day, original broad area, and unknown recording clock. Its only job
change is a limit of 100 million hypotheses. It uses the qualified `c29f646`
executable. The early check counted 63,003,202 hypotheses. The run is still
matching curves; it is not an accuracy pass. The original failed 50-million
attempt remains unchanged. See [native-budget-followup.json](native-budget-followup.json).

## Acoustic loudness and timing diagnostics

A further diagnostic compared measured sound power with the inverse-square
distance model for all candidates in two fixed Lake Hood reports. It used no
camera position or external recording clock. The evaluator first reproduced
78 saved native frequency curves, with a maximum difference below 0.000000001 Hz.
The loudness score moved the later recording's first choice from 82.787 m to
479.261 m away. The earlier recording's first choice stayed 45.749 m away.
The added score made one result worse, so it was not added to the app.

Another diagnostic allowed clock-rate drift in the existing broadband timing
fits. It preserved all 40 candidate associations in each of two new Midway
recordings. The first choices were 503.580 m and 312.343 m from the camera.
The first fit in the first recording did not converge. Neither result is an
accuracy pass. These diagnostics changed no production code or acceptance
limit. See [acoustic-envelope-diagnostics.json](acoustic-envelope-diagnostics.json).
