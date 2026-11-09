# Real-video validation

The acceptance benchmark remains incomplete. See [limitations](../docs/limitations.md) for current coverage and model constraints.

The current location target is within 1 km.
The original 250 m plans and results below remain unchanged.
New results must state which target they use.

## Current checks

[Corridor spacing checks](corridor-spacing-checks.json) fixed excess search
proposals from dense flight traces. Proposals now keep their metre spacing
along each continuous eligible path. The spacing follows turns and restarts
after gaps or unsuitable motion. Curve fitting keeps the original positions.
Revision `490cbef` passed all 156 Rust tests, 10 Python tests, and every required
check locally and in
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42549220092).
The record preserves an earlier CI failure from a newer Clippy rule in the
regression helper and the correction.

For the first November 1 Lake Hood clip, standard traces now need 15,277,836
proposals, down from 19,816,431. High-resolution traces need 15,274,007 and fit
the same 20-million-proposal budget. They previously exceeded that budget.
The paired searches continue offline with identical input jobs and no recording
date or start time argument. These counts cover search preparation.

The benchmark runner also records date arguments from the actual jobs and
lists imported reference files separately. Earlier direct-file diagnostics
could set the supplied-date flag when no date argument was present. Their
original records remain saved with this issue documented.

Both spacing builds repeated the original Maho recording offline with no
supplied date or time. Each remained within 549.65 m, including the reference
allowance. All report fields except creation time matched the preceding build.
The test proxy saw no connections, and source manifests stayed unchanged.

[Flight reference checks](flight-reference-fixes.json) found two lost-data
problems. Passage timing kept only one visit when an aircraft left the search
radius and returned. The trace reader also dropped the first recorded position
after a reception gap. Both are fixed. Slow and stationary positions keep a
visit connected. Reception gaps still split tracks.

Revision `598691d` passed all 153 Rust tests, nine Python tests, formatting,
strict Clippy, doc tests, and both release builds locally and in
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42541186372).
The location acceptance rules are unchanged. The record includes the failing
checks that led to the fixes and the upstream trace format evidence.

The [offline replay](flight-reference-replays.json) with that build completed
all 13 cases that use readsb flight traces. Maho remained within 550 m. All
seven Lake Hood cases remained unresolved. No wrong accepted result or
processing error occurred. Each job searched its prepared cache with empty
date fields and no recording start time. The test proxy saw no connections,
and the saved source manifests stayed unchanged.

The earlier passage-only build, `5ec1283`, completed all 27 fixed recordings.
Eight remained within 1 km, five were ambiguous, and fourteen had insufficient
evidence. All seven Lake Hood recordings remained unresolved. The same record
keeps both replays with their separate executable hashes and case selections.

A [free archive comparison](lake-free-archive-comparison.json) checked the
production and staging sources for all four prepared Lake Hood dates. All
16 archive parts matched their published sizes and hashes. Staging added
3,067 rounded position-seconds across the broad search area. Production held
more regional positions overall.

A [public-source recheck](free-history-recheck.json) found no November assets in
ADSBiq's published releases; its latest daily file was October 10. ADSB.fi's
documented open API lists aircraft queries and snapshots. One anonymous OpenSky
historical-track request returned 403 and no track. No further request was made.
These checks added no reference positions.

The [completed archive replay](lake-combined-reference-results.json) searched
all seven Lake Hood recordings against production data and the combined data.
All 14 runs finished offline: twelve reported insufficient evidence and two
remained ambiguous. No location was accepted. Each search used all four prepared
dates, the original broad area, and no recording date or time argument. Source
bytes and exported flight files stayed unchanged. The test proxy saw no connections.

[High-resolution sample checks](lake-high-resolution-reference.json) downloaded
both standard and high-resolution free traces for 570 aircraft on November 1.
Two aircraft were unavailable in both variants. The original broad search area
contained 64,701 standard airborne positions and 517,449 high-resolution
positions. The Rust importer retained 156,946 and 432,574 positions, respectively,
including its wider flight-data margin.

All seven fixed Lake Hood recordings were searched against each complete
sample reference, with no recording date or start time argument. All 14 runs
reported insufficient evidence. The runs used the original broad bounds and
unchanged sound detection and acceptance rules. They ran offline with no proxy
connections and unchanged reference files. This was a paired single-day source
diagnostic. The original multi-date benchmark is recorded separately.

A separate diagnostic supplied the known camera position and kept the clock
unknown. Within the default 500 m passage radius, both sources had fewer than
six reference passages. Checks at 1, 2, and 5 km found additional candidate
matches. These point checks did not test location acceptance. The record also
preserves three failed preparation or diagnostic attempts and their corrections.

[Broader-distance checks](lake-high-resolution-range-results.json) then searched
the two sample-covered recordings at 1 km and 2 km passage distances. All four
runs completed offline with the same sound peaks, original broad bounds, and
no recording date or time argument. Each ran all 19 shuffled controls and
reported insufficient evidence. The highest-ranked points were 2.87 to 7.07 km
from the reference, including its allowance. Control scores also remained higher
when both distance choices were evaluated together. Default settings are unchanged.

[Automatic date discovery](automatic-date-discovery.json) found 1,327 ADSB.lol
days, 125 first-of-month sample days, and 89 complete days for each airport
provider on November 7, 2026. Each saved index replayed offline with identical
output and no connection to the test proxy. These checks downloaded date
metadata. Flight files are checked when they are prepared.

The same build repeated three real audio searches offline. Maho passed at
549.65 m and Midway at 254.85 m. Lake Hood remained ambiguous. All report fields
except creation time matched the previous build. The full 89-day Chicago download
for a wider offline Midway test remains incomplete.

[Historical sample checks](empty-sample-date-validation.json) found and fixed
two failures. Some 2016 discovery snapshots have timestamps slightly more than
30 seconds after their filenames. The first fix accepted offsets below
one minute and kept the original flight-trace times. A verified day with zero
regional aircraft now stays in the coverage record while analysis continues
through later days.

