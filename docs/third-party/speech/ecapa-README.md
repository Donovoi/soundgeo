---
license: apache-2.0
base_model: speechbrain/lang-id-voxlingua107-ecapa
base_model_relation: quantized
library_name: transcribe.cpp
pipeline_tag: audio-classification
language:
  - ab
  - af
  - am
  - ar
  - as
  - az
  - ba
  - be
  - bg
  - bn
  - bo
  - br
  - bs
  - ca
  - ceb
  - cs
  - cy
  - da
  - de
  - el
  - en
  - eo
  - es
  - et
  - eu
  - fa
  - fi
  - fo
  - fr
  - gl
  - gn
  - gu
  - gv
  - ha
  - haw
  - hi
  - hr
  - ht
  - hu
  - hy
  - ia
  - id
  - is
  - it
  - iw
  - ja
  - jw
  - ka
  - kk
  - km
  - kn
  - ko
  - la
  - lb
  - ln
  - lo
  - lt
  - lv
  - mg
  - mi
  - mk
  - ml
  - mn
  - mr
  - ms
  - mt
  - my
  - ne
  - nl
  - nn
  - no
  - oc
  - pa
  - pl
  - ps
  - pt
  - ro
  - ru
  - sa
  - sco
  - sd
  - si
  - sk
  - sl
  - sn
  - so
  - sq
  - sr
  - su
  - sv
  - sw
  - ta
  - te
  - tg
  - th
  - tk
  - tl
  - tr
  - tt
  - uk
  - ur
  - uz
  - vi
  - war
  - yi
  - yo
  - zh
tags:
  - gguf
  - transcribe.cpp
  - language-identification
  - spoken-language-identification
  - ecapa-tdnn
  - voxlingua107
  - speechbrain
transcribe_cpp:
  schema_version: 2
  accuracy_fleurs_mul:
    f32: 85.23
    f16: 85.17
    q8_0: 86.3
  rtf_m4_max:
    cpu: 122.91
    metal: 1319.2
  rtf_ryzen_4750u:
    cpu: 42.16
    vulkan: 101.7
  streaming: false
  translate: false
  lang_detect: false
  timestamps: none
---

# lang-id-voxlingua107-ecapa: transcribe.cpp GGUF

