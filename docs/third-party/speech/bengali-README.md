---
license: apache-2.0
language:
- bn
base_model: bengaliAI/tugstugi_bengaliai-asr_whisper-medium
pipeline_tag: automatic-speech-recognition
tags:
- whisper
- whisper.cpp
- ggml
---

# Bengali.AI Whisper medium, for whisper.cpp

[Bengali.AI's Bengali Whisper medium model](https://huggingface.co/bengaliAI/tugstugi_bengaliai-asr_whisper-medium)
(from the tugstugi team's work for Bengali.AI), converted to the ggml format that
[whisper.cpp](https://github.com/ggml-org/whisper.cpp) reads and compressed to 5 bits.
It is offered as a download in [WinShades](https://winshades.org), a Windows reading and
writing tool, for offline dictation in Bengali.

All credit for the model goes to **Bengali.AI** and its authors.

## What was changed

The weights were converted and compressed; nothing was retrained.

1. Source: `bengaliAI/tugstugi_bengaliai-asr_whisper-medium` at revision
   `da605cc1bd2f60a18d8e440e977ddfa921a88e63`.
2. Converted with whisper.cpp's `models/convert-h5-to-ggml.py` (whisper.cpp commit
   `d09f61a`), at 16-bit precision.
3. Compressed to q5_0 with `whisper-quantize` from whisper.cpp release `b5130`.

| File | Size | SHA-256 |
|---|---:|---|
| `ggml-bengaliai-whisper-medium-q5_0.bin` | 539,257,671 bytes | `2c64c7f1e2c8f2417ed1fe5b78c7238933c00a2cdc2ef33e7087a95475256605` |

## How it did

Measured by WinShades on 25 clips of Bengali read speech from the test split of Google's
[FLEURS](https://huggingface.co/datasets/google/fleurs) dataset, as the proportion of
words wrong, missing or added:

| Model | Size | Words wrong |
|---|---:|---:|
| OpenAI Whisper base (q5_1) | 57 MB | 110% |
| OpenAI Whisper large-v3-turbo (q5_0) | 547 MB | 96% |
| AI4Bharat IndicWhisper medium (16-bit) | 1.5 GB | 61% |
| This model, 16-bit | 1.5 GB | 19% |
| **This model, q5_0** | **514 MB** | **20%** |

It is a medium-sized model: comfortably faster than speech on a graphics card, and just
keeping up on a fast desktop processor. Twenty-five clips rank models reliably but
aren't a precise figure.

## Licence

Apache 2.0, as the original; the full text is in `LICENSE`. This is a modified version:
the changes are listed above.
