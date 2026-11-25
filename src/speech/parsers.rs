//! Strict adapters for the pinned native tools' machine-readable output files.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub start: u64,
    pub end: u64,
}

pub(crate) fn vad(text: &str, samples: u64) -> Result<Vec<Span>> {
    let mut spans = Vec::new();
    for (index, line) in text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(
            fields.len() == 6 && fields[0] == "segment:" && fields[1].parse::<usize>()? == index,
            "Invalid voice activity output"
        );
        let start = fields[2]
            .strip_prefix("start=")
            .context("Missing speech start")?
            .parse::<u64>()?;
        let end = fields[3]
            .strip_prefix("end=")
            .context("Missing speech end")?
            .parse::<u64>()?;
        ensure!(
            end > start
                && end <= samples
                && spans.last().is_none_or(|span: &Span| span.end <= start),
            "Voice activity timestamps are outside the recording or overlap"
        );
        spans.push(Span { start, end });
    }
    Ok(spans)
}

pub(crate) fn language_candidates(text: &str) -> Result<Vec<(String, f64)>> {
    let mut candidates = Vec::new();
    for (index, line) in text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(
            fields.len() == 6
                && fields[0] == "candidate:"
                && fields[1].parse::<usize>()? == index + 1,
            "Invalid language detection output"
        );
        let language = fields[2];
        ensure!(
            (2..=3).contains(&language.len()) && language.bytes().all(|b| b.is_ascii_lowercase()),
            "Invalid detected language code"
        );
        let score = fields[4]
            .strip_prefix("p=")
            .context("Missing language score")?
            .parse::<f64>()?;
        ensure!(
            score.is_finite()
                && (0. ..=1.).contains(&score)
                && candidates
                    .last()
                    .is_none_or(|(_, previous)| *previous >= score),
            "Invalid language score order"
        );
        ensure!(
            !candidates.iter().any(|(code, _)| code == language),
            "Repeated language candidate"
        );
        candidates.push((language.to_owned(), score));
    }
    ensure!(
        !candidates.is_empty(),
        "The language detector returned no candidates"
    );
    Ok(candidates)
}

/// The completion CLI emits this final marker only after an end-of-generation
/// token. An exit code of zero alone also includes token-budget exhaustion.
pub(crate) fn translation(text: &str) -> Result<String> {
    let text = text
        .trim()
        .strip_suffix("[end of text]")
        .context(
            "Translation stopped before the model finished; original transcript was preserved",
        )?
        .trim();
    ensure!(!text.is_empty(), "The translation model returned no text");
    ensure!(!text.contains('\0'), "Invalid translation output");
    Ok(text.to_owned())
}

/// Add small context padding and group nearby speech without duplicating samples.
/// Long continuous speech is cut at a low-energy point near the maximum duration.
pub(crate) fn chunks(spans: &[Span], samples: &[i16], max_samples: usize) -> Result<Vec<Span>> {
    ensure!(
        max_samples >= 16_000,
        "Speech chunks must allow at least one second"
    );
    let total = samples.len() as u64;
    let mut groups: Vec<Span> = Vec::new();
    for span in spans {
        ensure!(
            span.start < span.end && span.end <= total,
            "Invalid speech span"
        );
        let padded = Span {
            start: span.start.saturating_sub(2400),
            end: (span.end + 2400).min(total),
        };
        if let Some(last) = groups.last_mut()
            && padded.start <= last.end + 6400
            && padded.end.saturating_sub(last.start) <= max_samples as u64
        {
            last.end = last.end.max(padded.end);
        } else {
            // Adjacent padded regions may overlap when their union is too long.
            // Share their midpoint to avoid duplicate words in exported evidence.
            let mut padded = padded;
            if let Some(last) = groups.last_mut()
                && padded.start < last.end
            {
                let boundary = (padded.start + last.end) / 2;
                last.end = boundary;
                padded.start = boundary;
            }
            groups.push(padded);
        }
    }
    let mut result = Vec::new();
    for span in groups {
        let mut start = span.start as usize;
        let end = span.end as usize;
        while end - start > max_samples {
            let upper = start + max_samples;
            let lower = upper - 8000.min(max_samples / 4);
            // 20 ms mean-square windows; integer arithmetic avoids NaN and
            // squaring i16 in its narrow representation.
            let boundary = (lower..=upper - 320)
                .step_by(160)
                .min_by_key(|&offset| {
                    samples[offset..offset + 320]
                        .iter()
                        .map(|&x| i64::from(x).pow(2))
                        .sum::<i64>()
                })
                .context("Could not split speech audio")?
                + 160;
            result.push(Span {
                start: start as u64,
                end: boundary as u64,
            });
            start = boundary;
        }
        if end > start {
            result.push(Span {
                start: start as u64,
                end: end as u64,
            });
        }
    }
    Ok(result)
}

