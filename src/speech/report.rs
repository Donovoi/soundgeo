use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Complete,
    Partial,
    NoSpeech,
    NeedsLanguage,
    UnsupportedLanguage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub duration_s: f64,
    pub audio_stream: usize,
    pub decoded_sample_rate: u32,
    pub decoded_channels: u16,
    pub decoder_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Detection {
    pub start_s: f64,
    pub end_s: f64,
    pub language: String,
    /// Model score, not a calibrated probability of a correct identification.
    pub score: f64,
    pub alternatives: Vec<(String, f64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelUse {
    pub stage: String,
    pub id: String,
    pub revision: String,
    pub sha256: String,
    pub runtime: String,
    pub runtime_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub start_s: f64,
    pub end_s: f64,
    /// "asr_segment" or "speech_interval"; neither claims word-level alignment.
    pub timestamp_kind: String,
    pub language: String,
    pub original: String,
    pub english: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema: u32,
    #[serde(default = "unknown_version")]
    pub application_version: String,
    pub status: Status,
    pub source: Source,
    pub options: super::Options,
    pub detected: Vec<Detection>,
    pub selected_language: Option<String>,
    pub models: Vec<ModelUse>,
    pub segments: Vec<Segment>,
    pub warnings: Vec<String>,
    pub created_utc: String,
}

fn unknown_version() -> String {
    "unknown".into()
}

impl Report {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 1, "Unsupported speech report schema");
        ensure!(
            self.source.duration_s.is_finite() && self.source.duration_s > 0.,
            "Invalid speech duration"
        );
        ensure!(
            self.source.sha256.len() == 64
                && self.source.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid speech source fingerprint"
        );
        let mut previous_start = 0.;
        for segment in &self.segments {
            ensure!(
                segment.start_s.is_finite()
                    && segment.end_s.is_finite()
                    && segment.start_s >= previous_start
                    && segment.end_s > segment.start_s
                    && segment.end_s <= self.source.duration_s + 0.05,
                "Speech segment timestamps are outside the source recording or out of order"
            );
            ensure!(!segment.original.trim().is_empty(), "Empty speech segment");
            previous_start = segment.start_s;
        }
        Ok(())
    }

    pub fn export(&self, format: ExportFormat, english: bool) -> Result<String> {
        self.validate()?;
        if format == ExportFormat::Json {
            return Ok(serde_json::to_string_pretty(self)? + "\n");
        }
        let mut output = if format == ExportFormat::Vtt {
            "WEBVTT\n\n".into()
        } else {
            String::new()
        };
        for (index, segment) in self.segments.iter().enumerate() {
            let text = if english {
                segment.english.as_deref().filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| anyhow::anyhow!("English translation is incomplete at {:.2}s; export the original transcript or JSON", segment.start_s))?
            } else {
                &segment.original
            };
            match format {
                ExportFormat::Txt => output.push_str(&format!(
                    "[{} – {}] {}\n",
                    timestamp(segment.start_s, false),
                    timestamp(segment.end_s, false),
                    text
                )),
                ExportFormat::Srt | ExportFormat::Vtt => {
                    // Keep evidence text verbatim in JSON. Subtitle cues need escaped
                    // markup and no blank-line injection from a model's output.
                    let text = text
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    let comma = format == ExportFormat::Srt;
                    output.push_str(&format!(
                        "{}\n{} --> {}\n{}\n\n",
                        index + 1,
                        timestamp(segment.start_s, comma),
                        timestamp(segment.end_s, comma),
                        text
                    ));
                }
                ExportFormat::Json => unreachable!(),
            }
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum ExportFormat {
    Json,
    Txt,
    Srt,
    Vtt,
}

fn timestamp(seconds: f64, comma: bool) -> String {
    let ms = (seconds * 1000.).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        if comma { ',' } else { '.' },
        ms % 1000
    )
}

/// Export with no-clobber semantics, including when the target is a symlink.
pub fn write_report(
    report: &Report,
    path: &Path,
    format: ExportFormat,
    english: bool,
) -> Result<()> {
    let text = report.export(format, english)?;
    let (mut file, destination) = crate::atomic_file::prepare(path)?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist_noclobber(destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        Report {
            schema: 1,
            status: Status::Complete,
            application_version: env!("CARGO_PKG_VERSION").into(),
            source: Source {
                path: "recording.wav".into(),
                sha256: "a".repeat(64),
                size: 1,
                duration_s: 61.,
                audio_stream: 0,
                decoded_sample_rate: 16000,
                decoded_channels: 1,
                decoder_version: "fixture".into(),
            },
            options: Default::default(),
            detected: vec![],
            selected_language: Some("ur".into()),
            models: vec![],
            segments: vec![Segment {
                start_s: 59.9996,
                end_s: 60.5,
                timestamp_kind: "speech_interval".into(),
                language: "ur".into(),
                original: "لاہور <i>\n\nپاکستان".into(),
                english: Some("Lahore & Pakistan".into()),
                warnings: vec![],
            }],
            warnings: vec![],
            created_utc: "2026-11-25T00:00:00Z".into(),
        }
    }

    #[test]
    fn exports_preserve_original_unicode_and_source_time() {
        let report = report();
        let json = report.export(ExportFormat::Json, true).unwrap();
        let roundtrip: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.segments[0].original, report.segments[0].original);
        let srt = report.export(ExportFormat::Srt, false).unwrap();
        assert!(srt.contains("00:01:00,000 --> 00:01:00,500"));
        assert!(srt.contains("لاہور &lt;i&gt; پاکستان"));
        assert!(
            report
                .export(ExportFormat::Vtt, true)
                .unwrap()
                .starts_with("WEBVTT\n\n1\n00:01:00.000")
        );
    }

    #[test]
    fn incomplete_translation_never_masquerades_as_english() {
        let mut report = report();
        report.status = Status::Partial;
        report.segments[0].english = None;
        assert!(report.export(ExportFormat::Srt, true).is_err());
        assert!(report.export(ExportFormat::Srt, false).is_ok());
        assert!(report.export(ExportFormat::Json, true).is_ok());
    }

    #[test]
    fn exports_reject_invalid_timestamps_and_preserve_existing_files() {
        let mut report = report();
        report.segments[0].end_s = 100.;
        assert!(report.export(ExportFormat::Json, false).is_err());
        report.segments[0].end_s = 60.5;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("evidence.json");
        std::fs::write(&path, b"existing").unwrap();
        assert!(write_report(&report, &path, ExportFormat::Json, false).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"existing");
    }
}