GGUF conversions of [speechbrain/lang-id-voxlingua107-ecapa](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa) for use
with [transcribe.cpp](https://github.com/handy-computer/transcribe.cpp).

Ported from upstream commit
[0253049](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa/commit/0253049),
pinned 2026-10-05.
Validated against the SpeechBrain reference at transcribe.cpp commit
[aa4d0f47](https://github.com/handy-computer/transcribe.cpp/tree/aa4d0f47)
on 2026-10-08.

Spoken language identification over 107 languages: SpeechBrain's
ECAPA-TDNN trained on VoxLingua107. NOT a transcription model: a run
returns the model's language labels ranked by probability, optionally
restricted to a caller-chosen set. Takes 16 kHz mono WAV; scores up to
the first 30 s of a clip.


## Downloads

| Quantization | Download | Size | Top-1 accuracy (FLEURS multilingual) |
| --- | --- | ---: | ---: |
| F32 | [lang-id-voxlingua107-ecapa-F32.gguf](https://huggingface.co/handy-computer/lang-id-voxlingua107-ecapa-gguf/resolve/main/lang-id-voxlingua107-ecapa-F32.gguf) | 85 MB | 85.23% |
| F16 | [lang-id-voxlingua107-ecapa-F16.gguf](https://huggingface.co/handy-computer/lang-id-voxlingua107-ecapa-gguf/resolve/main/lang-id-voxlingua107-ecapa-F16.gguf) | 45 MB | 85.17% |
| Q8_0 | [lang-id-voxlingua107-ecapa-Q8_0.gguf](https://huggingface.co/handy-computer/lang-id-voxlingua107-ecapa-gguf/resolve/main/lang-id-voxlingua107-ecapa-Q8_0.gguf) | 27 MB | 86.30% |

Top-1 accuracy on FLEURS multilingual (3,000 utterances), scored on cpu. Measured at transcribe.cpp `cf62265a` on 2026-10-08.

Open-set top-1 accuracy over all 107 labels, the mean over 15 FLEURS
languages, on the first 5 s of each clip without silence trimming.
Agreement with the SpeechBrain reference, per GGUF, is on the
transcribe.cpp model page.


## Usage

Build transcribe.cpp from source:

```bash
git clone git@github.com:handy-computer/transcribe.cpp.git
cd transcribe.cpp
cmake -B build && cmake --build build
```

Run on a 16 kHz mono WAV. This is a language identifier, not a
transcription model: the CLI prints ranked language candidates.

```bash
build/bin/transcribe-cli -m lang-id-voxlingua107-ecapa-Q8_0.gguf --allow en,de,fr --top 3 input.wav
# language: de index=18 p=0.999998
#   ...
```

From the C API, use the LANGID role (`include/transcribe/langid.h`, see
`docs/langid.md`).

If your audio isn't already 16 kHz mono WAV, convert it first:

```bash
ffmpeg -i input.mp3 -ar 16000 -ac 1 output.wav
```

See the [transcribe.cpp model page](https://github.com/handy-computer/transcribe.cpp/blob/main/docs/models/lang-id-voxlingua107-ecapa.md) for performance
numbers, numerical validation, and reproduction steps.

## License

Inherited from the base model: **Apache-2.0**. See the
[upstream model card](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa) for full terms.
This repository includes the license text
([`LICENSE`](https://huggingface.co/handy-computer/lang-id-voxlingua107-ecapa-gguf/blob/main/LICENSE)) and a
[`NOTICE`](https://huggingface.co/handy-computer/lang-id-voxlingua107-ecapa-gguf/blob/main/NOTICE) recording
how these files were derived from the original.

---

## Original Model Card

> The section below is reproduced from
> [speechbrain/lang-id-voxlingua107-ecapa](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa) at commit
> `0253049` for offline reference. The upstream card is the
> authoritative source.

# VoxLingua107 ECAPA-TDNN Spoken Language Identification Model

## Model description

This is a spoken language recognition model trained on the [VoxLingua107 dataset](https://cs.taltech.ee/staff/tanel.alumae/data/voxlingua107/) using SpeechBrain.
The model uses the ECAPA-TDNN architecture that has previously been used for speaker recognition. However, it uses
more fully connected hidden layers after the embedding layer, and cross-entropy loss was used for training. 
We observed that this improved the performance of extracted utterance embeddings for downstream tasks.

The system is trained with recordings sampled at 16kHz (single channel).
The code will automatically normalize your audio (i.e., resampling + mono channel selection) when calling *classify_file* if needed.

The model can classify a speech utterance according to the language spoken.
It covers 107 different languages (
Abkhazian, 
Afrikaans, 
Amharic, 
Arabic, 
Assamese, 
Azerbaijani, 
Bashkir, 
Belarusian, 
Bulgarian, 
Bengali, 
Tibetan, 
Breton, 
Bosnian, 
Catalan, 
Cebuano, 
Czech, 
Welsh, 
Danish, 
German, 
Greek, 
English, 
Esperanto, 
Spanish, 
Estonian, 
Basque, 
Persian, 
Finnish, 
Faroese, 
French, 
Galician, 
Guarani, 
Gujarati, 
Manx, 
Hausa, 
Hawaiian, 
Hindi, 
Croatian, 
Haitian, 
Hungarian, 
Armenian, 
Interlingua, 
Indonesian, 
Icelandic, 
Italian, 
Hebrew, 
Japanese, 
Javanese, 
Georgian, 
Kazakh, 
Central Khmer, 
Kannada, 
Korean, 
Latin, 
Luxembourgish, 
Lingala, 
Lao, 
Lithuanian, 
Latvian, 
Malagasy, 
Maori, 
Macedonian, 
Malayalam, 
Mongolian, 
Marathi, 
Malay, 
Maltese, 
Burmese, 
Nepali, 
Dutch, 
Norwegian Nynorsk, 
Norwegian, 
Occitan, 
Panjabi, 
Polish, 
Pushto, 
Portuguese, 
Romanian, 
Russian, 
Sanskrit, 
Scots, 
Sindhi, 
Sinhala, 
Slovak, 
Slovenian, 
Shona, 
Somali, 
Albanian, 
Serbian, 
Sundanese, 
Swedish, 
Swahili, 
Tamil, 
Telugu, 
Tajik, 
Thai, 
Turkmen, 
Tagalog, 
Turkish, 
Tatar, 
Ukrainian, 
Urdu, 
Uzbek, 
Vietnamese, 
Waray, 
Yiddish, 
Yoruba, 
Mandarin Chinese).

## Intended uses & limitations

The model has two uses:

  - use 'as is' for spoken language recognition
  - use as an utterance-level feature (embedding) extractor, for creating a dedicated language ID model on your own data
  
The model is trained on automatically collected YouTube data. For more 
information about the dataset, see [here](https://cs.taltech.ee/staff/tanel.alumae/data/voxlingua107/).


#### How to use
```bash
pip install git+https://github.com/speechbrain/speechbrain.git@develop
```

```python
import torchaudio
from speechbrain.inference.classifiers import EncoderClassifier
language_id = EncoderClassifier.from_hparams(source="speechbrain/lang-id-voxlingua107-ecapa", savedir="tmp")
# Download Thai language sample from Omniglot and cvert to suitable form
signal = language_id.load_audio("speechbrain/lang-id-voxlingua107-ecapa/udhr_th.wav")
prediction =  language_id.classify_batch(signal)
print(prediction)
#  (tensor([[-2.8646e+01, -3.0346e+01, -2.0748e+01, -2.9562e+01, -2.2187e+01,
#         -3.2668e+01, -3.6677e+01, -3.3573e+01, -3.2545e+01, -2.4365e+01,
#         -2.4688e+01, -3.1171e+01, -2.7743e+01, -2.9918e+01, -2.4770e+01,
#         -3.2250e+01, -2.4727e+01, -2.6087e+01, -2.1870e+01, -3.2821e+01,
#         -2.2128e+01, -2.2822e+01, -3.0888e+01, -3.3564e+01, -2.9906e+01,
#         -2.2392e+01, -2.5573e+01, -2.6443e+01, -3.2429e+01, -3.2652e+01,
#         -3.0030e+01, -2.4607e+01, -2.2967e+01, -2.4396e+01, -2.8578e+01,
#         -2.5153e+01, -2.8475e+01, -2.6409e+01, -2.5230e+01, -2.7957e+01,
#         -2.6298e+01, -2.3609e+01, -2.5863e+01, -2.8225e+01, -2.7225e+01,
#         -3.0486e+01, -2.1185e+01, -2.7938e+01, -3.3155e+01, -1.9076e+01,
#         -2.9181e+01, -2.2160e+01, -1.8352e+01, -2.5866e+01, -3.3636e+01,
#         -4.2016e+00, -3.1581e+01, -3.1894e+01, -2.7834e+01, -2.5429e+01,
#         -3.2235e+01, -3.2280e+01, -2.8786e+01, -2.3366e+01, -2.6047e+01,
#         -2.2075e+01, -2.3770e+01, -2.2518e+01, -2.8101e+01, -2.5745e+01,
#         -2.6441e+01, -2.9822e+01, -2.7109e+01, -3.0225e+01, -2.4566e+01,
#         -2.9268e+01, -2.7651e+01, -3.4221e+01, -2.9026e+01, -2.6009e+01,
#         -3.1968e+01, -3.1747e+01, -2.8156e+01, -2.9025e+01, -2.7756e+01,
#         -2.8052e+01, -2.9341e+01, -2.8806e+01, -2.1636e+01, -2.3992e+01,
#         -2.3794e+01, -3.3743e+01, -2.8332e+01, -2.7465e+01, -1.5085e-02,
#         -2.9094e+01, -2.1444e+01, -2.9780e+01, -3.6046e+01, -3.7401e+01,
#         -3.0888e+01, -3.3172e+01, -1.8931e+01, -2.2679e+01, -3.0225e+01,
#         -2.4995e+01, -2.1028e+01]]), tensor([-0.0151]), tensor([94]), ['th'])
# The scores in the prediction[0] tensor can be interpreted as log-likelihoods that
# the given utterance belongs to the given language (i.e., the larger the better)
# The linear-scale likelihood can be retrieved using the following:
print(prediction[1].exp())
#  tensor([0.9850])
# The identified language ISO code is given in prediction[3]
print(prediction[3])
#  ['th: Thai']
  
# Alternatively, use the utterance embedding extractor:
emb =  language_id.encode_batch(signal)
print(emb.shape)
# torch.Size([1, 1, 256])
```
To perform inference on the GPU, add  `run_opts={"device":"cuda"}`  when calling the `from_hparams` method.

The system is trained with recordings sampled at 16kHz (single channel).
The code will automatically normalize your audio (i.e., resampling + mono channel selection) when calling *classify_file* if needed. Make sure your input tensor is compliant with the expected sampling rate if you use *encode_batch* and *classify_batch*.

Warning: In the dataset and in the defaults of this model (see [`label_encoder.txt`](label_encoder.txt), the used ISO language code for Hebrew is obsolete (should be `he` instead of `iw`). The ISO language code for Javanese is incorrect (should be `jv` instead of `jw`). See [issue #2396](https://github.com/speechbrain/speechbrain/issues/2396).

#### Limitations and bias

Since the model is trained on VoxLingua107, it has many limitations and biases, some of which are:

 - Probably it's accuracy on smaller languages  is quite limited
 - Probably it works worse on female speech than male speech (because YouTube data includes much more male speech)
 - Based on subjective experiments, it doesn't work well on speech with a foreign accent
 - Probably it doesn't work well on children's speech and on persons with speech disorders


## Training data

The model is trained on [VoxLingua107](https://cs.taltech.ee/staff/tanel.alumae/data/voxlingua107/).

VoxLingua107 is a speech dataset for training spoken language identification models. 
The dataset consists of short speech segments automatically extracted from YouTube videos and labeled according the language of the video title and description, with some post-processing steps to filter out false positives.

VoxLingua107 contains data for 107 languages. The total amount of speech in the training set is 6628 hours. 
The average amount of data per language is 62 hours. However, the real amount per language varies a lot. There is also a seperate development set containing 1609 speech segments from 33 languages, validated by at least two volunteers to really contain the given language.

## Training procedure

See the [SpeechBrain recipe](https://github.com/speechbrain/speechbrain/tree/voxlingua107/recipes/VoxLingua107/lang_id).

## Evaluation results

Error rate: 6.7% on the VoxLingua107 development dataset

#### Referencing SpeechBrain
```bibtex
@misc{speechbrain,
  title={{SpeechBrain}: A General-Purpose Speech Toolkit},
  author={Mirco Ravanelli and Titouan Parcollet and Peter Plantinga and Aku Rouhe and Samuele Cornell and Loren Lugosch and Cem Subakan and Nauman Dawalatabad and Abdelwahab Heba and Jianyuan Zhong and Ju-Chieh Chou and Sung-Lin Yeh and Szu-Wei Fu and Chien-Feng Liao and Elena Rastorgueva and François Grondin and William Aris and Hwidong Na and Yan Gao and Renato De Mori and Yoshua Bengio},
  year={2021},
  eprint={2106.04624},
  archivePrefix={arXiv},
  primaryClass={eess.AS},
  note={arXiv:2106.04624}
}
```

### Referencing VoxLingua107

```bibtex
@inproceedings{valk2021slt,
  title={{VoxLingua107}: a Dataset for Spoken Language Recognition},
  author={J{\"o}rgen Valk and Tanel Alum{\"a}e},
  booktitle={Proc. IEEE SLT Workshop},
  year={2021},
}
```

#### About SpeechBrain
SpeechBrain is an open-source and all-in-one speech toolkit. It is designed to be simple, extremely flexible, and user-friendly. Competitive or state-of-the-art performance is obtained in various domains.
Website: https://speechbrain.github.io/
GitHub: https://github.com/speechbrain/speechbrain