#[derive(Deserialize)]
struct WhisperOutput {
    result: WhisperResult,
    transcription: Vec<WhisperSegment>,
}
#[derive(Deserialize)]
struct WhisperResult {
    language: String,
}
#[derive(Deserialize)]
struct WhisperSegment {
    offsets: Offsets,
    text: String,
}
#[derive(Deserialize)]
struct Offsets {
    from: u64,
    to: u64,
}

pub(crate) fn whisper(text: &str, language: &str, span: Span) -> Result<Vec<super::Segment>> {
    let output: WhisperOutput = serde_json::from_str(text).context("Invalid recognizer JSON")?;
    ensure!(
        output.result.language == language,
        "The recognizer returned a different language than requested"
    );
    let duration = (span.end - span.start) as f64 / 16000.;
    let offset = span.start as f64 / 16000.;
    let mut segments = Vec::new();
    let mut previous = 0.;
    for segment in output.transcription {
        if segment.text.trim().is_empty() {
            continue;
        }
        let start = segment.offsets.from as f64 / 1000.;
        let end = segment.offsets.to as f64 / 1000.;
        // Whisper may round the final boundary to its next 10 ms tick.
        ensure!(
            start >= previous && end > start && start < duration && end <= duration + 0.05,
            "Recognizer timestamps exceed the decoded speech chunk"
        );
        previous = start;
        segments.push(super::Segment {
            start_s: offset + start,
            end_s: offset + end.min(duration),
            timestamp_kind: "asr_segment".into(),
            language: language.into(),
            original: segment.text.trim().into(),
            english: None,
            warnings: vec![],
        });
    }
    Ok(segments)
}

