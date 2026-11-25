# Limitations

SoundGeo is in development. Reliable location recovery from varied internet
samples remains unfinished. The current location target is within 1 km.

## Video text clues

Video text reading is optional. It needs Tesseract 5, English language data,
FFmpeg, FFprobe, and a local GeoNames file. The Windows portable EXE includes
the tools and English model; place data is prepared separately through the app.
It samples frames across the video
and reads the full image and the top and bottom edges. Text that appears briefly
can fall between the sampled frames. Small, blurred, or compressed text can fail.

Place matching uses complete names or aliases with two to twelve words.
Names must contain at least two words with two letters each and five letters
in total. Single-word names and short numeric aliases are excluded. All matching
places remain in the report. The order favors longer names and more matching text samples.
Different places can share a name. Signs, captions, and watermarks can describe
places away from the camera. GeoNames coordinates identify a named feature;
they do not establish the camera position or a location error radius.

Visible dates and times remain text clues. SoundGeo does not infer a timezone
or pass them into an audio search. Video clues have separate reports and
validation results. The audio-only benchmark does not use OCR.

## Real-video results

The fixed audio benchmark replayed 27 recordings with flight passage timing.
Each job used a broad search area and prepared free flight data. No recording
date or start time was supplied. All analysis ran offline. The available dates
differed by cache; the test covered a finite set of days.

| Result | Recordings |
| --- | ---: |
| Returned a location within 1 km | 8 |
| Reported competing locations | 5 |
| Reported insufficient evidence | 14 |
| Returned a location outside 1 km | 0 |

Seven accepted locations came from one Midway camera, with errors of 89 to 547 m.
The eighth came from Maho Beach. Its error was within 550 m, including a planned
200 m allowance for the unsurveyed microphone. These repeated development tests
cover two successful sites. Broader accuracy remains unmeasured.

A separate [prospective Amsterdam hour](../validation/amsterdam-fixed-hour-timing.json)
adds a conditional third successful site. The returned region's center was
268.54 m from the mapped reference, or 488.54 m including the declared 220 m
allowance. Both date windows completed all 19 controls. This result assumes
the ambient microphone and headset stayed within the mapped spectator area
and allowance; their positions were not surveyed. The model region radius
was 806 m. This frozen `4f8764c` test is separate from the original 27 recordings.
A [same-hour regression](../validation/amsterdam-float-roundtrip-regression.json)
with `65669df` preserved the result exactly apart from creation time. Its new
decoded-cache entries contained the same reference data. This repeat adds no
independent recording or site; the same microphone-location assumptions apply.

Six compression and duration checks used copies of three existing recordings.
Full-length 16 kbit/s Opus audio passed for Maho and Midway. Compressed Lake Hood
remained ambiguous. The first 15 minutes of all three recordings remained
unresolved. These checks cover a small part of the variation in internet audio.

Lake Hood, Southampton, and LAX returned no accepted audio location. Expanded
Maho tests searched three, ten, and twelve prepared sample dates with the same result:
one accepted recording, one ambiguous, and one insufficient.
The app has not been verified against a complete historical flight archive.

The [cached-attribution change](../validation/archive-attribution-reuse.json)
passed all 226 Rust tests, 10 Python tests and the required local Linux checks,
plus [Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42931899906).
This verifies archive reuse; it adds no accepted location.
Revision `8c46b16` passed all 225 Rust tests, 10 Python tests, and the required
checks locally on Linux and in
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42920141177).
Location accuracy needs separate tests against known positions.

The [JSON numeric-fidelity change](../validation/json-numeric-fidelity.json)
preserves saved reports when they are read back. It also changes parsing of
JSON flight references, so decoded tracks use a new cache version. Older cache
files are preserved. Exact report restoration and regression checks establish
numeric fidelity; earlier field results retain their original executable versions.

