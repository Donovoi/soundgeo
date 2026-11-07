# Limitations

SoundGeo is in development. Reliable location recovery from varied internet
samples remains unfinished. The current location target is within 1 km.

## Video text clues

Video text reading is optional. It needs Tesseract 5, English language data,
FFmpeg, FFprobe, and a local GeoNames file. It samples frames across the video
and reads the full image and the top and bottom edges. Text that appears briefly
can fall between the sampled frames. Small, blurred, or compressed text can fail.

Place matching uses complete names or aliases with two to twelve words.
Names must contain at least two words with two letters each and five letters
in total. Single-word names and short numeric aliases are excluded. All matching places remain in the
report. The order favors longer names and more matching text samples.
Different places can share a name. Signs, captions, and watermarks can describe
places away from the camera. GeoNames coordinates identify a named feature;
they do not establish the camera position or a location error radius.

Visible dates and times remain text clues. SoundGeo does not infer a timezone
or pass them into an audio search. Video clues have separate reports and
validation results. The audio-only benchmark does not use OCR.

## Real-video results

Revision `80d33f2` replayed 25 recordings with flight passage timing.
Each job used a possible date, a broad search area, free flight data, and an
unknown recording start time.

| Result | Recordings |
| --- | ---: |
| Returned a location within 1 km | 8 |
| Reported competing locations | 5 |
| Reported insufficient evidence | 12 |
| Returned a location outside 1 km | 0 |

Seven accepted locations came from one Midway camera, with errors of 89 to 547 m.
The eighth came from Maho Beach. Its error was within 550 m, including a planned
200 m allowance for the unsurveyed microphone. These repeated development tests
cover two successful sites. Broader accuracy remains unmeasured.

The change in `80d33f2` lets passage timing use flight segments with three to
seven recorded positions. It added one aircraft match to the Maho result.
The accepted recording count stayed at eight. Lake Hood, Southampton, and LAX
returned no accepted location. Other Maho intervals had timing breaks and were
rejected before analysis.

All 108 Rust tests and the required checks passed locally and in
[Windows and Ubuntu CI](https://github.com/Donovoi/soundgeo/actions/runs/42156168484).
The downloaded Windows executable reproduced the Midway and Maho results and
kept an ambiguous Maho result. Six Python tests check the benchmark runner.
Location accuracy needs separate tests against known positions.

The [validation record](../validation/README.md) preserves the original 250 m
plans, failed tests, older wrong Doppler results, source records, and build checks.

## Input coverage

- The decoder accepts recordings from 20 seconds to two hours long.
- Passage timing needs at least eight minutes of continuous audio and six
  sound peaks. It requires a search area.
- Automatic downloads use a selected date range. Cached analysis can search all
  dates available for the selected provider. Missing dates outside that cache
  remain unsearched. A report records the dates covered by a batched search.
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

The first-of-month sample uses 288 worldwide snapshots spaced five minutes
apart. Short flights between snapshots can be absent. Missing traces appear in
the report and cache manifest. This source permits testing and evaluation.

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
- A default budget of 5 million hypotheses, configurable up to 100 million.
- Latitudes from 85 degrees south to 85 degrees north.
- 64 terrain tiles per download request. Each expanded tile uses about 3 MB or
  26 MB, depending on resolution.

Split search regions that cross the date line. Large archives can require
substantial memory, storage, and processing time. A hypothesis-budget overrun
stops the search with an error and no partial location result.
