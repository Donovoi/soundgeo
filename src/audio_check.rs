//! Audio-only intake. Observations are not confirmed aircraft or location fixes.
use crate::{Control, Job, Progress, audio, timing};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

pub const DOPPLER_MIN_SECONDS: f64 = 20.;
pub const TIMING_MIN_SECONDS: f64 = 480.;
pub const MAX_SECONDS: f64 = 7200.;
pub const DOPPLER_MIN_SOUNDS: usize = 2;
pub const TIMING_MIN_SOUNDS: usize = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectorSettings {
    pub min_hz: f64,
    pub max_hz: f64,
    pub min_snr_db: f64,
    pub fast_flyovers: bool,
    pub fast_min_snr_db: f64,
}
impl From<&Job> for DetectorSettings {
    fn from(job: &Job) -> Self {
        Self {
            min_hz: job.min_hz,
            max_hz: job.max_hz,
            min_snr_db: job.min_snr_db,
            fast_flyovers: job.fast_flyovers,
            fast_min_snr_db: job.fast_min_snr_db,
        }
    }
}

/// A quick local stream inspection. Duration is an estimate until audio is decoded.
#[derive(Clone, Debug)]
pub struct MediaInfo {
    pub duration_s: Option<f64>,
    pub has_audio: bool,
}

pub fn probe(job: &Job, control: &Control) -> Result<MediaInfo> {
    ensure!(
        job.media.is_file(),
        "Choose an existing video or audio file."
    );
    let decoder = audio::ffmpeg_path(job);
    let sibling = decoder.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let tool = if sibling.is_file() {
        sibling
    } else {
        audio::bundled_tool("ffprobe").unwrap_or_else(|| "ffprobe".into())
    };
    let temporary = tempfile::tempdir()?;
    let output = temporary.path().join("streams.json");
    let errors = temporary.path().join("probe.log");
    let mut command = audio::quiet_command(tool);
    command
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_entries",
            "format=duration:stream=codec_type,duration",
            "-of",
            "json",
            "-i",
        ])
        .arg(&job.media);
    let status = crate::subprocess::run(
        &mut command,
        &output,
        &errors,
        control,
        std::time::Duration::from_secs(30),
        || Ok(()),
    )?;
    ensure!(
        status.success(),
        "Could not inspect recording streams: {}",
        crate::subprocess::tail(&errors)
    );
    let value: serde_json::Value = serde_json::from_reader(std::fs::File::open(output)?)?;
    let streams = value["streams"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Recording has no readable streams"))?;
    let duration = |value: &serde_json::Value| {
        value
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n > 0.)
    };
    let first_audio = streams.iter().find(|s| s["codec_type"] == "audio");
    Ok(MediaInfo {
        has_audio: first_audio.is_some(),
        duration_s: first_audio
            .and_then(|s| duration(&s["duration"]))
            .or_else(|| duration(&value["format"]["duration"])),
    })
}

/// Only a completed local inspection can construct reusable observations.
#[derive(Clone, Debug)]
pub struct Prepared {
    report: Report,
    media: std::path::PathBuf,
    ffmpeg: Option<std::path::PathBuf>,
}
impl Prepared {
    pub fn report(&self) -> &Report {
        &self.report
    }
    pub(crate) fn verify(&self, job: &Job, control: &Control) -> Result<()> {
        ensure!(
            self.media == job.media
                && self.ffmpeg == job.ffmpeg
                && self.report.detector_settings == DetectorSettings::from(job),
            "Audio settings changed; check the recording again before analysis."
        );
        ensure!(
            crate::visual::hash_file(&job.media, control)? == self.report.media_sha256,
            "Recording changed; check it again before analysis."
        );
        Ok(())
    }
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
    let job = Job {
        media: media.into(),
        ffmpeg: ffmpeg.map(Path::to_path_buf),
        offline: true,
        ..Default::default()
    };
    Ok(inspect_job(&job, control, progress)?.report)
}

/// Check the user's detector settings without needing or fetching flight data.
pub fn inspect_job(job: &Job, control: &Control, progress: &Progress<'_>) -> Result<Prepared> {
    let media = &job.media;
    ensure!(media.is_file(), "Choose an existing video or audio file.");
    let control = control.with_offline(true);
    let media_sha256 = crate::visual::hash_file(media, &control)?;
    progress("Extracting audio locally", 0.02);
    let samples = audio::decode(job, &control)?;
    let duration_s = samples.len() as f64 / audio::SAMPLE_RATE as f64;
    progress("Finding aircraft-like Doppler curves", 0.10);
    let events = audio::detect(&samples, job, &control, progress)?;
    progress("Finding broadband sound peaks", 0.85);
    let timing_peaks = timing::detect(&samples, &control)?;
    ensure!(
        crate::visual::hash_file(media, &control)? == media_sha256,
        "Recording changed during audio inspection; repeat with an unchanged file."
    );
    // Two observations can support the existing unit-rate Doppler screen.
    // Fitted clock drift and larger detection sets need more matching sounds.
    let doppler = method_check(
        events.len(),
        DOPPLER_MIN_SOUNDS,
        duration_s,
        DOPPLER_MIN_SECONDS,
    );
    let passage_timing = method_check(
        timing_peaks.len(),
        TIMING_MIN_SOUNDS,
        duration_s,
        TIMING_MIN_SECONDS,
    );
    let status =
        if doppler.enough_observations_to_search || passage_timing.enough_observations_to_search {
            "audio_observations_found"
        } else {
            "insufficient_evidence"
        };
    progress("Audio check complete", 1.0);
    let report = Report {
        version: env!("CARGO_PKG_VERSION").into(),
        method: "audio_only_intake".into(),
        status: status.into(),
        offline: true,
        media_sha256,
        duration_s,
        detector_settings: DetectorSettings::from(job),
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
    };
    Ok(Prepared {
        report,
        media: job.media.clone(),
        ffmpeg: job.ffmpeg.clone(),
    })
}

/// Preserve existing files, including media, aliases and earlier reports.
pub fn write_report(report: &impl serde::Serialize, output: &Path) -> Result<()> {
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