The latest detector change suppresses persistent tones in the fast frequency
bands. Offline replays preserve ten previously matched curves across two Lake
Hood hours. A fresh hour still has no passing individual curve at its known
camera point and clock. Its [completed three-date location replay](../validation/native-lake-fresh-replay-4f.json)
remained ambiguous: all eight retained candidates were 5.79 to 10.91 km from
the reference, each matching three of ten sounds. The original audit rejected
new cache receipts; a supplemental audit verified those receipts and unchanged inputs.
The [later original hour](../validation/native-lake-later-replay-82.json) also
remained ambiguous, with its top candidate 446.72 m away and competitors several
kilometres away. These runs add no accepted location.
A separate 20-minute Lake Hood recording produced no usable Doppler observations
and returned insufficient evidence before searching flight dates.

An additional fixed hour from Heathrow produced no timing sound peaks and
returned insufficient evidence before loading reference tracks or searching
dates. Its prepared three-date grid and shuffled controls were not executed.
The [failure record](../validation/heathrow-fixed-hour-timing.json) preserves the
unchanged inputs and terminal audit. The historical Doppler attempt was stopped
before completing its first date batch; it provides no completed location result.

A [reconstructed Heathrow Doppler hour](../validation/heathrow-recovery-dbc12f8-result.json)
completed all three prepared dates and 25 proposal batches with frozen `dbc12f8`.
It remained ambiguous, retaining ten candidates. The top candidate matched
21 of 24 sounds and was 89.80 m from the mapped reference, or 289.80 m with
the declared 200 m allowance. The other candidates were 250.22 to 1,040.42 m
from the reference. Ambiguity prevents acceptance under the fixed rule.
The filming-site reference assumes microphone co-location and movement within
the allowance. All 46 fixed inputs and the complete saved results passed VM
audits. The reconstructed media differs from the historical clip. No 19-control
Doppler screen was run, and this result adds no accepted recording or site.

A [conditional diagnosis](../validation/heathrow-one-km-diagnostic.json) reproduced
all 21 Heathrow matches exactly and checked every retained variant of the three
unmatched sounds against 5,467 flight tracks at the inferred point and clock.
Sounds 11, 14 and 21 had best relative errors of 2.91%, 3.30% and 2.12%, above
the unchanged 1.5% pair limit. The best associations had full reference coverage
and did not reuse a matched flight. Sounds 11 and 14 also failed the requirement
to halve squared error relative to linear drift. Sound 21 passed that check,
but adding its best variant would raise joint error to 1.0206%, above 1%.
No variant passed the individual fit checks. The mismatch's physical cause
remains unresolved. This diagnostic does not change the historical result.

Optional video text tests matched an airport name within 31.06 m of the camera
reference in two 360p Lake Hood clips. The reports also contained a broad
metro-area clue. Seven low-resolution files returned no place matches. This
evaluates visible-name matching. The audio result for Lake Hood remains unresolved.

The [validation record](../validation/README.md) preserves the original 250 m
plans, failed tests, older wrong Doppler results, source records, and build checks.

## Input coverage

The [public investigative footage checks](../validation/public-investigative-footage.json)
examined one clip whose publication leaves its filming location unidentified and
one separate known-site comparison. The 27.41-second Bahamas clip produced no
Doppler observations or timing peaks. The 55.68-second Sycamore Gap excerpt
produced no Doppler observations and two timing peaks. Both analyses returned
insufficient evidence before loading flight references. Neither yielded a location.
Unavailable videos and a previously resolved filming location are recorded separately.

`check-audio` runs the default audio detectors without flight data, dates or a
search area. Its report records whether each method has enough observations to
attempt a search. This does not identify aircraft, verify a continuous timeline,
or establish location accuracy. The existing 20-second to two-hour decoder limits
apply. Decode failures return an error; no successful intake report is written.
Publication dates and container creation times can describe an upload or export.
They do not establish when the scene was recorded.

- Audio analysis accepts recordings from 20 seconds to two hours long.
  Optional video text can read shorter clips.
- Passage timing needs at least eight minutes of continuous audio and six
  sound peaks. It requires a search area.
- With blank dates, online analysis discovers the provider's published days and
  includes older prepared days. Offline analysis uses prepared days only.
  A report records the dates searched and the provider index used.
