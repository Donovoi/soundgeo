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

The runner uses the application's post-observation pipeline for each control.
It repeats the completed baseline's resolved dates, overlapping date windows,
and source files, using production proposal partitioning and candidate merging.
Changed observations can produce different proposal partitions under the same
budget. Identity replay must reproduce the original partition behavior.
It keeps the original
region, search mode, model settings, and hypothesis limit. It runs offline and
receives no candidate coordinates or recording clock.

Schema-2 plans bind the source report, control executable, current reference
files and metadata, terrain, and explicit or inferred aircraft catalogs.
Missing dates, changed inputs, incomplete baseline coverage, and incomplete
proposal partitions are rejected. Inputs are checked before and after each
search. Keep plans, outputs, and checkpoint folders outside input folders.
Preserve schema-1 plans as historical evidence and create a new plan before
using this runner. Single-date and imported-reference searches remain supported.

Each result retains a full native report alongside the block order, event
shifts, full curves, search settings, and candidates.
The runner checks the saved duration against the source report. It calculates
all shifts from that report again, so JSON rounding cannot change a control.
Replay uses the observations decoded from the saved report. It does not claim
bit-identical recovery of floats from an older report's original in-memory
values. An old report also does not prove its historical raw input bytes;
separate frozen-input receipts are needed to establish that link.

```text
cargo run --release --locked --example doppler_controls -- replay report.json identity.json
cargo run --release --locked --example doppler_controls -- plan report.json controls.json
cargo run --release --locked --example doppler_controls -- run report.json controls.json 0 control-00.json
```

`replay` uses the saved observations unchanged and does not require five activity
blocks. It retains the raw source-report hash, current search binding, and full
native report. Use this diagnostic to compare an older JSON report with the
qualified replay implementation before interpreting shuffled controls. It does
not establish a new independent location result.

For a multi-date baseline, `run` also accepts
`--date-checkpoints control-00-checkpoints`. Use a separate folder for each
control. Complete batches can be recovered with unchanged inputs; checkpoints
for the original observations or another permutation are rejected.

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
