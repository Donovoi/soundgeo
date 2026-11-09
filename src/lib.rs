pub mod aircraft;
pub mod archive;
pub mod audio;
pub mod availability;
pub mod data;
mod date_search;
pub mod demo;
pub mod doppler_controls;
pub mod geo;
pub mod index;
pub mod model;
mod network;
pub mod sample_archive;
pub mod search;
pub mod terrain_archive;
pub mod timing;
pub mod visual;
pub mod visual_data;
pub mod webtrak;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMethod {
    #[default]
    Doppler,
    PassageTiming,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub analysis_method: AnalysisMethod,
    pub timing_options: timing::Options,
    pub media: PathBuf,
    pub offline: bool,
    pub tracks: PathBuf,
    pub archive: Option<archive::ArchiveRequest>,
    pub terrain: Option<PathBuf>,
    pub download_terrain: bool,
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
    /// Maximum corridor workers; absent keeps the existing four-worker limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_threads: Option<usize>,
    pub silence_mode: search::SilenceMode,
}
impl Default for Job {
    fn default() -> Self {
        Self {
            analysis_method: AnalysisMethod::default(),
            timing_options: timing::Options::default(),
            media: PathBuf::new(),
            offline: false,
            tracks: PathBuf::new(),
            archive: None,
            terrain: None,
            download_terrain: false,
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
            search_threads: None,
            silence_mode: Default::default(),
        }
    }
}
impl Job {
    pub fn validate(&self) -> Result<()> {
        if self.analysis_method == AnalysisMethod::PassageTiming && self.bounds.is_none() {
            bail!(
                "Passage timing needs a search area. Set its south, north, west and east bounds."
            );
        }
        if !self.media.is_file() {
            bail!("Choose an existing video or audio file.");
        }
        if let Some(request) = &self.archive {
            request.validate()?;
            if request.provider.sample_resolution().is_some() {
                sample_archive::validate_bounds(self.bounds)?;
                if !request.date_from.trim().is_empty() {
                    for date in request.dates()? {
                        sample_archive::validate(&date, self.bounds)?;
                    }
                }
            }
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
        if self.search_threads == Some(0) {
            bail!("Flight-corridor workers must be at least 1.");
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
        if self.download_terrain {
            let bounds = self
                .bounds
                .context("Set a search area to download terrain")?;
            terrain_archive::tiles(bounds)?;
            if self.terrain.is_none() {
                bail!("Choose a terrain cache folder for automatic downloads");
            }
        }
        if !self.download_terrain
            && let Some(p) = &self.terrain
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
    pub offline: bool,
}
impl Control {
    pub fn require_network(&self, resource: &str) -> Result<()> {
        self.check()?;
        if self.offline {
            bail!(
                "Offline mode: missing or invalid cached data: {resource}. Prepare this data while online, then retry."
            );
        }
        Ok(())
    }
    pub fn with_offline(&self, offline: bool) -> Self {
        Self {
            cancel: self.cancel.clone(),
            offline: self.offline || offline,
        }
    }
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
    #[serde(default)]
    pub terrain_sources: Vec<terrain_archive::TileRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<timing::Assessment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_search: Option<date_search::Coverage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doppler_partitions: Vec<index::ProposalPartition>,
}

impl Report {
    pub fn candidate_count(&self) -> usize {
        self.timing
            .as_ref()
            .map_or(self.candidates.len(), timing::Assessment::candidate_count)
    }
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
    let control = &control.with_offline(effective.offline);
    let job = &effective;
    job.validate()?;
    control.check()?;
    progress("Extracting audio locally", 0.02);
    let samples = audio::decode(job, control)?;
    let duration_s = samples.len() as f64 / audio::SAMPLE_RATE as f64;
    let (events, timing_peaks) = match job.analysis_method {
        AnalysisMethod::Doppler => {
            progress("Finding aircraft-like Doppler curves", 0.10);
            (audio::detect(&samples, job, control, progress)?, None)
        }
        AnalysisMethod::PassageTiming => {
            progress("Finding broadband sound peaks", 0.10);
            (vec![], Some(timing::detect(&samples, control)?))
        }
    };
    drop(samples);
    let mut report = Report { version: env!("CARGO_PKG_VERSION").into(), status: "insufficient_evidence".into(),
        media: job.media.clone(), job: job.clone(), duration_s, track_count: 0, events,
        candidates: vec![], warnings: vec!["Experimental: candidate rankings are not calibrated location probabilities or a verified fix.".into()],
        search_grid_m: job.grid_m, search_method:String::new(),hypotheses:0,created_utc: chrono::Utc::now().to_rfc3339(),reference_sources:vec![],terrain_sources:vec![],timing:None,date_search:None,doppler_partitions:vec![] };
    if let Some(peaks) = &timing_peaks
        && (peaks.len() < 6 || duration_s < 480.)
    {
        report.search_method = "passage_timing".into();
        report.search_grid_m = job.timing_options.grid_m;
        report.timing = Some(timing::Assessment::pending(peaks.clone()));
        report.warnings.push("Passage timing needs at least six sound peaks and eight minutes of continuous audio. Flight downloads were not needed for this result.".into());
        progress("Analysis complete", 1.0);
        return Ok(report);
    }
    if timing_peaks.is_none() && report.events.is_empty() {
        report.warnings.push("No sufficiently strong, curved descending tone was detected. Try a longer outdoor recording or adjust the frequency band.".into());
        return Ok(report);
    }
    if report.events.len() == 1 && job.start_utc.is_none() {
        report.warnings.push("Only one flyover and no known clock: time and location can be ambiguous, especially on straight flight paths.".into());
    }
    if let Some(request) = &job.archive {
        let (dates, discovery) = request.resolve_dates(control, &|m, _| progress(m, 0.33))?;
        if dates.len() > 1 || discovery.is_some() {
            let mut result =
                date_search::run(job, report, timing_peaks, &dates, control, progress)?;
            result.date_search.as_mut().unwrap().provider_discovery = discovery;
            return Ok(result);
        }
    }
    let sources = if let Some(request) = &job.archive {
        if let Some(site) = request.provider.webtrak_site() {
            report.warnings.push(format!("{} public airport data covers its local radar area. Altitudes are converted from MSL to WGS84 using EGM96; their original sensor/pressure provenance is unknown. Recorded coverage does not establish that every flight is present.",site.name()));
        }
        request.ensure_for_bounds(job.bounds, control, &|m, p| progress(m, 0.34 + p * 0.02))?
    } else {
        vec![job.tracks.clone()]
    };
    analyze_sources(
        job,
        report,
        timing_peaks,
        &sources,
        false,
        control,
        progress,
    )
}

pub(crate) fn analyze_sources(
    job: &Job,
    mut report: Report,
    timing_peaks: Option<Vec<timing::Peak>>,
    sources: &[PathBuf],
    all_controls: bool,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Report> {
    let duration_s = report.duration_s;
    if let Some(site) = job
        .archive
        .as_ref()
        .and_then(|request| request.provider.webtrak_site())
    {
        for path in sources {
            if let Some(warning) = webtrak::coverage_warning(site, path)? {
                report.warnings.push(warning);
            }
        }
    }
    let mut prepared = job.clone();
    if job
        .archive
        .as_ref()
        .is_some_and(|r| r.provider.sample_resolution().is_some())
    {
        let mut no_traces = !sources.is_empty();
        for path in sources {
            let (warning, empty) = sample_archive::coverage(path)?;
            report.warnings.push(warning);
            no_traces &= empty;
        }
        if no_traces {
            report.reference_sources = sources.to_vec();
            // Empty date batches contribute no controls. The combined search
            // creates a pending assessment if every batch is empty.
            if !all_controls && let Some(peaks) = timing_peaks {
                report.search_method = "passage_timing".into();
                report.timing = Some(timing::Assessment::pending(peaks));
            }
            return Ok(report);
        }
    }
    if prepared.type_catalog.is_none() {
        prepared.type_catalog = sources
            .iter()
            .filter_map(|p| p.parent())
            .map(|p| p.join("aircraft-types.json"))
            .find(|p| p.is_file());
    }
    let job = &prepared;
    report.job = job.clone();
    report.reference_sources = sources.to_vec();
    progress("Reading historical flight paths", 0.36);
    let dataset = match data::load_for_job_sources(job, sources, duration_s, control) {
        Ok(data) => data,
        Err(error) if all_controls && error.is::<data::EmptyFlightData>() => {
            report.warnings.push(error.to_string());
            return Ok(report);
        }
        Err(error) => return Err(error),
    };
    report.track_count = dataset.tracks.len();
    report.warnings.extend(dataset.warnings);
    if job.download_terrain {
        report.terrain_sources = terrain_archive::ensure(
            job.bounds.context("Terrain download needs search bounds")?,
            job.terrain
                .as_deref()
                .context("Terrain download needs a cache folder")?,
            control,
            &|m, p| progress(m, 0.40 + p * 0.02),
        )?;
        report.warnings.push("Terrain: Mapzen Terrain Tiles (https://registry.opendata.aws/terrain-tiles/). Provider credits are in TERRAIN-ATTRIBUTION.txt in the terrain cache. Bare-earth elevation does not include microphone height above the ground.".into());
    }
    let mut terrain = geo::Terrain::with_datum(job.terrain.clone(), job.ground_m, job.ground_datum);
    if let Some(peaks) = timing_peaks {
        let assess = if all_controls {
            timing::assess_all_controls
        } else {
            timing::assess
        };
        let assessment = assess(
            job,
            peaks,
            duration_s,
            &dataset.tracks,
            &job.timing_options,
            control,
            &|m, p| progress(m, 0.42 + p * 0.57),
        )?;
        report.search_method = "passage_timing".into();
        report.search_grid_m = job.timing_options.grid_m;
        report.hypotheses = assessment
            .search
            .as_ref()
            .map_or(0, |s| s.likelihood_evaluations);
        report.hypotheses += assessment.control_likelihood_evaluations;
        if assessment
            .search
            .as_ref()
            .is_some_and(|s| s.terrain_fallback_points > 0)
        {
            report.warnings.push(format!("Missing/void terrain used the configured fallback height of {} m ({:?}). Candidate heights are WGS84 ellipsoid heights.",job.ground_m,job.ground_datum));
        }
        report.status = match assessment.screen {
            timing::Screen::InsufficientEvidence => "insufficient_evidence",
            timing::Screen::Ambiguous => "ambiguous",
            timing::Screen::TimingCandidate => "candidates_found",
        }
        .into();
        report.warnings.push(assessment.explanation.clone());
        report.warnings.push(format!("Passage timing uses nearby aircraft, a fixed media clock rate and a timing width of {} seconds. Speech, missing flights, changing sound power and nonstationary noise can invalidate this model. See the limitations document for result interpretation.", job.timing_options.timing_sigma_s));
        report.timing = Some(assessment);
        progress("Analysis complete", 1.);
        return Ok(report);
    }
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
    report.doppler_partitions.extend(result.partition);
    set_doppler_status(&mut report);
    if terrain.missing > 0 {
        report.warnings.push(format!("Missing/void terrain used the configured fallback height of {} m ({:?}). Candidate heights are WGS84 ellipsoid heights.", job.ground_m, job.ground_datum));
    }
    report.warnings.push("Wind, variable engine pitch, moving microphones, edited audio, sparse flight coverage, and altitude error can invalidate matches. Quiet-gap constraints depend on an explicit audibility assumption.".into());
    progress("Analysis complete", 1.0);
    Ok(report)
}

pub(crate) fn set_doppler_status(report: &mut Report) {
    if !report.candidates.is_empty() {
        let provisional_clock = report.candidates[0].clock_fit_is_provisional();
        if provisional_clock {
            report.warnings.push("The fitted recording clock rate has only the minimum three supporting sounds. The location and time remain provisional; more independent evidence is required.".into());
        }
        let ambiguous = report.events.len() < 2
            || provisional_clock
            || report.candidates[0].matches.len() < report.events.len()
            || search::has_competing_fit(&report.candidates);
        report.status = if !ambiguous {
            "candidates_found"
        } else {
            "ambiguous"
        }
        .into();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreparedData {
    pub flight_sources: Vec<PathBuf>,
    pub terrain_sources: Vec<terrain_archive::TileRecord>,
}

pub fn prepare_data(job: &Job, control: &Control, progress: &Progress<'_>) -> Result<PreparedData> {
    let control = &control.with_offline(job.offline);
    control.check()?;
    if let Some(bounds) = job.bounds {
        bounds.validate()?;
    }
    let terrain = if job.download_terrain {
        let bounds = job.bounds.context("Set search bounds to prepare terrain")?;
        terrain_archive::tiles(bounds)?;
        Some((
            bounds,
            job.terrain
                .as_ref()
                .context("Choose a terrain cache folder")?,
        ))
    } else {
        None
    };
    let request = job
        .archive
        .as_ref()
        .context("Select a flight provider and cache folder")?;
    if request.provider.sample_resolution().is_some() {
        sample_archive::validate_bounds(job.bounds)?;
    }
    let flight_sources =
        request.ensure_for_bounds(job.bounds, control, &|m, p| progress(m, p * 0.9))?;
    let terrain_sources = if let Some((bounds, folder)) = terrain {
        terrain_archive::ensure(bounds, folder, control, &|m, p| progress(m, 0.9 + p * 0.1))?
    } else {
        vec![]
    };
    progress("Data ready for offline analysis", 1.);
    Ok(PreparedData {
        flight_sources,
        terrain_sources,
    })
}

pub fn save_report(report: &Report, path: &std::path::Path) -> Result<()> {
    if let Ok(target) = path.canonicalize() {
        for input in [&report.media, &report.job.tracks]
            .into_iter()
            .chain(report.job.type_catalog.iter())
            .chain(report.job.terrain.iter())
            .chain(report.reference_sources.iter())
            .chain(report.job.archive.iter().map(|a| &a.cache))
        {
            if let Ok(source) = input.canonicalize()
                && (target == source || (source.is_dir() && target.starts_with(&source)))
            {
                bail!(
                    "A report cannot overwrite an input recording, flight-data file, terrain file, or catalog. Choose a separate output path."
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