- Date discovery reads provider metadata. Each flight archive still needs a
  complete download and integrity check. Airport indexes use the advertised
  replay period and exclude partial UTC days. Source gaps can stop a download.
- Full date indexes are reused for up to six hours online. During discovery,
  ADSB.lol checks recent publications if the last check is at least 15 minutes
  old. A failed recent check
  keeps the saved dates and adds a warning. The `dates --offline` command reads
  a saved index; flight files must be downloaded separately.
- Online preparation checks for a preferred ADSB.lol production release when a
  saved date uses a fallback release. Checks are at least 15 minutes apart.
  If a complete saved archive is available, a failed check or replacement download
  keeps that archive in use and reports the failure. Offline runs use the saved selection. New releases
  can contain more flight positions, but coverage still depends on the source.
- A release can appear before all its parts finish uploading. Online preparation
  refreshes an incomplete release and downloads newly listed parts. Incomplete
  archives stop offline preparation. Part checksums and the full archive end marker
  must pass before a split archive is marked ready.
- A worldwide search across all available dates can need several TB of storage
  and a long processing time. Downloads can be cancelled and resumed. Use the
  `dates` command to inspect coverage before preparing a large cache.
- Wider ranges increase download size and total processing time. Analysis loads
  overlapping daily batches and compares their results. Free sources can have
  gaps or shorter retention periods. A missing required archive stops the run.
- Offline analysis requires the complete selected flight cache, aircraft catalog
  where used, and terrain cache where configured. SoundGeo and FFmpeg must also
  be installed. Missing or damaged downloaded files stop the offline run.
  Recorded source gaps remain visible when the available cache is used offline.
- The current model fits one fixed recording location. Camera movement can
  invalidate that fit.
- Edits that remove, repeat, or rearrange audio can break the shared timeline.
  Automatic repair of these edits remains unfinished.
- Doppler matching can fit a small, constant clock-rate error. Passage timing
  uses a fixed clock rate.
- Noise, wind, changing engine speed, unstable tones, and changing weather can
  prevent useful matches. Sound detectors can also select unrelated sounds.

The existing fixed-camera tests cover a small part of the intended OSINT inputs.
Tests with varied lengths, compression, edits, movement, and sound quality are
still needed. The synthetic demo and selected noise fixture exercise software
behavior. General accuracy and false-alarm rates need independent measurements.

## Flight data

Free archives can miss audible aircraft or parts of their paths. A complete
download confirms the requested files and their integrity. Aircraft coverage
still depends on the source network.

The first-of-month sample checks 288 worldwide snapshot slots at five-minute
intervals. If a snapshot is missing, it tries the preceding and following
minute within the same day. The cache keeps the actual source filename and
records the replacement. Short flights between snapshots can be absent.
Historical snapshots can be delayed relative to their filenames. Preparation
accepts offsets smaller than one five-minute discovery interval. Offsets over
one minute appear in the manifest and report. Flight-trace times are preserved.
Some historical traces include positions from the preceding UTC day. The cache
keeps these source files and records the overlap. Sample analysis uses positions
within the selected archive dates.
Missing traces, snapshot replacements, and unavailable discovery slots appear
in the report and cache manifest. A slot is recorded as unavailable when its
file and the checked neighboring files all return HTTP 404. Preparation reuses
verified completed days, including their recorded source gaps. The download
option `--refresh-gaps` retries unavailable source files. The desktop provides
the same action with **Retry unavailable source files**. Offline reuse preserves
recorded gaps and rejects missing or damaged downloaded files. Online preparation
repairs missing or damaged cache files. Preparation stops if no discovery snapshot is
available for the day. This source permits testing and evaluation.

When a daily trace is unavailable, the downloader checks for an airborne
position in the discovery snapshots. It then downloads five-second snapshots
from five minutes before to five minutes after that position. Overlapping
windows share the same source files. This can recover short flight segments.
The cache records the source snapshots, attempted slots, missing files, and
derived traces. The missing daily trace remains listed in the manifest.
Online preparation also runs this recovery once for older caches with missing
daily traces. It keeps the original manifest and reuses saved source files.
Offline reads leave older caches unchanged; prepare them online to add recovery
data before offline use.

