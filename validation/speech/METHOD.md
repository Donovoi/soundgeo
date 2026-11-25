# Speech model comparison, 25 November 2026

These measurements guide the initial local CPU model choices. They cover a small
read-speech sample. The integrated application, noise trials and release packages
have separate checks. Speech results do not alter aircraft-location acceptance.

## Selection and reproducibility

The source is Google FLEURS, revision
`70bb2e84b976b7e960aa89f1c648e09c59f894dd`, under CC BY 4.0. For each of the
ten languages, the selection plan took the first five distinct sentence IDs in
the original test TSV order with an independent English reference and a recording
between 5 and 25 seconds. [Selection samples](selection-samples.json) retain the
IDs, filenames, text references and source audio hashes. The next five eligible
IDs per language form the separate [held-out split](heldout-samples.json).

[Model-selection results](model-selection.json) retain individual results,
aggregate scores, exact model download hashes, native executable/library hashes,
source archive hashes and failed-attempt descriptions. The application registry
in `src/speech/models.json` pins every selected model to an immutable revision,
size and SHA-256 hash. Native source pins and the backend-initialization patch
are under `scripts/speech-tools-lock.json` and `scripts/speech-patches/`.

Model comparisons ran offline in a bounded Linux VM on CPU, using two inference
workers. Recognition used the known language for this selection experiment;
automatic language detection was evaluated separately. Whisper decoding used
temperature zero with no temperature increment. Qwen used a 4096-token context.
Bengali's final comparison disabled recognizer timestamps and used the complete
interval text. Product tests also exercise detection and voice-activity timing.

Translation comparisons used the published original transcript, rather than an
ASR hypothesis, to isolate translation quality. Each non-English language had
five sentences with an independent English reference: 45 cases per translator.
The prompt was:

```text
Translate the following text into English. Note that you should only output the translated result without any additional explanation:
<source text>
```

The native completion runner used its model chat template, temperature zero,
seed 42, CPU execution, a 2048-token context and a 512-token output limit. All
180 final translation cases reached the model's end-of-generation marker.
The application allows a 4096-token context and 1024 output tokens for longer
speech intervals, and rejects output stopped by the token limit.

## Metrics and model choices

Text normalization applies NFKC and case folding, keeps Unicode letters,
marks and numbers, and replaces other characters with spaces. Character error
rate (CER) removes spaces before computing Levenshtein distance. Word error
rate uses normalized space-delimited words; it is not useful for this Chinese
reference segmentation. Written numbers versus digits count as errors, as can
unspoken Latin names in parenthetical Chinese references.

The translation score is a custom character n-gram F2 over orders 1–6 after
the same normalization and space removal. It is **not SacreBLEU chrF**. It
measures agreement with one reference, so a reasonable paraphrase can score
poorly. Scores do not establish that names, numbers or factual meaning are correct.

| Selected recognizer | Language | Selection CER |
| --- | --- | ---: |
| Qwen3-ASR 1.7B Q8_0 | English | 6.20% |
| Qwen3-ASR 1.7B Q8_0 | Mandarin Chinese | 16.31% |
| Qwen3-ASR 1.7B Q8_0 | Hindi | 2.84% |
| Qwen3-ASR 1.7B Q8_0 | Spanish | 0.88% |
| Qwen3-ASR 1.7B Q8_0 | Arabic | 4.09% |
| Qwen3-ASR 1.7B Q8_0 | French | 0.56% |
| Bengali.AI Whisper medium Q5_0, no timestamps | Bengali | 3.11% |
| Qwen3-ASR 1.7B Q8_0 | Portuguese | 4.03% |
| Qwen3-ASR 1.7B Q8_0 | Indonesian | 2.45% |
| Whisper large-v3 Q5_0 | Urdu | 4.63% |

No recognizer won every language. Large-v3 had slightly lower CER on English,
Spanish, Arabic and Portuguese; Qwen had lower CER on the other four languages
it supports. Qwen was retained for its eight-language route because of its
overall accuracy and substantially shorter CPU processing time in this sample.
Five recordings per language are too few to establish a universal ranking.
Qwen does not support Bengali or Urdu in this model. The Bengali specialist
substantially improved on the general Whisper models in this test.

| Translator | Mean custom character F2, 45 cases |
| --- | ---: |
| Hy-MT2 1.8B Q4_K_M | 63.10 |
| Hy-MT3 1.8B Q4_K_M | 62.91 |
| Hy-MT3 1.8B Q8_0 | 63.67 |
| Hy-MT3 7B Q4_K_M | 65.87 |

Hy-MT3 7B is the quality default because it had the highest overall score among
these candidates. It was not best on every language. Its measured peak memory
was about 8 GB during this comparison. The smaller Q4 model remains an explicit
compact option; the Q8 compact candidate improved the aggregate score slightly
at the cost of a larger download. English needs no translation model.

## Detection and retained failures

ECAPA VoxLingua107 identified 46 of 50 selection recordings correctly; Whisper
base identified 45. ECAPA was selected as the small bundled detector. Three
Urdu recordings were confidently identified as Hindi, with top scores from
0.80 to 0.95. SoundGeo therefore asks for the Hindi/Urdu script even when the
score is high. A low-confidence Indonesian/Javanese confusion also supports
waiting for a choice. Scores are not calibrated probabilities. Conflicting
sections and confident unsupported languages do not force a supported language.

The initial shared-runtime Qwen attempt failed all 40 cases before inference
because native CPU backends were not initialized. The CLI initialization patch
fixed this, and all 40 cases were rerun. Native unit tests alone had not caught
that model-loading failure, so real inference is now a package release gate.

The first Whisper-medium comparison stopped on case 33 because Bengali output
contained invalid UTF-8; 32 earlier results remain recorded. A separate complete
large-v3 run had invalid UTF-8 in four Bengali cases despite successful process
exits. Their replacement-decoded text is retained for diagnosis and explicitly
marked invalid. Production parsing rejects invalid UTF-8. Large-v3 Bengali CER
was 44.35% on that diagnostic text, compared with 3.11% for the specialist.

The Bengali specialist's first timestamped run dropped opening words on two
recordings. Disabling timestamp decoding recovered them. Its CER fell from
5.51% to 3.11% over the same five cases. The application labels these times as
detected speech intervals, rather than word or recognizer alignment.

A translation probe returned no displayed text when `--log-disable` was used,
although the process exited successfully. That option was removed. A zero
exit code is also insufficient to distinguish completion from a token limit,
so the application checks the actual end-of-generation marker.

For examples of metric limits, an English recognizer output `100%` where the
reference spells out “one-hundred percent.” A low-scoring Spanish translation
preserved the meaning through a different paraphrase. A held-out English result
also introduced an extra article around a currency amount. Review the recording,
names, quantities and negations before using speech text as evidence.