/// Fine-tuned recognizers trained without timestamp tokens must be decoded that
/// way. Their text is attached to the independently detected audio interval.
pub(crate) fn whisper_interval(
    text: &str,
    language: &str,
    span: Span,
) -> Result<Vec<super::Segment>> {
    let output: WhisperOutput = serde_json::from_str(text).context("Invalid recognizer JSON")?;
    ensure!(
        output.result.language == language,
        "Recognizer language differs from the request"
    );
    let original = output
        .transcription
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    ensure!(
        !original.is_empty() && !original.contains(['\0', '\u{fffd}']),
        "Recognizer returned empty or damaged original text"
    );
    Ok(vec![super::Segment {
        start_s: span.start as f64 / 16000., end_s: span.end as f64 / 16000.,
        timestamp_kind: "speech_interval".into(), language: language.into(), original, english: None,
        warnings: vec!["Times identify a detected speech interval; this recognizer is decoded without timestamp tokens.".into()],
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_requires_actual_completion_and_nonempty_text() {
        assert_eq!(translation("Lahore. [end of text]\n\n").unwrap(), "Lahore.");
        assert!(translation("A truncated sentence").is_err());
        assert!(translation(" [end of text]").is_err());
        assert!(translation("").is_err());
    }

    #[test]
    fn native_detection_output_does_not_force_the_ten_languages() {
        let result = language_candidates("candidate: 1 ja index=1 p=0.920000 logit=4.0\ncandidate: 2 zh index=2 p=0.030000 logit=1.0\n").unwrap();
        assert_eq!(result[0].0, "ja");
        assert!(language_candidates("candidate: 1 en index=1 p=NaN logit=0").is_err());
        assert!(
            language_candidates(
                "candidate: 1 en index=1 p=0.1 logit=0\ncandidate: 2 fr index=2 p=0.9 logit=1"
            )
            .is_err()
        );
        assert!(language_candidates("").is_err());
    }

    #[test]
    fn vad_accepts_silence_and_rejects_out_of_range_or_overlapping_times() {
        assert!(vad("", 16000).unwrap().is_empty());
        assert!(vad("segment: 0 start=100 end=17000 t0=0.006 t1=1.062", 16000).is_err());
        assert!(vad("segment: 0 start=100 end=8000 t0=0 t1=0.5\nsegment: 1 start=7000 end=15000 t0=0.4 t1=0.9", 16000).is_err());
        assert_eq!(
            vad("segment: 0 start=100 end=15000 t0=0.006 t1=0.938", 16000).unwrap(),
            [Span {
                start: 100,
                end: 15000
            }]
        );
    }

    #[test]
    fn speech_chunking_keeps_original_offsets_and_never_duplicates_padding() {
        let samples = vec![100i16; 80 * 16000];
        let spans = [
            Span {
                start: 10 * 16000,
                end: 39 * 16000,
            },
            Span {
                start: 39 * 16000 + 1600,
                end: 75 * 16000,
            },
        ];
        let chunks = chunks(&spans, &samples, 30 * 16000).unwrap();
        assert_eq!(chunks[0].start, spans[0].start - 2400);
        assert_eq!(chunks.last().unwrap().end, spans[1].end + 2400);
        assert!(
            chunks
                .iter()
                .all(|s| s.end > s.start && s.end - s.start <= 30 * 16000)
        );
        assert!(chunks.windows(2).all(|pair| pair[0].end <= pair[1].start));
    }

    #[test]
    fn recognizer_timestamps_are_rebased_once_and_original_script_is_preserved() {
        let json = r#"{"result":{"language":"bn"},"transcription":[{"offsets":{"from":100,"to":900},"text":" ঢাকা "}]}"#;
        let segments = whisper(
            json,
            "bn",
            Span {
                start: 160000,
                end: 176000,
            },
        )
        .unwrap();
        assert_eq!(segments[0].start_s, 10.1);
        assert_eq!(segments[0].end_s, 10.9);
        assert_eq!(segments[0].original, "ঢাকা");
        assert!(
            whisper(
                json,
                "hi",
                Span {
                    start: 0,
                    end: 16000
                }
            )
            .is_err()
        );
        assert!(
            whisper(
                json,
                "bn",
                Span {
                    start: 0,
                    end: 8000
                }
            )
            .is_err()
        );
    }

    #[test]
    fn timestamp_free_bengali_keeps_the_full_text_on_the_original_speech_interval() {
        let json = r#"{"result":{"language":"bn"},"transcription":[{"offsets":{"from":0,"to":0},"text":" গুরুত্বপূর্ণ সামরিক "}]}"#;
        let span = Span {
            start: 20000,
            end: 100000,
        };
        let segments = whisper_interval(json, "bn", span).unwrap();
        assert_eq!(segments[0].original, "গুরুত্বপূর্ণ সামরিক");
        assert_eq!((segments[0].start_s, segments[0].end_s), (1.25, 6.25));
        assert_eq!(segments[0].timestamp_kind, "speech_interval");
        assert!(whisper_interval(&json.replace("গুরুত্বপূর্ণ সামরিক", "ড�"), "bn", span).is_err());
    }
}