Discovery uses the same nearby flight area as the flight reader. It extends
0.6 degrees beyond the observer's latitude bounds and adjusts the longitude
margin for latitude. Older caches selected aircraft only inside the observer
bounds. Online preparation builds a separate cache with the wider selection.
It reuses verified source files and recorded source gaps. The older cache stays
available for offline use if preparation stops. A completed wider cache must
pass its own integrity checks before offline use.

Recovered positions use the snapshot time minus the reported position age.
Geometric and barometric altitude remain separate. Duplicate position times
are counted once, and gaps over 20 seconds split the path. Ground records do
not supply flight altitude. These windows can miss other parts of a flight.
Snapshot recovery also needs an airborne discovery position; a flight between
discovery snapshots can remain absent. Samples before May 2020 use the source's
one-minute snapshot interval. Offline use requires a prepared cache.

The high-resolution sample uses the same discovery snapshots and first-of-month
coverage. It can add positions for a recorded flight, but source gaps and flights
missed during discovery remain. Downloads and disk use can be larger. Selecting
high-resolution data does not establish a location by itself.

The free sample downloader retrieved all 288 snapshots and 97 Maho traces in
validation. Every file matched an independent download. Cache reuse and recovery
of a missing trace passed. LAX full-day download, cache reuse, and missing-part
recovery also passed. These checks establish download behavior on those inputs.

The Chicago and LAX sources cover local radar areas and retain about 90 days
of replay. Older complete local caches remain usable. The original radar sensor
and pressure correction are unknown. Reports identify intervals whose radar
status is offline.

Passage timing requires three recorded positions per flight segment. Doppler
matching requires eight. Gaps between positions must be at most 60 seconds.
Sparse tracks, altitude errors, and missing reference days can block a search.

## Search and result interpretation

Searches use a finite set of locations, clock hypotheses, and candidate flights.
Some possible solutions can remain unexplored. One flyover can fit many places
and times. Several flyovers can still leave competing locations.

`candidates_found` means that a result passed the model checks. Verify the
location against independent evidence. `--require-match` checks for candidates;
an ambiguous result can satisfy that option. `--require-unambiguous` also checks
the report status. A clock-rate fit with only three matched sounds remains
provisional.

Error percentages measure residual fit. Signal contrast controls detection.
Doppler candidates are ranked by their improvement over a straight-line
frequency drift. Each matched sound contributes its covered fraction multiplied
by `1 - (model RMSE / linear RMSE)^2`, bounded between zero and one. Contributions
are added across sounds. Match count and residual error break ties; quiet-gap
conflicts retain their residual penalty. Missing or invalid baseline measurements
contribute zero. This score still needs full-search testing on further recordings
and shuffled controls.

The experimental `doppler-location-1km-v1` screen permits unmatched sounds when
at least two-thirds of detections match. Recordings with three or more detections
need at least three matched sounds; fitting clock drift needs four. The existing
two-sound unit-rate case is retained. A fit more than 1,000 m from the leading
point blocks acceptance if it has at least 90% of the leader's curve support.
Nearby clock alternatives remain visible through `clock_ambiguous`, independently
of the location decision. The radius is measured from the reported point.
The consensus and support thresholds are heuristics that need new field and
negative-control testing; they do not estimate the probability of a 1 km error.

Competitor summaries survive the ten-candidate display limit, spatial
deduplication, and proposal/date merges. Older reports lack these summaries and
remain ambiguous when reassessed. Their saved candidates can support a separate
post-hoc diagnostic. The original Heathrow result and fixed acceptance plan
remain unchanged.

`diagnose_unmatched` reuses the production pair fitter at a report's inferred
point and clock. It checks every retained observation variant against selected
recorded reference tracks, reproduces the existing matches, and reports coverage,
curvature, residual, joint-error and flight-reuse checks. Final-point coarse
features cannot reconstruct an earlier proposal's pruning. These diagnostics
do not identify the physical sound source or establish a new location result.

The timing region encloses competitive search points, and `enclosing_radius_m`
describes that region. Calibrated probabilities and location error bounds remain
unavailable. Decimal places and grid spacing describe numerical precision.