All 288 snapshots from July 1, 2016 passed preparation and offline verification.
That day contained no aircraft in the Maho region. A real Maho search across
that day and the three prepared 2026 dates passed offline at 549.65 m. Its timing
scores, controls, and location match the earlier three-date result. The test
proxy received no connections. Code revision `d2366f4` passed all 139 Rust
tests, nine Python tests, formatting, strict Clippy, doc tests, and both release
builds. The record keeps the two failed attempts.

[Further archive checks](sample-archive-compatibility.json) recovered a missing
September 2016 snapshot from the preceding minute. The cache keeps the actual
filename and records the replacement. Two legacy flight traces used `hex`
identities and a time base at their first observation. Their downloaded bytes
matched independent source copies. Their timestamps stayed unchanged.

The full day passed offline verification. Maho then passed at 549.65 m across
six prepared dates, with no recording date or clock supplied. The local
build, `275b0f2`, passed 142 Rust tests, nine Python tests, and all required
release checks. An offline regression also checks the dates inside older cache
files whose stored checksums match.

[Snapshot boundary checks](snapshot-boundary-validation.json) recovered a
December 2016 snapshot timestamped exactly one minute after its filename.
The nine-date Maho replay passed offline at 549.65 m, with no recording date or
time supplied and no connections to the test proxy.

[Historical overlap checks](historical-trace-overlap.json) then found January
2017 snapshot delays up to 71.3 seconds. Seven traces also included positions
from the preceding evening. Build `1c948eb` records these offsets and overlaps,
preserves source bytes and times, and filters sample analysis to the selected
archive dates. It passed all 144 Rust tests, nine Python tests, formatting,
strict Clippy, doc tests, and both release builds.

The [ten-date offline replay](ten-date-offline-replay.json) verified the repaired
January archive and tested all three original Maho sample recordings. All 288
snapshots and 13 traces passed preparation. Seven traces retained 897 positions
from the preceding evening. The source hashes matched independent downloads.

All three audio cases completed with no recording date or time supplied. Maho B
passed at 549.65 m. Maho A remained ambiguous, and the continuous clip had
insufficient evidence. The runner returned 1 because two cases failed location
acceptance. No processing error occurred. The proxy received no connections,
and all source manifests stayed unchanged. These tests reuse existing recordings.

[Source-gap recovery](snapshot-source-gaps.json) then prepared March 2017 with
284 snapshots, four unavailable slots, two neighboring replacements, and six
traces. The provider index independently confirmed the 23-minute snapshot gap.
The cache records each unavailable slot. Offline preparation passed with no
proxy connection. All three Maho cases completed across twelve dates, again
with one accepted, one ambiguous, and one insufficient result. Maho B remained
549.65 m from its reference. Build `ac17fdd` passed all 145 Rust tests, nine
Python tests, and the required local checks. Wider source preparation continues;
the resumed Chicago plan covers 90 dates, including older prepared data.

A [channel audit](lake-channel-audit.json) checked all seven Lake Hood recordings
at the original 44.1 kHz rate and the app's 8 kHz rate. Across 6.25 hours, the two
channels were nearly identical. Difference-channel power was below -68.28 dB
relative to sum-channel power at the original rate. These measurements gave no
basis for a stereo-localization change. Lake Hood remains unresolved.

Revision `99588f3` passed [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42364660722)
with 142 Rust tests and nine Python tests on each platform.

Revision `0dccdd9` passed [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42361118775)
with 139 Rust tests on each platform. Its downloaded Windows executable repeated
the earlier four-date Maho result. All report fields except creation time
matched the local build.

The wider preparation plans cover 89 Chicago days and 125 free sample dates.
The first newly downloaded Chicago day contains 634,783 positions across all
2,880 intervals. It passed offline verification with unchanged files and no
proxy connections. The full-window location tests remain pending.

The [unknown-date audio run](unknown-date-results.json) tested 27 recordings
offline with no recording date or time supplied. Each case kept its broad search
region and used all dates in its prepared provider cache. Eight recordings
returned locations within 1 km. Five were ambiguous and 14 had insufficient
evidence. No accepted result exceeded 1 km. All seven Lake Hood recordings
failed location acceptance. The [27-case manifest](unknown-date-cases.json)
preserves the inputs and reference positions used only by the evaluator.

The Maho sample cache initially contained one date. A
[separate expanded check](unknown-date-maho.json) prepared September 1,
October 1, and November 1 from the free sample source. The same three recordings
were searched offline without dates or times. One passed at 549.65 m, including
the existing 200 m microphone allowance. One was ambiguous and one had
insufficient evidence. The Windows CI executable reproduced the passing result.

[Optional video text tests](optional-video-clues.json) used seven low-resolution
Lake Hood files and two separate five-second 360p clips. The two 360p clips
matched the visible name **Lake Hood Seaplane Base** to a GeoNames point 31.06 m
from the camera reference. Both reports also kept a broad Anchorage metro-area
clue. The seven low-resolution files returned no place matches. Earlier false
matches and processing errors remain in the record. These are development
tests of named-place clues. The clips were inspected before the OCR work.

Code revision `eee5b0c` passed 130 Rust tests, nine Python tests,
formatting, strict Clippy, documentation tests, and both release builds.
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42351026350)
also passed. The downloaded Windows artifact produced the same Lake Hood text
report as the local build. [Three audio replays](optional-video-audio-regression.json)
kept the same peaks, scores, controls, regions, and status after the optional
video path was added. The 27-case run used `dcd73cc`; its executable and report
hashes remain in the record.

[Compression and duration checks](unknown-date-audio-variants.json) used six
copies of three existing recordings. Full-length mono Opus audio at 16 kbit/s
passed for Maho at 549.65 m and Midway at 384.25 m. Compressed Lake Hood remained
ambiguous. The first 15 minutes of each recording returned either an ambiguous
result or insufficient evidence. All six runs used offline data with no supplied
dates or times. No accepted result exceeded 1 km. These copies share their source
recordings with the original benchmark.

A [playback-speed diagnostic](unknown-date-playback-rates.json) searched five
fixed rates for Lake Hood, LAX, and Midway. The unchanged rate had the highest
score in all three cases. The alternative rates did not improve the selected
Lake Hood or LAX result. These searches had no shuffled-control assessment.
The app's timing model remains unchanged.

The documentation revision `ce5e828` also passed
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42353809388).

