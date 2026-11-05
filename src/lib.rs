pub mod aircraft;
pub mod archive;
pub mod audio;
pub mod data;
pub mod demo;
pub mod geo;
pub mod index;
pub mod model;
mod network;
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
    pub archive: Option<archive::ArchiveRequest>,
    pub terrain: Option<PathBuf>,
    pub start_utc: Option<f64>,
    pub time_tolerance_s: f64,
    /// Maximum shared media-clock rate error; zero fixes UTC/media seconds 1:1.
    pub max_clock_drift_ppm: f64,
    pub bounds: Option<geo::Bounds>,
    pub ground_m: f64,
    pub ground_datum: geo::ElevationDatum,
    pub smooth_tracks: bool,
    pub temperature_c: f64,
    pub min_hz: f64,
    pub max_hz: f64,
    pub min_snr_db: f64,
    pub fast_flyovers: bool,
    pub fast_min_snr_db: f64,
    pub grid_m: f64,
    pub max_grid_points: usize,
    pub ffmpeg: Option<PathBuf>,
    pub aircraft_filter: aircraft::AircraftFilter,
    pub type_catalog: Option<PathBuf>,
    pub search_mode: search::SearchMode,
    pub max_hypotheses: usize,
    pub silence_mode: search::SilenceMode,
}
impl Default for Job {
    fn default() -> Self {
        Self {
            media: PathBuf::new(),
            tracks: PathBuf::new(),
            archive: None,
            terrain: None,
            start_utc: None,
            time_tolerance_s: 60.,
            max_clock_drift_ppm: 5000.,
            bounds: None,
            ground_m: 0.,
            ground_datum: Default::default(),
            smooth_tracks: true,
            temperature_c: 15.,
            min_hz: 40.,
            max_hz: 400.,
            min_snr_db: 10.,
            fast_flyovers: true,
            fast_min_snr_db: 8.,
            grid_m: 200.,
            max_grid_points: 6000,
            ffmpeg: None,
            aircraft_filter: Default::default(),
            type_catalog: None,
            search_mode: Default::default(),
            max_hypotheses: 5_000_000,
            silence_mode: Default::default(),
        }
    }
}
impl Job {
    pub fn validate(&self) -> Result<()> {
        if !self.media.is_file() {
            bail!("Choose an existing video or audio file.");
        }
        if let Some(request) = &self.archive {
            request.dates()?;
        } else if !self.tracks.exists() {
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
        if !(100..=100_000_000).contains(&self.max_hypotheses) {
            bail!("Hypothesis budget must be 100–100,000,000");
        }
        if let Some(path) = &self.type_catalog
            && !path.is_file()
        {
            bail!("Aircraft type catalog not found");
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
        if !self.fast_min_snr_db.is_finite() || !(6. ..=60.).contains(&self.fast_min_snr_db) {
            bail!("Short-pass signal contrast must be 6–60 dB.");
        }
        if !self.time_tolerance_s.is_finite() || !(0. ..=86400.).contains(&self.time_tolerance_s) {
            bail!("Time tolerance must be 0–86400 seconds.");
        }
        if !self.max_clock_drift_ppm.is_finite()
            || !(0. ..=5000.).contains(&self.max_clock_drift_ppm)
        {
            bail!("Media clock drift limit must be 0–5000 ppm.");
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
    #[serde(default)]
    pub search_method: String,
    #[serde(default)]
    pub hypotheses: usize,
    pub created_utc: String,
    #[serde(default)]
    pub reference_sources: Vec<PathBuf>,
}

pub fn analyze(job: &Job, control: &Control, progress: &Progress<'_>) -> Result<Report> {
    let mut effective = job.clone();
    if effective.type_catalog.is_none() {
        let parent = if effective.tracks.is_dir() {
            Some(effective.tracks.as_path())
        } else {
            effective.tracks.parent()
        };
        if let Some(path) = parent.map(|p| p.join("aircraft-types.json"))
            && path.is_file()
        {
            effective.type_catalog = Some(path);
        }
    }
    let job = &effective;
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
        search_grid_m: job.grid_m, search_method:String::new(),hypotheses:0,created_utc: chrono::Utc::now().to_rfc3339(),reference_sources:vec![] };
    if report.events.is_empty() {
        report.warnings.push("No sufficiently strong, curved descending tone was detected. Try a longer outdoor recording or adjust the frequency band.".into());
        return Ok(report);
    }
    if report.events.len() == 1 && job.start_utc.is_none() {
        report.warnings.push("Only one flyover and no known clock: time and location can be ambiguous, especially on straight flight paths.".into());
    }
    let sources = if let Some(request) = &job.archive {
        request.ensure(control, &|m, p| progress(m, 0.34 + p * 0.02))?
    } else {
        vec![job.tracks.clone()]
    };
    let mut prepared = job.clone();
    if prepared.type_catalog.is_none() {
        prepared.type_catalog = sources
            .iter()
            .filter_map(|p| p.parent())
            .map(|p| p.join("aircraft-types.json"))
            .find(|p| p.is_file());
    }
    let job = &prepared;
    report.job = job.clone();
    report.reference_sources = sources.clone();
    progress("Reading historical flight paths", 0.36);
    let dataset = data::load_for_job_sources(job, &sources, duration_s, control)?;
    report.track_count = dataset.tracks.len();
    report.warnings.extend(dataset.warnings);
    let mut terrain = geo::Terrain::with_datum(job.terrain.clone(), job.ground_m, job.ground_datum);
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
    report.search_method = result.method;
    report.hypotheses = result.hypotheses;
    report.candidates = result.candidates;
    report.warnings.extend(result.warnings);
    if !report.candidates.is_empty() {
        let ambiguous = report.events.len() < 2
            || report.candidates[0].matches.len() < report.events.len()
            || (report.candidates.len() > 1
                && report.candidates[1].relative_error_percent
                    < report.candidates[0].relative_error_percent * 1.5 + 0.05);
        report.status = if !ambiguous {
            "candidates_found"
        } else {
            "ambiguous"
        }
        .into();
    }
    if terrain.missing > 0 {
        report.warnings.push(format!("Missing/void terrain used the configured fallback height of {} m ({:?}). Candidate heights are WGS84 ellipsoid heights.", job.ground_m, job.ground_datum));
    }
    report.warnings.push("Wind, variable engine pitch, moving microphones, edited audio, sparse flight coverage, and altitude error can invalidate matches. Quiet-gap constraints depend on an explicit audibility assumption.".into());
    progress("Analysis complete", 1.0);
    Ok(report)
}

pub fn save_report(report: &Report, path: &std::path::Path) -> Result<()> {
    if let Ok(target) = path.canonicalize() {
        for input in [&report.media, &report.job.tracks]
            .into_iter()
            .chain(report.job.type_catalog.iter())
            .chain(report.reference_sources.iter())
            .chain(report.job.archive.iter().map(|a| &a.cache))
        {
            if let Ok(source) = input.canonicalize()
                && (target == source || (source.is_dir() && target.starts_with(&source)))
            {
                bail!(
                    "A report cannot overwrite an input recording, flight-data file, or catalog. Choose a separate output path."
                );
            }
            // A split .aa archive reads all sibling parts. Protect those too.
            if input.to_string_lossy().ends_with(".tar.aa")
                && let (Some(parent), Some(name)) = (input.parent(), input.file_name())
                && let Ok(parent) = parent.canonicalize()
                && target.parent() == Some(parent.as_path())
            {
                let prefix = name.to_string_lossy().trim_end_matches("aa").to_owned();
                if target
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
                {
                    bail!("A report cannot overwrite a split flight-archive part");
                }
            }
        }
    }
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