Quiet-gap checks assume that a nearby aircraft would be audible. Strict mode
can reject a true location when that assumption fails. Grouping overlapping
sound curves can also combine two real aircraft. Short fragments and turning
aircraft can bias the initial straight-flight estimate.

A wider aircraft-distance setting can increase runtime and chance matches.
A larger hypothesis budget can increase runtime while still ending without a
useful location. Other active searches affected several recorded runtimes;
those timings require controlled repeats for performance comparisons.

## Speech analysis

Speech recognition and translation are machine-generated evidence aids. Names,
numbers, negation, dialects and background speech can be wrong even when the text
reads fluently. Music, added narration, overlapping speakers, strong noise and
edited recordings can produce missing or invented words. Review the original
recording and preserve the original-language transcript alongside translations.
The initial model comparison uses a small read-speech sample; it does not
establish accuracy for arbitrary crime or missing-person footage.

Detection samples at most three speech intervals. It can miss a language spoken
elsewhere in the recording. A recording is transcribed with one selected
language; mixed-language recordings should be split or reviewed carefully.
Hindi and Urdu require an explicit script choice because the tested detectors
confused them even at high model scores. Detection scores are uncalibrated.

The first version supports ten languages and a maximum of two hours of audio.
It uses the first audio stream, downmixes its channels to mono, and runs on CPU.
Long clips are divided into speech intervals of up to 24 seconds. Boundaries
can split words or lose context. There is no speaker identification or reliable
word-level alignment. Qwen and Bengali times identify speech intervals; Urdu times are
recognizer estimates. Subtitle formats do not carry that provenance; keep the
JSON report alongside subtitles used as evidence.

The quality translator needs several GB of disk and memory and can take longer
than the recording duration on a CPU. The compact option trades quality for
smaller downloads and memory use. Network access is needed only for uncached
models and media downloads. A damaged or missing cache fails in offline mode.
Speech analysis preserves completed originals in a partial report if translation
fails or the desktop Cancel button is used. It does not infer a recording
location from the spoken language or change aircraft-based location acceptance.

See [speech use and model downloads](speech.md).

## Terrain and processing

Terrain tiles supply ground elevation. Microphone height on buildings or towers
needs a separate estimate. Missing height samples use the configured fallback
and add a warning. One local Chicago diagnostic improved with terrain and still
retained competing locations.

Pressure-altitude correction and atmospheric ray tracing remain unimplemented.
EGM96 can differ from national height datums. Terrain cache hashes check local
file integrity; publisher-signed checksums are unavailable for this cache.

Current processing bounds are:

- 10 million retained flight positions.
- 32 MB decoded per aircraft trace.
- 50,000 regional grid points.
- A default budget of 5 million hypotheses per flight-proposal batch, configurable
  up to 100 million. Large corridor searches divide automatically by flight
  timestamp. Matching and refinement use the complete reference set in every
  batch. The recording clock remains unknown when no time was supplied.
- Automatic proposal division stops at a one-second flight interval or 4,096
  batches. Every batch must pass its budget check before matching starts. A
  failed check returns no location. Reports record all completed partitions in
  `doppler_partitions`; the total work can exceed the budget for one batch.
- Latitudes from 85 degrees south to 85 degrees north.
- 64 terrain tiles per download request. Each expanded tile uses about 3 MB or
  26 MB, depending on resolution.

Split search regions that cross the date line. Large archives can require
substantial memory, storage, and processing time. A hypothesis-budget overrun
stops the search with an error and no partial location result.

The `--search-threads` option controls flight-corridor matching. Other search
stages keep their existing worker settings. Terrain-folder searches can share
an immutable tile snapshot within a 128 MiB combined cache, decoded-tile and
temporary-read bound. Larger or unsupported layouts keep serial corridor
matching. Prepared tile fingerprints remain active through refinement, and a
detected file change or cancellation rejects the result. Missing-sample counts
use the actual worker queries. More workers can use more memory. Field speedup
has not been measured for this change; speed depends on the search and other
work running on the computer.
