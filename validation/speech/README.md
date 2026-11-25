# Speech validation

The speech feature is qualified separately from aircraft geolocation. These
fixtures do not establish the location of a recording or change the 1 km target.

`fixtures/manifest.json` records four held-out FLEURS recordings in English,
French, Bengali and Urdu. They were selected before integrated-app evaluation,
after excluding the model-selection sample. The package check uses original
transcripts and independent English references, and records source and fixture
hashes. A broad character-error bound catches unusable packaged recognizers;
it is not an estimate of field accuracy.

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
The original WAV recordings were encoded as 16 kHz mono FLAC for compact storage.
The manifest retains each source filename, sentence ID and published-language
configuration. No words were added or removed.
