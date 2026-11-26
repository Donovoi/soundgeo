# Video-clue workflow checks

The [desktop record](desktop.json) covers the checkbox, tooltips, automatic
GeoNames preparation and separate result handling added in 0.4.0-alpha.2.
It records the tested source, frozen executable hashes, fixture hash, downloaded
data hashes and screenshot hashes. The checks ran on a headless Linux VM with
bounded CPU, memory and I/O. Screenshot artifacts were retained with the local
validation run.

The fixture was a six-second video with no audio, containing the literal text
`Gibraltar International Airport`, `2026-11-06 11:59:46` and
`SYNTHETIC OCR CHECK`. With the video checkbox selected and country `GI`, the
desktop downloaded the missing catalog and read the text. The clues remained
visible when audio analysis failed because the fixture had no audio stream.

The standalone video scan then reused the automatic cache with Offline mode
selected. A separate CLI replay ran with networking unavailable and confirmed
nine frame views, the matching name and literal clock text. Cached file hashes
and modification times remained unchanged during that replay.

With the video option still checked, the built-in audio-only synthetic demo
completed geolocation at its known coordinates while showing the missing-video
warning. The newly selected `US` place catalog was not downloaded.

These are functional checks. They do not measure real-world geolocation accuracy
or establish that a visible place name identifies the camera position. The
existing 1 km benchmark remains separate.

Rust regression tests cover default-off behavior, optional-result preservation,
worker lifetime, cancellation, country changes, explicit database overrides,
offline cache reuse and cancellation during place-data verification. The source
audit counts shared UI tests in both desktop and console binaries. Release CI
runs the full test suite and packages on Windows and Linux VMs before publishing.