To repeat the 27-case run with retained media and prepared data:

```powershell
python validation/run_timing.py --cli target/release/soundgeo-cli.exe --manifest validation/unknown-date-cases.json --data-root .. --output ../unknown-date-check --unknown-date --offline
```

## Earlier checks and runner use

Local artifact paths in published records are relative to the validation data folder.
Original artifact hashes are retained. Where paths were normalized in a published
record, its bytes can differ from the original retained artifact.

To repeat the current 1 km timing benchmark, run this command from the repository:

```powershell
python validation/run_timing.py --cli target/release/soundgeo-cli.exe --data-root .. --output ../timing-verification
```

The [timing manifest](timing-cases.json) lists 25 preserved recordings and their
file hashes. `--data-root` must contain the original local media and reference
folders. The media is not redistributed. Use `--case maho-nov1-b` to run one
case. Use a new output folder for each run. The default time limit is one hour
per case, including downloads; `--timeout` changes this limit in seconds.

The runner uses the broad search areas and possible dates from the fixed cases.
It does not give the app a recording clock or a reference position. It checks
the returned area center against the reference only after the app exits.
The Maho check includes the planned 200 m allowance for the unsurveyed microphone.
It returns code 0 only if every selected recording passes. An ambiguous,
insufficient, wrong, or failed result returns code 1. Existing failed recordings
remain in the manifest. This benchmark is expected to fail until they pass.

To test without a recording date or time, add `--unknown-date`. This searches
all available dates for the selected provider. Add `--offline` to disable network
access. Imported day-specific flight files require a prepared provider cache:

```powershell
python validation/run_timing.py --cli target/release/soundgeo-cli.exe --data-root .. --output ../lake-date-search --case lake-hood --case lake-hood-nov1 --unknown-date --offline --cache ../real-validation/flight-cache --provider adsb_lol
```

Repeat `--case` to select more recordings. Use `--manifest` for a separately
prepared set of cases. The runner records the executable, manifest, and runner
hashes. It checks that date-free reports contain no supplied archive date or
day-specific import and that offline reports used offline mode. References
enter only the evaluator after the app exits.
The nine fast Python tests in CI check the runner's safeguards. They do not run
the real media benchmark or establish location accuracy.

The completed fixed-build benchmark has nine reports and one search-limit error.
Two first choices are within 250 m. Both remain ambiguous. Acceptance is **0/10**.
The failure does not inherit an older successful coordinate result. See
[qualified-batch-results.json](qualified-batch-results.json).

A separate experimental search improved the later Lake Hood first choice to
82.787 m. It also found a five-match alternative 479.261 m away. It remains
ambiguous. That fixed build failed on both preselected Lake Hood recordings.
Their first choices were 8.56 km and 9.38 km from the camera.
Both also fail the revised 1 km target. The preceding main revision passed 76
software tests locally and in Windows and Ubuntu CI. This does not pass the
real-video benchmark.

Ten recordings from three public cameras were captured locally on 2026-11-04.
The known UTC reference day and an approximately 40 km regional search box are
provided; the recording clock, camera coordinates, aircraft identities, tone
bands and flyover intervals are not supplied to the application. Every solver
job has `start_utc: null`. The search covers predefined dates and regions.

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
  and is not redistributed in this repository.
- Stream timestamps are not independently accurate recording clocks: one
  Midway visible clock differed from the manifest-derived timestamp by about
  50 seconds. The inferred recording clocks remain unverified.
- `midway-holdout` was first evaluated after the short-pass detector was developed
  on the other recordings. It is a single same-site holdout. Later development runs reuse that recording.

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
windows had only about 7â€“68% modeled flight coverage at the camera. Several
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
unavailable.

## Reproduction and acceptance

`run.py` requires Python 3.11+ and the original local captures. It verifies media
hashes, constructs solver jobs without clock/ground-truth fields, downloads or
reuses full-day flight data, and evaluates coordinates only after the solver has
finished. It exits 1 if any case lacks an unambiguous top result within 250 m.
This threshold defines the evaluation.

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

Software checks: all 25 deterministic tests, strict Clippy, formatting and release compilation pass locally on Windows. Desktop checks covered automatic-download/date-range controls, manual import, partial-match counts, and correct event-to-curve association in a clearly labelled synthetic UI fixture.

## Free November 1 reference data

