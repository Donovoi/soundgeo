# Real-video validation

This is a failing acceptance benchmark, not a claim of reliable geolocation.
Four recordings from two public cameras were captured locally on 2026-11-04.
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
provider returned HTTP 403; no alternate authenticated data was available.

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

Next experiment uses only free data: ADS-B Exchange documents a free first-of-month sample bucket with public demonstration credentials. The direct web trace URL was unavailable, but its documented S3 sample endpoint was accessible. November 1 public-camera captures and whole-day sample references are being collected; no result from that experiment is claimed here.
