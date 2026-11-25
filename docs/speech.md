# Local speech analysis

Speech analysis is optional and starts unchecked. All recognition and translation
run on the computer. The application downloads model weights from pinned public
Hugging Face URLs; recordings and transcripts stay local.

## Desktop use

1. Choose a recording or download a clip with the URL controls.
2. Tick **Transcribe speech and translate to English**.
3. Watch the status while SoundGeo decodes the recording, finds speech, detects
   its language and prepares the required model files.
4. If prompted, select the spoken language. Hindi uses Devanagari and Urdu uses
   Arabic script. Uncertain or conflicting detections wait for a choice.
5. Select **Analyze speech only**, or **Analyze recording** to also run geolocation.
6. Review the original transcript beside its English translation. Copy text or
   save a JSON, TXT, SRT or VTT file.

The ten initial languages are English, Mandarin Chinese, Hindi, Spanish, Arabic,
French, Bengali, Portuguese, Indonesian and Urdu. Automatic detection considers
107 languages, including languages outside this initial scope. A confident
unsupported detection does not download a recognizer for a different language.

## Downloads and memory

The EXE includes the native CPU runtimes, small speech/language detectors and
fonts for the supported scripts. Large recognition and translation weights are
downloaded only when needed. One shared multilingual translator covers all nine
non-English languages; it does not need a separate download for each language.

| Translation setting | Download | Intended use |
| --- | ---: | --- |
| Quality: Hy-MT3 7B Q4_K_M | 5.15 GB | Default; allow about 10 GB of free memory |
| Compact: Hy-MT3 1.8B Q4_K_M | 1.35 GB | Lower memory use; allow about 3 GB free |

Recognition downloads are separate. The initial routing uses Qwen3-ASR 1.7B
Q8_0 (2.19 GB) for its eight supported languages, Bengali.AI Whisper medium
Q5_0 (539 MB) for Bengali, and Whisper large-v3 Q5_0 (1.08 GB) for Urdu. Model
choices and measurements are recorded in the [speech validation record](../validation/speech/METHOD.md). English
recordings need a recognizer but no translator. With no speech detected, large
recognition and translation downloads are skipped.

Downloads have pinned revisions, exact sizes and SHA-256 hashes. Cached weights
are rechecked before use. Large downloads use resumable aria2 transfers when it
is available, with up to 16 connections for a single file. Retry after a network
interruption to reuse completed ranges. Status displays actual downloader
transfer information; processing stages show elapsed time and interval counts.

The default cache is `%LOCALAPPDATA%\SoundGeo\speech` on Windows and
`$XDG_CACHE_HOME/SoundGeo/speech` or `~/.cache/SoundGeo/speech` on Linux.
Choose another location under **Speech settings**. Models are under `models`;
automatic JSON checkpoints are under `reports`. Keep checkpoints if they form
part of your analysis record. Close the app before removing unused model files.
An offline run requires the appropriate models to have been prepared beforehand.

## Command line

```powershell
soundgeo-cli.exe transcribe --input recording.mp4 --output speech.json
soundgeo-cli.exe transcribe --url "https://example.org/video-page" --media-item 2 --output speech.json
soundgeo-cli.exe transcribe --input recording.mp4 --language ur --prepare-only --output preparation.json
soundgeo-cli.exe --offline transcribe --input recording.mp4 --language ur --output speech.json
soundgeo-cli.exe transcribe --input recording.mp4 --translation-quality compact --format srt --english --output english.srt
soundgeo-cli.exe view-speech speech.json
```

Use `--language en`, `zh`, `hi`, `es`, `ar`, `fr`, `bn`, `pt`, `id` or `ur` to
override detection. `--threads 2` is the default CPU limit. `--cache` changes the
speech cache, while `--media-cache` changes the URL download cache.
`--ffmpeg` overrides the audio decoder.

JSON always preserves the original and English text together. TXT/SRT/VTT use
the original text unless `--english` is supplied. Existing output files are
preserved. An incomplete English translation cannot be exported as a complete
English subtitle file; save the original or JSON instead. The command returns
exit code 3 when a language choice is needed or speech analysis is incomplete.
No-speech and successful preparation return zero. Prepared reports have status
`ready`; analysis changes this to `complete` or `partial`.

## Evidence and provenance

Speech uses a separate 16 kHz mono decode. It preserves the recording timeline,
including delayed audio starts. Qwen and Bengali timestamps describe detected speech
intervals; Urdu timestamps describe recognizer segments. Neither is claimed
as word-level alignment. JSON records the source hash, decoder version, model
revisions and hashes, helper executable hashes, language choices and warnings.

Original text is checkpointed before translation. Desktop cancellation or a model
failure produces an explicitly partial report and preserves completed text.
Speech results remain separate from the aircraft search and do not change the
1 km geolocation acceptance rules. Review [speech limitations](limitations.md#speech-analysis).