The additional three recordings are two one-hour Midway clips and one one-hour
Southampton clip. These tests use free sources only. ADS-B Exchange publishes
a [free first-of-month sample](https://www.adsbexchange.com/data-products/sample-data/)
and [public demo S3 access](https://www.adsbexchange.com/pull-data/). The documented
sample interface worked without a private account. No paid data was used.

All 288 worldwide five-minute snapshots for November 1 were downloaded. Aircraft
were discovered throughout the entire UTC day in broad regions (Midway:
41.00â€“42.66, -88.9â€“-86.6; Southampton: 50.15â€“51.81, -2.6â€“-0.08), followed by their
full-day traces. An audit preserving non-ICAO `~` prefixes found 4,483 identifiers:
4,466 trace files were available and 17 returned `NoSuchKey`. There were no extra
or substituted identities. Flights entirely between discovery snapshots may
still be absent. Reference coverage is therefore not claimed complete.

The complete cache is a root folder containing `traces/` and `supplement/`; supply both directories. Pass this root as `--sample-cache`
to `run.py` when reproducing November 1 cases. The application imported these
readsb files. At that revision, the integrated downloader retrieved ADSB.lol.
This sample download was a separate validation operation. The native sample
downloader was added later; see the final section below. Provider
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
2.2 Ã— 2.5 km region returned an ambiguous candidate 88.2 m from the Midway camera,
matching three of 17 detections. The recording clock was absent, but the small
region was chosen using ground truth. This known-area diagnostic is excluded from the acceptance count. Only the original broad-region
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
`3707cee64e28e0ffcbf90aa69685842d81ca4791`. These checks validate software behavior.

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
(61.179633, -149.9719665). A full hour from November 3 was evaluated;
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
That diagnostic used the known location and clock. It is excluded from acceptance results. A regression based on independently synthesized emission-phase audio
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
non-JSON content. The probes ended without usable data.

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
its combined latest-case totals span several executables.

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
for the same aircraft within 30 seconds. This comparison checks agreement between the two position sources.

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
remain unsuccessful. Acceptance and release readiness remain unmet.

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
They helped identify detector and search failures.

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
This totals two hours of noise.
The control summaries, input hashes, and diagnostic limitations are in [observation-controls.json](observation-controls.json).
To reproduce a full control, use `python validation/generate_noise_fixture.py --seed 731 --full-control --output control.wav`.
The selected 30-second regression and the full seed-731 WAV both reproduce their recorded hashes.

All 56 release software tests and required local checks pass on `e1aa6bc03295aca469aa35667e49393fa5c743fb`.
[Windows and Ubuntu CI also pass](https://github.com/Donovoi/soundgeo/actions/runs/41940895711).
The original ten broad-area cases are running on that executable.
The older 52-test benchmark stopped after eight completed cases; two slow Midway runs were cancelled to free CPU time.
The old, unindexed main-Midway known-area diagnostic was also cancelled and replaced by a run on the current executable.
Cancelled runs are incomplete. Their logs and metadata remain available. They are excluded from completed test counts.

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
This timing sample was taken while other searches were active.
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
unknown in both runs. This run produced a false match.

A fitted rate supported by only three sounds now keeps an ambiguity flag.
Three sounds are the minimum fitting consensus. Additional independent support
is required before that flag can be removed. This guard preserves the location estimate.
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
start time. The search failed before producing a location. Its source and
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
these two runs. Other searches were active during both runs. See
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
with zero accepted unambiguous results. These results span several development revisions. The complete result and hashes
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

This diagnostic uses fixed observation variants. Sparse local
sampling cannot bound the full uncertainty region. The app can also select alternative observations. The cause remains unresolved. Evidence is in [lake-hood-sensitivity.json](lake-hood-sensitivity.json).
The diagnostic can be reproduced with `cargo run --release --example profile_candidate -- report.json sensitivity.json`
when the report's original reference data is available.

## Completed broad Chicago retry

The November 3 `midway-arrivals` retry on `7f95301` finished in 3,393.53 seconds.
It used the full free Chicago airport archive, original bounds, and no recording
start time. The explicit resource limit was 50 million hypotheses; the failed
20-million run remains recorded. Its first candidate is 235.572 m from the
camera with five matches from six detected groups. The next candidate has four
matches. The result remains ambiguous and does not pass full acceptance.

This run gives another coordinate hit.
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
across ten recordings. These results span several revisions.

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
also passed.

The full Midway, Midway holdout, and November 1 Southampton controls returned
exactly the same candidates, status, detections, and hypothesis counts as their
preceding runs. The two Midway controls returned no candidates. Southampton
still returned the wrong ambiguous candidate 7.6 km away. The known-area
Chicago corridor comparison also preserved its results exactly. These regression checks reuse the existing cases.

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
to about 0.68%. This diagnostic used a seeded location.

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
Other searches were active during the run. See
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
Other searches were active during the runs.
See [clock-cache-comparison.json](clock-cache-comparison.json).

Revision `e947120` also passes
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/41979243404).
The November 4 Chicago download completed with 575,365 positions. All 2,880
requested parts were saved and parsed; 43 parts were not marked online by the
source.
The two preselected recordings are running on the planned `2988d77` build.

Further Lake Hood diagnostics use the prior known-area candidate as an explicit
hint. With the current model, its five flights still fit the unchanged sounds
at 0.8233% joint error. Actual corridor generation for three of those aircraft
produced a proposal about 30 m from that prior app estimate. Profiling nearby
proposals gave at most four valid matches. Passing those 363 nearby proposals
to the unchanged final refinement step recovered three five-match candidates.

This supports a proposal-selection problem. The pruning stage responsible for the full-search failure remains unresolved. The latest build must complete the broad
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
[airport-source-diagnostics.json](airport-source-diagnostics.json). This checks the downloader and parser.

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
Other searches were active. See [arrival-cache-evidence.json](arrival-cache-evidence.json).

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
is **0/2**, with no coordinate hits.

| Recording | Detected events | Hypotheses | Runtime | Candidates |
| --- | ---: | ---: | ---: | ---: |
| midway-nov4-a | 9 | 19,227,476 | 6,389.21 s | 0 |
| midway-nov4-b | 4 | 7,964,629 | 710.46 s | 0 |

These are new recordings from the existing camera.
Their plan, source files, dates, bounds and executable were fixed before
evaluation. The second run finished before the combined results were reviewed.
See [fresh-cases-results.json](fresh-cases-results.json). These failed results
do not replace any of the original ten cases.

## First result from the consolidated benchmark

The Midway November 1 holdout finished on `a25bd0d` with the original broad
area, complete regional reference, no recording clock and an explicit
50-million-hypothesis limit. Its first candidate is **28.393 m** from the
camera. All five returned candidates have three supporting sounds; the app
still reports `ambiguous`. The result failed acceptance because it remained ambiguous.

The run examined 23,596,939 hypotheses and took 6,586.9 seconds while other
searches were active. Four of the original ten cases have now completed on
this revision. The latest mixed-revision record has four coordinate hits and
zero accepted results. The remaining consolidated runs continue unchanged.
See [results.json](results.json) for the executable hash, report hash and
preserved earlier failures.

## Further checks on the November 4 recordings

The two Midway audio channels are almost identical. Mixing them to mono does
not cancel the sound. Checks also found no source-offline flags during either
recording.

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
Both results remain ambiguous. The history preserves both results. Two separate comparisons now use the current main
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
inputs. This experiment runs in a separate Python script.

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
candidate for Southampton. This change improves reuse of calculations. The running benchmark keeps its original
executable. See [segment-cache-evidence.json](segment-cache-evidence.json).

## Fitting all accepted curves together

A further experiment refined every accepted curve together, while keeping
the original observations, flight assignments and acceptance checks. It kept
the original candidates as alternatives. This experimental code is on a separate branch at `6ce0753`.

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
The report remains `ambiguous`. The result failed acceptance because it remained ambiguous.

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
reached it. These comparisons use unlabelled audio. Classification accuracy remains unmeasured.

The classifier could remove useful quiet observations. It was not added to
the app. No detection, ranking or acceptance rule changed. See
[audio-classifier-ast-diagnostic.json](audio-classifier-ast-diagnostic.json).

## Command-line rejection of ambiguous results

Revision `2a0952e` adds `--require-unambiguous`. It saves the report before
returning code 3 for no candidates or code 4 for an ambiguous result.
The existing `--require-match` behavior remains available. Scripts can use this option to check the report status.

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
coordinates or an external recording clock. The checks evaluate local refinements. Both versions used the same events and data.

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

The app uses 15 Â°C unless another value is supplied. A separate diagnostic
downloaded free routine weather reports from the Iowa Environmental Mesonet.
It used all stations within each original broad area and the whole UTC day.
It calculated hourly station medians, daily station medians, and a median
across stations. It used no camera position or recording clock to select weather.

The Lake Hood reference was 1.389 Â°C. With that temperature, its local first
choice moved from 108.563 m to 233.326 m. Its next choice was 607.289 m away.
The Chicago reference was 17.778 Â°C. Its first choice moved from 70.411 m to
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
The failed run remains part of the test set. The earlier 83 m improvement was limited to one case. See [fresh-lake-cases-results.json](fresh-lake-cases-results.json).

## Further checks on the failed November 4 Lake Hood recording

A diagnostic searched a small area around the known camera position, without
its recording clock. It used the same frozen experimental build and detected
sounds as the failed broad-area run. It returned no candidates. This diagnostic uses the answer position and is excluded from acceptance tests. See
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
These checks compare software outputs. See
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
executable. The early check counted 63,003,202 hypotheses. The process stopped
without a report. Its last log entry counted 61,820,420 hypotheses. The cause
and exit code are unknown. A separate retry uses the same inputs and binary.
Its files are kept separate. The original failed 50-million attempt remains
unchanged. See [native-budget-followup.json](native-budget-followup.json).

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

## Passage timing with a chance-match penalty

A new method uses broadband sound peaks and nearby flight passages. It fits
one recording start time for all peaks. It compares a flight-plus-background
model with a constant background model. Busy flight routes therefore do not
receive a high score only because they contain many possible matches.

The complete original Midway area was searched at 300 m spacing. Each of two
development recordings used 18,090 positions and the full free reference day.
No camera position or recording time entered the search. The first locations
were 546.45 m and 254.85 m from the camera. Other locations remain in the report.

Six fixed controls shuffled the sounds in 120-second blocks. Their strongest
scores ranged from 10.565 to 13.077. The real recordings scored 35.143 and
42.352. Six controls cannot establish a reliable false-positive rate.

The Rust implementation reproduced all 2,048 saved candidate results at the
recorded precision. It also reproduced all 165 detected audio peaks. The
numerical likelihood fit was checked against an independent optimizer on 320
cases. Ten focused tests cover detection, matching, geometry, numerical fits,
input limits, and a generated search with an unknown clock. These checks do
not qualify real location accuracy. Two further same-camera clips were reserved
before the native search was tested. They will be evaluated after the native
implementation is qualified and frozen. Their completed results follow below. See
[passage-timing-development.json](passage-timing-development.json).

## Frozen passage-timing results

Revision `72d4658` passed all 86 local tests and all required checks. Both Windows
and Ubuntu CI also passed 86 tests. The frozen executable reproduced the leading
candidate in each of the eight development/control cases. See
[passage-timing-frozen-ci.json](passage-timing-frozen-ci.json) and
[passage-timing-frozen-parity.json](passage-timing-frozen-parity.json).

The two reserved recordings, C and D, both ranked a point 546.45 m from the camera
first. Each score exceeded all 19 shuffled controls. Their scores were 49.074
and 40.215. The strongest control scores were 14.626 and 16.399. These are new
recordings from the same camera. They are not tests at a new location. The
one-hour container duration differed from decoded audio by 0.001375 seconds.
The test harness was corrected to use the decoded duration for the controls.
The frozen program and the completed first report were preserved. See
[passage-timing-heldout-results.json](passage-timing-heldout-results.json).

The same frozen method then tested all ten original recordings and both fresh
Lake Hood clips. Three original Midway recordings ranked within 1 km. The other
nine cases failed that coordinate target. The two fresh Lake Hood errors were
2.33 km and 10.23 km. A strong-looking timing score can also cover several
locations. The raw ranking is not sufficient to claim a location. See
[passage-timing-existing-cases.json](passage-timing-existing-cases.json).

## Main CLI and desktop integration

The new mode is available as `analyze --method passage-timing` and through the
desktop method selector. Older jobs keep their Doppler method. Timing reports
use a separate `timing` field. The normal CLI status and strict exit codes also
apply to this mode.

The development screens require six matched peaks and a score above all 19
distinct shuffled controls. They also check competing locations more than 1 km
away. All evaluated grid positions retain their best score, even when nearby
candidates fill the detailed report. A likelihood factor of 100 defines the
competitive score range. The screening rule compares candidate scores. See [passage-timing-screen-spec.json](passage-timing-screen-spec.json).

All 91 local software tests and required checks pass. Tests cover the unknown
clock, one-to-one matching, numerical fits, distinct controls, retained spatial
alternatives, CLI status, old job files, and avoiding flight downloads for silent
audio. Revision `f8f1735` also passed all 91 tests in Windows and Ubuntu CI.
The CI logs were checked for the expected test names, with no failed or ignored
tests. Two further clips, E and F, were reserved before capture and evaluated
after the main CLI was qualified and frozen. See
[passage-timing-integrated-local-checks.json](passage-timing-integrated-local-checks.json)
and [passage-timing-integrated-ci.json](passage-timing-integrated-ci.json).

## Integrated CLI results within 1 km

The fixed 18-case plan is complete. Every case used the same frozen main CLI,
the original broad search area, and no recording clock. Midway used the full
free Chicago airport reference day. Lake Hood and Southampton kept their
original free references. The possible date was known. This does not test
worldwide search or an unrestricted recording date.

| Outcome | Cases |
| --- | ---: |
| Screened location within 1 km | 6 |
| Ambiguous | 3 |
| Insufficient evidence | 9 |
| Wrong screened location | 0 |

The six returned locations are all from Midway. Their errors range from 98.83 m
to 546.45 m. Only E and F were prospective tests of the new screens. E returned
a location 98.83 m from the camera. F ranked a point 384.25 m away first, but
reported competing locations and returned strict exit code 4. It failed acceptance. All other cases were already development recordings.

All five Lake Hood recordings and both Southampton recordings returned
insufficient evidence. Their diagnostic coordinates remain in the reports but
do not count as location matches. The
returned-location coverage is 6 of 18 recordings. No calibrated confidence or
accuracy radius is claimed. See
[passage-timing-integrated-results.json](passage-timing-integrated-results.json)
for every case, its inputs, report hash, status, and error.

## Further timing diagnostics

Two lower-frequency sound bands were tested on both November 1 Lake Hood
recordings. None of the four first choices was within 1 km. These were raw
development searches without shuffled controls. The app's filter was not
changed. See [lake-timing-band-diagnostic.json](lake-timing-band-diagnostic.json).

Wider passage ranges of 1 km and 2 km were tested on two Lake Hood clips.
All four runs returned insufficient evidence. Three failed the shuffled-control
comparison. The fourth had fewer than six matched peaks. The default range was
not changed. See [lake-timing-range-diagnostic.json](lake-timing-range-diagnostic.json).

Two public O'Hare clips returned insufficient evidence with strict exit code 3.
Their source includes air traffic control audio. The exact microphone location remains unverified. These clips are used for source diagnostics. See [ohare-timing-diagnostic.json](ohare-timing-diagnostic.json).

Two LAX clips were selected before solver evaluation. Their plan uses a full
free LAX radar day, broad bounds, and no recording clock. The publisher and
airport sources identify the north-side parking garage at building level.
The reference is inferred from the building location.
The planned evaluator checks the distance to the whole building footprint.
The two clips are now prepared, but their source has unresolved limitations.
The first visible clock advances about 65 minutes in 60 minutes of video.
The second clip includes a cutaway to another camera. Microphone continuity is
not verified. Both clips remain planned diagnostics; neither can count as a
clean accuracy test without further source evidence. The full free reference
day is now downloaded and verified. See
[lax-timing-prospective-plan.json](lax-timing-prospective-plan.json) and
[lax-source-quality-notes.json](lax-source-quality-notes.json).

Two further LAX clips use the publisher's fixed cameras at the H Hotel.
They were selected before capture and analysis. The hotel address and map
identify a building footprint about 105 m across. The microphone position is
not surveyed. The stream's transport clock differs from the clock shown in
the picture. The source includes radio speech. These clips remain source
diagnostics until the recording date and local microphone are verified.
See [lax-fixed-camera-plan.json](lax-fixed-camera-plan.json).

## Search-cache verification

Revision `38ff00e` passed all 92 software tests on Windows and Ubuntu.
The cached search reproduced both the Midway A report and the Lake Hood
2 km range diagnostic exactly. It preserved every compared search result,
control score, warning, and status. The observed times were 73 s and 792 s,
compared with 96 s and 841 s in the earlier runs. Host load differed between the runs. No accuracy rule changed.
See [timing-geometry-verification.json](timing-geometry-verification.json).

## LAX download and completed diagnostics

Revision `4681a84` adds automatic free LAX downloads to the CLI and desktop app.
All 93 tests pass locally, on Windows, and on Ubuntu. The first Ubuntu attempt
was cancelled during slow system-package downloads. The retry passed on the
same source revision. See [lax-provider-ci.json](lax-provider-ci.json).

The live download check removed one planned part from a separate cache.
The CLI downloaded that part and rebuilt the full UTC day. All 996,783 positions
matched the independent reader exactly, including converted altitude. The next
CLI call reused the verified files. The server changed its generation timestamp
and row order between requests. Two overly strict comparison attempts were
retained before the full field comparison passed. See
[lax-provider-live-verification.json](lax-provider-live-verification.json).

All four one-hour LAX runs are complete. Two were ambiguous and two returned
insufficient evidence. Broadcast B ranked a point within 724 m of every garage
vertex, but competing locations were over 23 km away. Neither fixed-camera clip
found a useful location. Both source diagnostics failed acceptance.
See [lax-timing-diagnostics.json](lax-timing-diagnostics.json).

A separate two-hour fixed-camera clip used the native LAX provider with no
recording clock. It also returned ambiguity. Its first point was about 1.90 km
from the farthest hotel vertex. The visible source clock advanced about
126 minutes during 120 minutes of video; the cause is unverified. See
[lax-extended-timing-diagnostic.json](lax-extended-timing-diagnostic.json).

A higher-frequency band was tested on all four LAX clips, one Midway clip, and
one Lake Hood clip. It did not fix the wider failures. This was a raw-ranking
diagnostic without shuffled controls. The production filter is unchanged.
See [high-band-timing-diagnostic.json](high-band-timing-diagnostic.json).

The Doppler detector found nine curves in broadcast B and 21 in fixed B.
Those curves were compared with the retained timing proposals from both sound
bands. Each test used one shared clock adjustment and separate flights and
sounds. No tested proposal had three supporting curves. This finite diagnostic
does not rule out other ways to combine the two methods. The app was not changed.
See [lax-timing-curve-diagnostic.json](lax-timing-curve-diagnostic.json).

## LAX flight distance and source continuity

A diagnostic used the known hotel building center to check flight coverage.
No reference flight passed within 500 m at the previous 38 m ground setting.
There were 1,085 passages within 1 km. This explains a structural exclusion
in the earlier hotel searches. The building coordinate was used for this
diagnostic only. It was not supplied to the broad searches.

Four main CLI runs then used the original broad area, unknown clock, and
aircraft distances of 1 km and 2 km. All four failed the 19 shuffled controls.
One first point was within 569 m of the hotel footprint. Its inferred clock
also agreed with the visible clock, but its sound score was too weak to accept.

An attempted two-hour extension showed a six-minute jump in the visible clock.
Its container timestamps also became irregular. That full capture was saved
and was not analyzed. A different two-hour interval had regular timestamps
and five visible clocks that advanced with the video. It scored 20.60 against
a maximum shuffled score of 15.75. It still returned ambiguity, with its first
point about 1.80 km from the hotel footprint. See
[lax-range-and-continuity.json](lax-range-and-continuity.json).

Wider sound peaks and a fixed speech-exclusion rule did not establish another
usable location. The speech model removed 64 of 79 peaks in hotel A and 62 of
80 in hotel B. Their new first points were still wrong. It kept all 69 peaks
in the Midway control. The production detector is unchanged. See
[lax-audio-source-followups.json](lax-audio-source-followups.json).

The research command now reads the timing options from the job. Revision
`71d05e3` passed all 93 local, Windows CI, and Ubuntu CI tests. Default-option
research output kept all 256 Midway position/clock choices in the same order.
Reloading the peaks from JSON produced score differences below 1e-12; the
complete reports were not bit-identical. See
[timing-research-options-checks.json](timing-research-options-checks.json).

## Candidate regions within 1 km

The earlier spatial check measured every competitive point from the leading
candidate. That can reject a compact region when the leading point is near
its edge. The new check first keeps the existing center when it passes.
Otherwise, it checks the midpoint of the competitive latitude and longitude
bounds. Every competitive point must be within 1 km of the reported center.
This is a conservative enclosing-region check. It does not claim to find the
smallest possible circle or to give a calibrated location error bound.

The sound scores, six-match minimum, and all 19 shuffled controls are unchanged.
The report adds `timing.region` after the checks pass. Individual candidates
remain in the report. The desktop app shows the area center and has a separate
button to copy it. Old reports do not gain a region when they are loaded.

All 97 tests pass locally, on Windows CI, and on Ubuntu CI, with no ignored
tests. Formatting, strict Clippy, documentation tests, and both release builds
pass. The four added tests check an edge-leading candidate, the date line,
invalid or wide regions, and older report loading. See
[timing-region-local-checks.json](timing-region-local-checks.json) and
[timing-region-ci.json](timing-region-ci.json).

The first Windows run failed because the FFmpeg package server returned HTTP
503. The installer reported success without installing FFmpeg. Revision
`81e6a38` checks the executable and retries a failed install. It passed all
required checks. The application source and saved binaries did not change.
The failed run and four setup checks remain in
[ci-ffmpeg-setup-checks.json](ci-ffmpeg-setup-checks.json).

The saved `d6e9418` CLI replayed all 18 original benchmark recordings and the
continuous LAX development recording. Every search result, sound peak, and
shuffled score matched the earlier report. The new region check added one
Midway result. Its center was 89 m from the reference, and all 17 competitive
grid points were within 959 m of that center. The 18-case totals are seven
locations within 1 km, two ambiguous results, and nine insufficient results.
No screened location was outside 1 km. All seven successes came from the same
Midway camera. The extra LAX case stayed ambiguous. These are repeated development tests. See
[timing-region-results.json](timing-region-results.json).

Two recordings from November 4 were reserved before the continuous November 3
follow-up finished. Their intervals were selected using source timestamps,
without location-search results. Five visible clocks and all fragment times
were checked in each recording. They used the original frozen `4681a84`
build, the same broad area, a 1 km aircraft distance, and no recording clock.
The complete free flight download saved 1,051,893 positions. All 2,880 parts
were online. The manifest, CSV hash, row count, and cached CLI reuse passed
checks. A separate Python receipt used the wrong manifest name and failed
after the app completed; that failure is preserved. See
[the download checks](lax-next-date-download.json).

Both analyses finished with all 19 shuffled controls. Recording A reported
insufficient evidence. Its first point was 1.63 km from the farthest hotel
vertex, and its score did not exceed the controls. Recording B reported
ambiguity. Its first point was 25.69 km away, with competitive points more
than 35 km apart. Neither recording returned an accepted location. The new
region-center check cannot remove either failure. See [the plan](lax-next-date-plan.json),
[the prepared recordings](lax-next-date-prepared.json), and
[the results](lax-next-date-results.json).

Twelve fixed audio windows checked whether the stereo channels could separate
radio speech from aircraft sound. In eleven windows, the channel difference
was more than 70 dB below the shared signal. The remaining window was 27 dB
below it. The sampled channels were almost identical. This does not support
useful separation by subtracting the center channel.

## Further sound checks

The longer continuous LAX recording had 24 detected Doppler events. None of
its 256 timing proposals had three supporting curves. Summing location scores
from two recordings at the same camera also left LAX and Lake Hood unresolved.
Neither test changed the app.

The free LAX replay also contains readings from 25 noise monitors. A full-day
search compared their level changes with five recordings. Two exceeded all
19 shuffled controls. Their preferred clock changed with the monitor's
position along the flight path. That clock cannot be treated as the camera's
clock. A follow-up timing search using the inferred clock did not produce a
qualifying 1 km region. See
[additional-audio-diagnostics.json](additional-audio-diagnostics.json).

A lower sound band, from 80 to 250 Hz, was checked in nine recordings with
two sound-peak width limits. One LAX result was 559 m from the hotel. Another
gave a stronger score and a compact region 3.57 km from the hotel. A successful
Midway case also became wrong. These research searches did not run the shuffled
controls. Neither change was added to the app.

In the closer LAX result, 33 of 34 matched aircraft segments were less than
30 m above the model's ground height. Most accelerated. These heights are computed from the flight and terrain models. The wrong result had only three such
segments. Removing low segments does not resolve both failures. See
[low-band-diagnostics.json](low-band-diagnostics.json).

The older, separate Midway Doppler retry also finished. It evaluated
63,003,202 hypotheses and ranked a point 178 m from the reference first.
Competing points were about 25 km away, so the result remained ambiguous.
The earlier interrupted attempt remains saved. See
[native-budget-followup.json](native-budget-followup.json).

## Maho Beach: a second successful site

The camera owner places this stream at Tortuga Maho Beach Cafe. The local
tourism association documents its ambient sound. The host's own map link gives
the reference pin. The plan adds a 200 m allowance because the microphone was
not surveyed. The app did not receive that pin or the recording clock.

Two fixed two-hour recordings from November 1 were selected before either
location result. All fragment times and five visible clocks passed checks in
each recording. The free ADS-B Exchange sample supplied 97 full-day aircraft
traces. All 288 five-minute discovery snapshots were checked in a broad region.
The native reader retained 336 segments and 22,688 positions. Short flights
between snapshots and gaps in receiver coverage can still be absent.

The unchanged `d6e9418` CLI returned a location for recording B. Its point was
350 m from the reference pin, or 550 m with the planned allowance. It matched
15 flights and exceeded all 19 shuffled controls. Recording A ranked the same
point first, but kept competing locations about 3.8 km apart. It failed acceptance.

The November 4 free archive had only 145 regional positions, all above 10 km.
Recording B passed source checks, but the app reported insufficient evidence.
Recording A failed the fixed continuity rule. Its visible clocks also gained
about 32 seconds relative to media time. It was preserved and not analyzed.

A separate 200 m grid check kept November 1 B within 1 km, but A failed its
shuffled controls. The Doppler method left both recordings ambiguous. An
enclosing-circle diagnostic also failed to resolve earlier spatial ambiguity.
These checks did not change the production detector or spatial thresholds.
See [maho-results.json](maho-results.json).

Two more fixed same-camera intervals were selected afterward. Both failed the
original fragment-timing rule. They remain rejected. A separate plan then used
only media timestamps to select each interval's longest continuous run, with
two minutes removed from each end. It did not join sections or use a location
result to choose them.

The resulting 41-minute section passed all fragment checks and five visible
clock checks. The frozen `67482a4` CLI used automatic sample downloads and no
recording clock. It reported insufficient evidence after all 19 shuffled
controls. The other section had a five-second jump in its visible clock and
was rejected before analysis. Neither follow-up adds a successful location.
See [maho-continuity-followups.json](maho-continuity-followups.json).

The Linux package download at `3373361` was stopped after more than 26 minutes.
Revision `6ba1a72` uses Ubuntu's public mirror and bounds installation waits.
All 97 tests and required checks then passed on Windows and Ubuntu. See
[ci-linux-setup-checks.json](ci-linux-setup-checks.json) and
[ci-linux-setup-result.json](ci-linux-setup-result.json).

## Native free sample download

The Rust CLI and desktop app now offer the documented ADS-B Exchange free
evaluation sample. It accepts first-of-month dates and requires search bounds.
It checks all 288 discovery snapshots and downloads the selected full-day traces.
It preserves non-ICAO identifiers, checks source dates and identities, verifies
cached file hashes, and saves rejected cache bytes before repair. Missing traces
are recorded. A missing discovery snapshot stops the operation. No private
account or paid bucket is used.

All 105 local tests, formatting, strict Clippy, documentation tests, and both
release builds pass. The new tests cover public-request signing, cache reuse
and repair, missing data, cancellation, and CLI validation. All 105 tests and
required checks also passed in Windows and Ubuntu CI at `67482a4`. See
[sample-provider-local-checks.json](sample-provider-local-checks.json) and
[sample-provider-ci.json](sample-provider-ci.json).

A fresh native CLI download saved all 288 discovery snapshots and 97 Maho
traces. All 385 source files matched the independent downloads byte for byte.
Complete cache reuse passed. One trace was moved aside in this new validation
cache, and the CLI restored the same bytes. The withheld file remains saved.

Automatic-download replays of the original November 1 Maho recordings kept
every sound peak, search result, control score, and region unchanged. A stayed
ambiguous. B stayed within 550 m, including the planned reference allowance.
These checks prove download and replay behavior on those inputs. They do not
add independent location tests. See
[sample-provider-live-checks.json](sample-provider-live-checks.json).

## Short flight segments and the completed 25-case replay

Revision `80d33f2` keeps three to seven recorded positions for passage timing.
Doppler matching and the general import API still require eight. The importer
keeps its duplicate, two-second thinning, gap, and speed checks. It does not
invent positions. The new tests check these boundaries and recover an unknown
clock after importing three-position flights. All 108 Rust tests and all five
required checks passed locally, on Windows, and on Ubuntu. See
[short-track-local-checks.json](short-track-local-checks.json) and
[short-track-ci.json](short-track-ci.json).

All 25 fixed replays completed. Eight returned locations within 1 km, five
remained ambiguous, and twelve had insufficient evidence. No previous success
was lost, and no wrong screened result was returned. The successful Maho result
now matches 16 distinct aircraft instead of 15. The added segment contains seven
recorded positions. Its sound peak also coincides with a visible passing aircraft;
the frame does not independently identify that aircraft's registration.
No additional recording passed. See [short-track-results.json](short-track-results.json),
[short-reference-audit.json](short-reference-audit.json), and
[short-track-support-check.json](short-track-support-check.json).

The first support-only export used a stale standalone example executable.
That failed diagnostic is preserved. An explicit rebuild and separate repeat
verified the seven positions. The frozen CLI runs were not affected.

The Windows CI artifact reproduced the local search, controls, and regions in
two successful cases and one ambiguous case. Its CI checkout tree matched the
tested source tree. The portable 1 km runner also passed six guard tests,
rejected changed media before analysis, and preserved an existing report.
Its real positive case returned code 0; its ambiguous case returned code 1.
See [short-track-artifact-checks.json](short-track-artifact-checks.json) and
[timing-runner-checks.json](timing-runner-checks.json).

## Date search and offline analysis

Revision `09995cf` removes the archive date-range cap. Wider searches use
adjacent-day batches with overlap. Empty date fields select all dates found in
the provider cache. Offline mode blocks network requests and external media
requests through FFmpeg. The download command prepares a date range and optional
terrain for later offline use.

All 118 Rust tests, six Python tests, formatting, strict Clippy, documentation
tests, and release builds passed locally. New checks cover 33 cached days without
a supplied date, a midnight crossing, competing dates, blocked network requests,
monthly samples, and an imported reference spanning 41 days.

Two preserved real recordings were replayed offline with both date fields empty
and `start_utc: null`. The original broad search areas and audio were retained.
Maho Beach returned a location within 550 m, including its 200 m microphone
allowance. Midway searched November 1, 3, and 4, selected November 3, and returned
a location 89.43 m from the camera. Both passed the strict CLI status check.

The executable hash, media hashes, report hashes, checked dates, and results are
in [offline-date-search.json](offline-date-search.json).
