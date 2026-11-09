# Whole-activity Doppler controls

These development controls test how often reordered sounds produce strong
flight matches. They do not change location acceptance.

The planner groups detections that overlap or have less than 30 seconds between
their activity intervals. It includes each detection's full activity interval
and all alternative frequency curves. It cuts blocks in the remaining gaps.
Each of the 19 controls reorders whole blocks. Frequency samples and their
relative times stay unchanged. Identity and cyclic block orders are excluded
because the recording clock is unknown. Controls must also differ from each
other after cyclic rotation. At least five activity blocks are required to
produce 19 such controls.

The runner searches the original region and flight data again for each control.
It keeps the original search mode, model settings, and hypothesis limit. It
runs offline and receives no candidate coordinates or recording clock.
The saved plan binds the source report by SHA-256. Each result records the
block order, event shifts, full curves, search settings, and candidates.

```text
cargo run --release --locked --example doppler_controls -- plan report.json controls.json
cargo run --release --locked --example doppler_controls -- run report.json controls.json 0 control-00.json
```

Control indices run from 0 through 18. Output files must be new. Preserve a
failed search and its resource limits. Compare complete searches made with
the same matching code. A partial set of controls cannot establish a passing
signal screen.

Permutation inference depends on exchangeability. Temporal correlation can
violate that condition, as described in the
[FSL permutation documentation](https://fsl.fmrib.ox.ac.uk/fsl/docs/statistics/randomise.html).
Whole blocks preserve the measured curves and nearby activity. Exchangeability
of these internet recordings remains unverified. These results do not provide
a calibrated probability or a location accuracy bound. Any future acceptance
change also needs checks of distant competing locations and independent cases.
