//! Early input requirements and the local audio check, before flight downloads.
use super::{ACCENT, App, MUTED, Message, PANEL, ResponseHelp};
use eframe::egui::{self, RichText};
use soundgeo::{
    AnalysisMethod, Job,
    audio_check::{self, MediaInfo, Prepared},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct State {
    observed: Option<String>,
    changed: Option<Instant>,
    requested: Option<String>,
    pub running: bool,
    pub info: Option<MediaInfo>,
    pub prepared: Option<Arc<Prepared>>,
    pub error: Option<String>,
}

pub(super) fn key(job: &Job) -> String {
    let metadata = job.media.metadata().ok();
    format!(
        "{:?}|{:?}|{:?}|{}",
        job.media,
        job.ffmpeg,
        metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .zip(metadata.as_ref().map(|m| m.len())),
        serde_json::to_string(&audio_check::DetectorSettings::from(job)).unwrap()
    )
}

impl State {
    pub fn current(&self, job: &Job) -> bool {
        self.requested.as_ref() == Some(&key(job))
    }
    pub fn complete(&self, job: &Job) -> bool {
        self.current(job) && !self.running && (self.prepared.is_some() || self.error.is_some())
    }
    pub fn prepared(&self, job: &Job) -> Option<Arc<Prepared>> {
        self.current(job).then(|| self.prepared.clone()).flatten()
    }
    pub fn problem(&self, job: &Job) -> Option<String> {
        if !self.current(job) {
            return None;
        }
        if let Some(error) = &self.error {
            return Some(error.clone());
        }
        let report = self.prepared.as_ref()?.report();
        let (name, check) = match job.analysis_method {
            AnalysisMethod::Doppler => ("Doppler curves", &report.doppler),
            AnalysisMethod::PassageTiming => ("Passage timing", &report.passage_timing),
        };
        if check.enough_observations_to_search {
            return None;
        }
        let mut reasons = Vec::new();
        if report.duration_s < check.minimum_duration_s {
            reasons.push(format!(
                "{} of audio; needs at least {}",
                duration(report.duration_s),
                minimum_duration(check.minimum_duration_s)
            ));
        }
        if check.observation_count < check.minimum_observations {
            reasons.push(format!(
                "{} detected sounds; needs at least {}",
                check.observation_count, check.minimum_observations
            ));
        }
        Some(format!(
            "{name} cannot run: {}. No flight data will download for this search.",
            reasons.join("; ")
        ))
    }
    pub fn begin(&mut self, key: String) {
        self.requested = Some(key);
        self.running = true;
        self.info = None;
        self.prepared = None;
        self.error = None;
    }
    pub fn finish(&mut self, result: Result<Arc<Prepared>, String>) {
        self.running = false;
        match result {
            Ok(value) => self.prepared = Some(value),
            Err(error) => self.error = Some(error),
        }
    }
}

fn duration(seconds: f64) -> String {
    let seconds = seconds.floor() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
fn minimum_duration(seconds: f64) -> String {
    if seconds < 60. {
        format!("{} seconds", seconds as u64)
    } else {
        format!("{} minutes", (seconds / 60.) as u64)
    }
}

impl App {
    pub(super) fn schedule_readiness(&mut self, ctx: &egui::Context) {
        if self.busy() || !self.job.media.is_file() {
            return;
        }
        let identity = key(&self.job);
        if self.readiness.observed.as_ref() != Some(&identity) {
            self.readiness = State {
                observed: Some(identity.clone()),
                changed: Some(Instant::now()),
                ..Default::default()
            };
        }
        if self.readiness.current(&self.job) {
            return;
        }
        if self
            .readiness
            .changed
            .is_some_and(|changed| changed.elapsed() < Duration::from_millis(500))
        {
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }
        self.readiness.begin(identity.clone());
        let job = self.job.clone();
        self.worker(ctx, "Checking recording requirements locally", move |tx, control| {
            let progress = |message: &str, fraction| { let _ = tx.send(Message::Progress(message.into(), fraction)); };
            progress("Reading duration and audio streams; no flight data is being downloaded", 0.01);
            let result = (|| -> anyhow::Result<Prepared> {
                if let Ok(info) = audio_check::probe(&job, &control) {
                    let has_audio = info.has_audio;
                    let _ = tx.send(Message::RecordingInfo(identity.clone(), info));
                    anyhow::ensure!(has_audio, "No audio stream: location analysis needs audio. Video clues can still run.");
                }
                control.check()?;
                audio_check::inspect_job(&job, &control, &progress)
            })().map(Arc::new).map_err(|error| format!("{error:#}"));
            let _ = tx.send(Message::RecordingChecked(identity, result));
        });
    }

    pub(super) fn readiness_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new().fill(PANEL).corner_radius(10.).inner_margin(14.).show(ui, |ui| {
            ui.heading("Recording requirements");
            let current = self.readiness.current(&self.job);
            let report = self.readiness.prepared(&self.job);
            let report = report.as_ref().map(|p| p.report());
            if let Some(report) = report {
                ui.label(format!("Audio duration: {} · checked locally", duration(report.duration_s)));
            } else if current && let Some(info) = &self.readiness.info {
                if let Some(seconds) = info.duration_s { ui.label(format!("Duration estimate: {} (min:sec)", duration(seconds))); }
            } else if self.job.media.is_file() && !self.readiness.complete(&self.job) {
                ui.label("Checking this recording before any flight downloads…");
            } else if !self.job.media.is_file() { ui.label("Choose or download a recording to check it automatically."); }
            for (name, method, seconds, sounds) in [
                ("Doppler curves", AnalysisMethod::Doppler, audio_check::DOPPLER_MIN_SECONDS, audio_check::DOPPLER_MIN_SOUNDS),
                ("Passage timing", AnalysisMethod::PassageTiming, audio_check::TIMING_MIN_SECONDS, audio_check::TIMING_MIN_SOUNDS),
            ] {
                let selected = if self.job.analysis_method == method { " (selected)" } else { "" };
                let sound_kind = if method == AnalysisMethod::Doppler { "separate pitch curves" } else { "sound peaks" };
                ui.label(RichText::new(format!("{name}{selected}: at least {} and {sounds} {sound_kind}", minimum_duration(seconds))).strong());
                if let Some(report) = report {
                    let check = if method == AnalysisMethod::Doppler { &report.doppler } else { &report.passage_timing };
                    let meets = check.enough_observations_to_search;
                    ui.colored_label(if meets { ACCENT } else { egui::Color32::from_rgb(255,181,134) },
                        format!("{} · {} audio · {} / {sounds} sounds", if meets { "Meets duration and sound-count minimum" } else { "Below minimum" }, duration(report.duration_s), check.observation_count));
                } else if current && let Some(info) = &self.readiness.info {
                    ui.label(if !info.has_audio { "Not eligible: no audio stream".into() }
                        else if info.duration_s.is_some_and(|n| n > audio_check::MAX_SECONDS) { "Duration appears above the 2-hour maximum".into() }
                        else if info.duration_s.is_some_and(|n| n < seconds) { format!("Duration appears too short; minimum is {}", minimum_duration(seconds)) }
                        else { "Sound count is not checked yet".into() });
                }
            }
            ui.label(RichText::new("Maximum audio length: 2 hours. Passage timing also needs continuous audio and a search area. Sounds are unconfirmed; meeting the input minimum does not verify a location.").small().color(MUTED));
            if self.job.analysis_method == AnalysisMethod::PassageTiming && !self.use_bounds {
                ui.colored_label(egui::Color32::from_rgb(255,181,134), "Passage timing also needs a search area: open Search area & terrain to set it.");
            }
            if current && let Some(error) = &self.readiness.error { ui.colored_label(egui::Color32::from_rgb(255,181,134), error); }
            ui.separator();
            if self.job.offline {
                ui.colored_label(egui::Color32::from_rgb(255,181,134), "Offline mode: missing flight data WILL NOT download.");
                if ui.add_enabled(!self.busy(), egui::Button::new("Go online and download missing flight data")).help("Turn off Offline mode and enable automatic flight data. Reuse valid cached files and download missing data when an eligible location analysis runs. This does not start a search.").clicked() {
                    self.job.offline = false;
                    self.enable_automatic_flights();
                }
            } else if self.job.archive.is_some() {
                ui.label("Flight data: automatic. Analyze recording reuses valid cached files and downloads missing data when the location search needs it.");
                ui.label(RichText::new("A full day can be several GB. The recording check itself downloads no flight data.").small().color(MUTED));
            } else {
                ui.label("Flight data: using your selected local files. Missing flight data will not download.");
                if ui.add_enabled(!self.busy(), egui::Button::new("Enable automatic flight downloads")).help("Use the free historical flight provider and its cache. Missing data downloads during eligible location analysis; valid cached files are reused.").clicked() { self.enable_automatic_flights(); }
            }
            if self.readiness.complete(&self.job) && ui.add_enabled(!self.busy(), egui::Button::new("Recheck recording")).help("Repeat the local duration and sound-count check with the current audio settings. This does not download flight data or search for a location.").clicked() {
                self.readiness = State::default();
            }
        });
        ui.add_space(10.);
    }

    fn enable_automatic_flights(&mut self) {
        if self.job.archive.is_none() {
            self.job.archive = Some(soundgeo::archive::ArchiveRequest {
                date_from: self.date.clone(),
                date_to: None,
                cache: soundgeo::archive::default_cache(),
                provider: Default::default(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_recording_or_detector_settings_invalidates_eligibility() {
        let mut job = Job::default();
        let mut state = State::default();
        state.begin(key(&job));
        state.finish(Err("No audio stream".into()));
        assert!(state.complete(&job));
        job.min_snr_db += 1.;
        assert!(!state.complete(&job));
        job.min_snr_db -= 1.;
        job.media = "another.wav".into();
        assert!(!state.current(&job));
    }

    #[test]
    fn online_action_preserves_existing_provider_dates_and_cache() {
        let mut app = App::initial(None);
        assert!(!app.job.offline);
        assert!(app.job.archive.is_some());
        let request = app.job.archive.as_mut().unwrap();
        request.date_from = "2026-09-08".into();
        request.cache = "chosen-cache".into();
        request.provider = soundgeo::archive::ArchiveProvider::WebtrakChicago;
        app.enable_automatic_flights();
        let request = app.job.archive.unwrap();
        assert_eq!(request.date_from, "2026-09-08");
        assert_eq!(request.cache, std::path::PathBuf::from("chosen-cache"));
        assert_eq!(
            request.provider,
            soundgeo::archive::ArchiveProvider::WebtrakChicago
        );
    }

    #[test]
    fn automatic_short_recording_check_finishes_without_preparing_flights() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("short.wav");
        let mut wav = hound::WavWriter::create(
            &media,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..8000 {
            wav.write_sample(0i16).unwrap();
        }
        wav.finalize().unwrap();
        let mut app = App::initial(None);
        app.job.media = media;
        let cache = dir.path().join("flight-cache");
        app.job.archive.as_mut().unwrap().cache = cache.clone();
        app.readiness.observed = Some(key(&app.job));
        app.readiness.changed = Some(Instant::now() - Duration::from_secs(1));
        app.schedule_readiness(&egui::Context::default());
        assert!(app.busy());
        let deadline = Instant::now() + Duration::from_secs(15);
        while app.busy() && Instant::now() < deadline {
            app.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!app.busy());
        assert!(app.readiness.complete(&app.job));
        assert!(app.readiness.error.as_ref().unwrap().contains("20 seconds"));
        assert!(!cache.exists());
        app.run(&egui::Context::default());
        assert!(!app.busy());
        assert!(!cache.exists());
    }
}
