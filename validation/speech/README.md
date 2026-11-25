# Speech validation

The speech feature is qualified separately from aircraft geolocation. These
fixtures do not establish the location of a recording or change the 1 km target.

See the [model comparison and method](METHOD.md) and its
[machine-readable results](model-selection.json) for model choices, individual
measurements, source hashes and retained failed attempts.

## Integrated application checks

[Integrated results](integrated-app.json) record a frozen application and its
native helper hashes. All inference ran offline in a bounded VM. The first two
held-out recordings per language were evaluated through the full application,
including voice activity, detection, recognition, English translation,
timestamps and provenance. Hindi and Urdu used the explicit script choice.
All 20 returned a complete report. Completion means the pipeline finished;
it does not mean the transcript or translation was error-free.

| Language | Held-out CER, two recordings |
| --- | ---: |
| English | 5.63% |
| Mandarin Chinese | 1.41% |
| Hindi | 3.08% |
| Spanish | 0.00% |
| Arabic | 7.41% |
| French | 1.60% |
| Bengali | 6.18% |
| Portuguese | 1.98% |
| Indonesian | 0.00% |
| Urdu | 7.60% |

The predeclared stress check used the first held-out recording in each language,
prepended 1.25 seconds, mixed deterministic pink noise at amplitude 0.015 and
encoded AAC at 32 kb/s. Nine returned complete reports. The very quiet English
recording returned no detected speech after this transformation. The noise
amplitude was fixed, so the speech-to-noise ratio differs between recordings.
This is a small robustness check, not a measured general noise tolerance.

Silence, a tone, an ESC-50 airplane recording and a public-domain piano recording
returned no speech without needing recognition or translation weights. A German
recording was marked unsupported; mixed English/French requested a choice; a
150 ms excerpt returned no speech. A deliberately failed translator preserved
the real original transcript and a partial checkpoint. The record also retains
harness failures, including an invalid FFmpeg duration spelling and a concurrent
cache lock, so interrupted attempts are not counted as completed tests.

Headless desktop checks visually inspected Urdu, Bengali, Hindi and Chinese
rendering, automatic preparation and progress, and cancellation during real
quality translation. Cancel preserved the original French transcript in the
window and checkpoint. A confusing preparation status was changed from Partial
to Ready and verified in a subsequent real preparation and desktop run.

[Follow-up checks](followup-checks.json) record all 285 Rust test names matching
the published source, the required format/lint/build checks, public-URL intake
followed by real transcription, and a combined desktop analysis whose speech
result survived a subsequent aircraft-analysis error.

## Packaged runtime checks

`fixtures/manifest.json` records four held-out FLEURS recordings in English,
French, Bengali and Urdu. They were selected before integrated-app evaluation,
after excluding the model-selection sample. The manifest retains original
transcripts, independent English references, and source and fixture hashes.
The automated package check compares original transcripts with a broad
character-error bound and requires completed English output. It does not
establish translation meaning or field accuracy. Review of the Windows package
reports found an invented name in compact Urdu and a brief/permanent meaning
error in compact Bengali. The integrated quality-mode Urdu result also changed
who released the people described. These errors remain in the recorded outputs;
see [speech limitations](../../docs/limitations.md#speech-analysis).

The release check exercises real native model loading, Unicode recording paths,
automatic language detection, explicit language choices, offline cached use,
missing-model rejection, silence, preservation of existing reports and both
translation sizes. It opens an Urdu report in the desktop app. Rust tests also
check right-to-left and Indic shaping, delayed audio timestamps, corrupt caches,
language ambiguity, truncated translation output and disabled-feature behavior.

FLEURS recordings are provided under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
Attribution: Alexis Conneau and collaborators, *FLEURS: Few-shot Learning
Evaluation of Universal Representations of Speech* (2022),
[Google FLEURS](https://huggingface.co/datasets/google/fleurs), revision
`70bb2e84b976b7e960aa89f1c648e09c59f894dd`.
The original floating-point WAV recordings were encoded as 16 kHz mono FLAC
for compact storage. This converts floating-point samples to integer samples.
The app's 16-bit decode differs by at most one integer level from the original
WAV decode in each of the four fixtures; sample counts and rates are unchanged.
The [PCM comparison](fixture-pcm.json) records both hashes and measured differences.
The manifest retains each source filename, sentence ID and published-language
configuration. No words were added or removed.
