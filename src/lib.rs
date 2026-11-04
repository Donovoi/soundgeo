pub mod audio;
pub mod data;
pub mod demo;
pub mod geo;
pub mod model;
pub mod search;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub media: PathBuf,
    pub tracks: PathBuf,
    pub terrain: Option<PathBuf>,
    pub start_utc: Option<f64>,
    pub time_tolerance_s: f64,
    pub bounds: Option<geo::Bounds>,
    pub ground_m: f64,
    pub temperature_c: f64,
    pub min_hz: f64,
    pub max_hz: f64,
    pub min_snr_db: f64,
    pub grid_m: f64,
    pub max_grid_points: usize,
    pub ffmpeg: Option<PathBuf>,
}
impl Default for Job {
    fn default() -> Self {
        Self {
            media: PathBuf::new(),
            tracks: PathBuf::new(),
            terrain: None,
            start_utc: None,
            time_tolerance_s: 60.,
            bounds: None,
            ground_m: 0.,
            temperature_c: 15.,
            min_hz: 40.,
            max_hz: 400.,
            min_snr_db: 10.,
            grid_m: 200.,
            max_grid_points: 6000,
            ffmpeg: None,
        }
    }
}
impl Job {
    pub fn validate(&self) -> Result<()> {
        if !self.media.is_file() {
            bail!("Choose an existing video or audio file.");
        }
        if !self.tracks.exists() {
            bail!("Choose a flight CSV, readsb trace, or archive folder.");
        }
        if !(self.min_hz.is_finite()
            && self.max_hz.is_finite()
            && self.min_hz >= 20.
            && self.max_hz <= 1500.
            && self.max_hz > self.min_hz + 10.)
        {
            bail!("Frequency range must be within 20–1500 Hz and at least 10 Hz wide.");
        }
        if !self.grid_m.is_finite() || !(50. ..=10000.).contains(&self.grid_m) {
            bail!("Grid spacing must be 50–10000 metres.");
        }
        if !(100..=50000).contains(&self.max_grid_points) {
            bail!("Grid budget must be 100–50000 points.");
        }
        if !self.temperature_c.is_finite() || !(-50. ..=60.).contains(&self.temperature_c) {
            bail!("Temperature must be -50 to 60 °C.");
        }
        if !self.ground_m.is_finite() || !(-500. ..=9000.).contains(&self.ground_m) {
            bail!("Ground elevation must be -500 to 9000 m.");
        }
        if !self.min_snr_db.is_finite() || !(6. ..=60.).contains(&self.min_snr_db) {
            bail!("Minimum signal contrast must be 6–60 dB.");
        }
        if !self.time_tolerance_s.is_finite() || !(0. ..=86400.).contains(&self.time_tolerance_s) {
            bail!("Time tolerance must be 0–86400 seconds.");
        }
        if self
            .start_utc
            .is_some_and(|t| !t.is_finite() || !(0. ..=4102444800.).contains(&t))
        {
            bail!("Recording start must be a UTC timestamp between 1970 and 2100.");
        }
        if let Some(b) = self.bounds {
            b.validate()?;
        }
        if let Some(p) = &self.terrain
            && !p.is_dir()
        {
            bail!("Terrain must be a folder containing SRTM .hgt files.");
        }
        Ok(())
    }
    pub fn sound_speed(&self) -> f64 {
        331.3 + 0.606 * self.temperature_c
    }
}

#[derive(Clone, Default)]
pub struct Control {
    pub cancel: Arc<AtomicBool>,
}
impl Control {
    pub fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!("Cancelled");
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
pub type Progress<'a> = dyn Fn(&str, f32) + Send + Sync + 'a;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub version: String,
    pub status: String,
    pub media: PathBuf,
    pub job: Job,
    pub duration_s: f64,
    pub track_count: usize,
    pub events: Vec<audio::Event>,
    pub candidates: Vec<search::Candidate>,
    pub warnings: Vec<String>,
    pub search_grid_m: f64,
    pub created_utc: String,
}

pub fn analyze(job: &Job, control: &Control, progress: &Progress<'_>) -> Result<Report> {
    job.validate()?;
    control.check()?;
    progress("Extracting audio locally", 0.02);
    let samples = audio::decode(job, control)?;
    let duration_s = samples.len() as f64 / audio::SAMPLE_RATE as f64;
    progress("Finding aircraft-like Doppler curves", 0.10);
    let events = audio::detect(&samples, job, control, progress)?;
    let mut report = Report { version: env!("CARGO_PKG_VERSION").into(), status: "insufficient_evidence".into(),
        media: job.media.clone(), job: job.clone(), duration_s, track_count: 0, events,
        candidates: vec![], warnings: vec!["Experimental: candidate rankings are not calibrated location probabilities or a verified fix.".into()],
        search_grid_m: job.grid_m, created_utc: chrono::Utc::now().to_rfc3339() };
    if report.events.is_empty() {
        report.warnings.push("No sufficiently strong, curved descending tone was detected. Try a longer outdoor recording or adjust the frequency band.".into());
        return Ok(report);
    }
    if report.events.len() == 1 && job.start_utc.is_none() {
        report.warnings.push("Only one flyover and no known clock: time and location can be ambiguous, especially on straight flight paths.".into());
    }
    progress("Reading historical flight paths", 0.36);
    let dataset = data::load_for_job(job, duration_s, control)?;
    report.track_count = dataset.tracks.len();
    report.warnings.extend(dataset.warnings);
    let mut terrain = geo::Terrain::new(job.terrain.clone(), job.ground_m);
    progress("Matching flyovers with a shared recording time", 0.42);
    let result = search::locate(
        job,
        &report.events,
        &dataset.tracks,
        &mut terrain,
        control,
        progress,
    )?;
    report.search_grid_m = result.coarse_grid_m;
    report.candidates = result.candidates;
    report.warnings.extend(result.warnings);
    if !report.candidates.is_empty() {
        report.status = if report.events.len() >= 2 {
            "candidates_found"
        } else {
            "ambiguous"
        }
        .into();
    }
    if terrain.missing > 0 {
        report.warnings.push(format!("{} candidate elevations used the fallback of {} m because terrain tiles were missing or void.", terrain.missing, job.ground_m));
    }
    report.warnings.push("Wind, variable engine pitch, moving microphones, edited audio, sparse flight coverage, and altitude error can invalidate matches. Silence is not treated as proof of aircraft absence.".into());
    progress("Analysis complete", 1.0);
    Ok(report)
}

pub fn save_report(report: &Report, path: &std::path::Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), report)?;
    temp.persist(path).context("Could not save the report")?;
    Ok(())
}
