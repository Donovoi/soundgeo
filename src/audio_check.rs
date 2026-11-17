//! Audio-only intake. Observations are not confirmed aircraft or location fixes.
use crate::{Control, Job, Progress, audio, timing};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectorSettings {
    pub min_hz: f64,
    pub max_hz: f64,
    pub min_snr_db: f64,
    pub fast_flyovers: bool,
    pub fast_min_snr_db: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MethodCheck {
    pub enough_observations_to_search: bool,
    pub observation_count: usize,
    pub minimum_observations: usize,
    pub minimum_duration_s: f64,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub version: String,
    pub method: String,
    pub status: String,
    pub offline: bool,
    pub media_sha256: String,
    pub duration_s: f64,
    pub detector_settings: DetectorSettings,
    pub doppler: MethodCheck,
    pub passage_timing: MethodCheck,
    pub events: Vec<audio::Event>,
    pub timing_peaks: Vec<timing::Peak>,
    pub warnings: Vec<String>,
}

fn method_check(count: usize, minimum: usize, duration: f64, minimum_duration: f64) -> MethodCheck {
    let mut reasons = Vec::new();
    if count < minimum {
        reasons.push("too_few_observations".into());
    }
    if duration < minimum_duration {
        reasons.push("recording_too_short".into());
    }
    MethodCheck {
        enough_observations_to_search: reasons.is_empty(),
        observation_count: count,
        minimum_observations: minimum,
        minimum_duration_s: minimum_duration,
        reasons,
    }
}

/// Uses the same default detectors as analysis, with no dates, coordinates or references.
pub fn inspect(
    media: &Path,
    ffmpeg: Option<&Path>,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Report> {
    ensure!(media.is_file(), "Choose an existing video or audio file.");
    let control = control.with_offline(true);
    let job = Job {
        media: media.into(),
        ffmpeg: ffmpeg.map(Path::to_path_buf),
        offline: true,
        ..Default::default()
    };
    let media_sha256 = crate::visual::hash_file(media, &control)?;
    progress("Extracting audio locally", 0.02);
    let samples = audio::decode(&job, &control)?;
    let duration_s = samples.len() as f64 / audio::SAMPLE_RATE as f64;
    progress("Finding aircraft-like Doppler curves", 0.10);
    let events = audio::detect(&samples, &job, &control, progress)?;
    progress("Finding broadband sound peaks", 0.85);
    let timing_peaks = timing::detect(&samples, &control)?;
    ensure!(
        crate::visual::hash_file(media, &control)? == media_sha256,
        "Recording changed during audio inspection; repeat with an unchanged file."
    );
    // Two observations can support the existing unit-rate Doppler screen.
    // Fitted clock drift and larger detection sets need more matching sounds.
    let doppler = method_check(events.len(), 2, duration_s, 20.);
    let passage_timing = method_check(timing_peaks.len(), 6, duration_s, 480.);
    let status =
        if doppler.enough_observations_to_search || passage_timing.enough_observations_to_search {
            "audio_observations_found"
        } else {
            "insufficient_evidence"
        };
    progress("Audio check complete", 1.0);
    Ok(Report {
        version: env!("CARGO_PKG_VERSION").into(),
        method: "audio_only_intake".into(),
        status: status.into(),
        offline: true,
        media_sha256,
        duration_s,
        detector_settings: DetectorSettings {
            min_hz: job.min_hz, max_hz: job.max_hz, min_snr_db: job.min_snr_db,
            fast_flyovers: job.fast_flyovers, fast_min_snr_db: job.fast_min_snr_db,
        },
        doppler,
        passage_timing,
        events,
        timing_peaks,
        warnings: vec![
            "Audio-only check: no flight data, dates, coordinates or location search were used.".into(),
            "Detected tones and peaks can come from speech, music or other sounds; they are not confirmed aircraft.".into(),
            "Enough observations permits a search; it does not establish a location or 1 km accuracy. Matching, consensus and competitor checks still apply. Fitted clock drift needs at least four matched sounds.".into(),
            "Review the source for edits, dubbed audio and microphone movement. Publication dates and file creation timestamps do not establish the recording date.".into(),
        ],
    })
}

/// Preserve existing files, including media, aliases and earlier reports.
pub fn write_report(report: &Report, output: &Path) -> Result<()> {
    if output == Path::new("-") {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, report)?;
        writeln!(stdout)?;
    } else {
        let (mut temp, destination) = crate::atomic_file::prepare(output)?;
        serde_json::to_writer_pretty(&mut temp, report)?;
        writeln!(temp)?;
        temp.flush()?;
        temp.persist_noclobber(destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_clip_cannot_pass_timing_even_with_many_peaks() {
        let check = method_check(12, 6, 27.39, 480.);
        assert!(!check.enough_observations_to_search);
        assert_eq!(check.reasons, ["recording_too_short"]);
        assert!(method_check(6, 6, 480., 480.).enough_observations_to_search);
        assert!(!method_check(5, 6, 3600., 480.).enough_observations_to_search);
        assert!(!method_check(1, 2, 30., 20.).enough_observations_to_search);
    }

    #[test]
    fn silent_media_needs_no_references_and_outputs_cannot_replace_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("silence.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&media, spec).unwrap();
        for _ in 0..8000 * 20 {
            wav.write_sample(0i16).unwrap();
        }
        wav.finalize().unwrap();
        let original = std::fs::read(&media).unwrap();
        let report = inspect(&media, None, &Control::default(), &|_, _| {}).unwrap();
        assert!(report.offline);
        assert_eq!(report.status, "insufficient_evidence");
        assert_eq!(report.duration_s, 20.);
        assert!(report.events.is_empty() && report.timing_peaks.is_empty());
        assert!(!report.doppler.enough_observations_to_search);
        assert!(!report.passage_timing.enough_observations_to_search);
        assert!(write_report(&report, &media).is_err());
        assert_eq!(std::fs::read(&media).unwrap(), original);
        let output = dir.path().join("report.json");
        write_report(&report, &output).unwrap();
        let saved = std::fs::read(&output).unwrap();
        let restored: Report = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.media_sha256, report.media_sha256);
        assert!(write_report(&report, &output).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), saved);
    }
}
