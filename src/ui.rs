use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use soundgeo::{Control, Job, Report, geo::Bounds};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

#[path = "speech_ui.rs"]
mod speech_ui;

// Keep action explanations available when a control is temporarily disabled.
trait ResponseHelp {
    fn help(self, text: &str) -> Self;
}
impl ResponseHelp for egui::Response {
    fn help(self, text: &str) -> Self {
        self.on_hover_text(text).on_disabled_hover_text(text)
    }
}

const ACCENT: Color32 = Color32::from_rgb(76, 224, 194);
const MUTED: Color32 = Color32::from_rgb(147, 164, 188);
const PANEL: Color32 = Color32::from_rgb(21, 31, 47);
#[derive(Default)]
enum MediaFeedback {
    #[default]
    None,
    Working(String),
    Info(String),
    Ready(Box<soundgeo::media_url::Download>),
    Error(String),
}

enum Message {
    SpeechPrepared(std::sync::Arc<soundgeo::speech::Prepared>),
    SpeechReport(Box<soundgeo::speech::Report>, bool),
    MediaListing(Box<soundgeo::media_url::Listing>),
    MediaDownloaded(Box<soundgeo::media_url::Download>),
    Progress(String, f32),
    Done(Box<Report>),
    Demo(Box<Job>),
    Downloaded(PathBuf),
    Catalog(PathBuf),
    Prepared(usize),
    VideoClues {
        report: Box<soundgeo::visual::Report>,
        inputs: [PathBuf; 2],
        terminal: bool,
    },
    VideoError(String, bool),
    VideoData(PathBuf),
    Error(String),
}
pub struct App {
    speech: speech_ui::State,
    job: Job,
    clock: String,
    use_bounds: bool,
    bounds: Bounds,
    report: Option<Report>,
    selected: usize,
    tab: usize,
    receiver: Option<Receiver<Message>>,
    control: Control,
    progress: f32,
    status: String,
    error: Option<String>,
    date: String,
    visual_options: soundgeo::visual::Options,
    visual_report: Option<soundgeo::visual::Report>,
    visual_country: String,
    visual_enabled: bool,
    visual_cache: PathBuf,
    visual_inputs: [PathBuf; 2],
    visual_error: Option<String>,
    media_options: soundgeo::media_url::Options,
    media_listing: Option<soundgeo::media_url::Listing>,
    media_item: usize,
    media_feedback: MediaFeedback,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, report: Option<Report>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(12, 20, 33);
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = Color32::from_rgb(9, 16, 28);
        style.visuals.selection.bg_fill = Color32::from_rgb(30, 88, 89);
        style.spacing.item_spacing = Vec2::new(10., 10.);
        style.spacing.button_padding = Vec2::new(12., 8.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.));
        cc.egui_ctx.set_style(style);
        Self::initial(report)
    }
    fn initial(report: Option<Report>) -> Self {
        Self {
            speech: speech_ui::State::default(),
            job: Job {
                ground_datum: soundgeo::geo::ElevationDatum::Egm96,
                archive: Some(soundgeo::archive::ArchiveRequest {
                    date_from: String::new(),
                    date_to: None,
                    cache: PathBuf::from("flight-cache"),
                    provider: Default::default(),
                }),
                ..Job::default()
            },
            clock: String::new(),
            use_bounds: false,
            bounds: Bounds {
                south: 34.,
                north: 36.,
                west: -111.,
                east: -109.,
            },
            report,
            selected: 0,
            tab: 0,
            receiver: None,
            control: Control::default(),
            progress: 0.,
            status: "Ready when you are".into(),
            error: None,
            date: String::new(),
            visual_options: soundgeo::visual::Options::default(),
            visual_report: None,
            visual_country: "ALL".into(),
            visual_enabled: false,
            visual_cache: soundgeo::visual_data::default_cache(),
            visual_inputs: Default::default(),
            visual_error: None,
            media_options: Default::default(),
            media_listing: None,
            media_item: 1,
            media_feedback: MediaFeedback::None,
        }
    }
    fn busy(&self) -> bool {
        self.receiver.is_some()
    }
    pub fn with_speech(mut self, report: Option<soundgeo::speech::Report>) -> Self {
        self.speech.report = report;
        self
    }
    fn url_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.collapsing("Or paste a video / page URL", |ui| {
            if ui.add(egui::TextEdit::singleline(&mut self.media_options.url).hint_text("https://…").desired_width(f32::INFINITY)).changed() { self.media_listing = None; self.media_feedback = MediaFeedback::None; }
            let previous = (self.media_options.engine, self.media_options.impersonate);
            egui::ComboBox::from_id_salt("media_engine").selected_text(format!("{:?}", self.media_options.engine)).show_ui(ui, |ui| {
                use soundgeo::media_url::Engine;
                ui.selectable_value(&mut self.media_options.engine, Engine::Auto, "Automatic: yt-dlp, then gallery-dl").help("Try yt-dlp first, then gallery-dl if discovery fails or finds no playable media.");
                ui.selectable_value(&mut self.media_options.engine, Engine::YtDlp, "yt-dlp").help("Use yt-dlp to discover and download clips from the supplied URL.");
                ui.selectable_value(&mut self.media_options.engine, Engine::GalleryDl, "gallery-dl").help("Use gallery-dl to discover and download clips from the supplied URL.");
            }).response.help("Choose which downloader finds clips at the supplied URL. Automatic tries yt-dlp, then gallery-dl.");
            ui.checkbox(&mut self.media_options.impersonate, "Browser impersonation (Chrome)").help("Ask yt-dlp to use Chrome-compatible request behavior for sites that need browser-like requests. Automatic download mode can also retry with this enabled.");
            if previous != (self.media_options.engine, self.media_options.impersonate) { self.media_listing = None; self.media_feedback = MediaFeedback::None; }
            if ui.add_enabled(!self.media_options.url.trim().is_empty(), egui::Button::new("Find media")).help("Enter a public video or page URL, then list its downloadable clips. Choose a clip before downloading it. New listings require a connection; offline mode reuses a saved listing.").clicked() {
                let options = self.media_options.clone(); self.media_listing = None;
                self.status = "Finding downloadable clips".into();
                self.media_feedback = MediaFeedback::Working(self.status.clone());
                self.worker(ctx, move |tx, control| {
                    let progress = |m: &str, p| { let _ = tx.send(Message::Progress(m.into(), p)); };
                    let result = soundgeo::media_url::discover(&options, &control, &progress);
                    let _ = tx.send(match result { Ok(value) => Message::MediaListing(Box::new(value)), Err(error) => Message::Error(format!("{error:#}")) });
                });
            }
            if let Some(listing) = &self.media_listing {
                ui.label("Select the recording to download:");
                for candidate in &listing.candidates {
                    ui.radio_value(&mut self.media_item, candidate.item, format!("{}: {}", candidate.item, candidate.title)).help("Select this clip from the discovered page results, then choose Download and use selected clip.");
                }
                ui.label(RichText::new("Up to 20 clips. Downloaded page metadata is kept as provenance; it does not set the search location or clock.").small().color(MUTED));
                if ui.button("Download and use selected clip").help("Download the selected clip into the media cache and select it as your recording. Then choose Analyze recording to run the analysis.").clicked() {
                    let listing = listing.clone(); let options = self.media_options.clone(); let item = self.media_item;
                    self.status = "Downloading selected clip".into();
                    self.media_feedback = MediaFeedback::Working(self.status.clone());
                    self.worker(ctx, move |tx, control| {
                        let progress = |m: &str, p| { let _ = tx.send(Message::Progress(m.into(), p)); };
                        let result = soundgeo::media_url::download(&options, &listing, item, &control, &progress);
                        let _ = tx.send(match result { Ok(value) => Message::MediaDownloaded(Box::new(value)), Err(error) => Message::Error(format!("{error:#}")) });
                    });
                }
            }
            ui.label(RichText::new(format!("Media cache: {}", self.media_options.cache.display())).small().color(MUTED));
            if ui.small_button("Choose media cache folder…").help("Choose where downloaded recordings and their source metadata are stored. Existing cached files stay in their current folder.").clicked() && let Some(path) = rfd::FileDialog::new().pick_folder() { self.media_options.cache = path; self.media_listing = None; self.media_feedback = MediaFeedback::None; }
        }).header_response.help("Expand to find and download a recording from a public video or page URL.");
    }
    fn media_feedback(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match &self.media_feedback {
            MediaFeedback::None => {}
            MediaFeedback::Working(message) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(message);
                });
                if ui.button("Cancel media task").help("Stop the current clip discovery or download. Partial downloads remain in the cache so a later attempt can resume.").clicked() {
                    self.control.stop();
                    self.media_feedback = MediaFeedback::Working("Cancelling media task…".into());
                }
            }
            MediaFeedback::Info(message) => {
                ui.label(message);
            }
            MediaFeedback::Error(message) => {
                ui.colored_label(
                    Color32::from_rgb(255, 181, 134),
                    "Recording download needs attention",
                );
                ui.collapsing("Media error details", |ui| { ui.label(message); })
                    .header_response.help("Show the download error and diagnostic log location. Correct the issue, then retry Find media or Download and use selected clip.");
            }
            MediaFeedback::Ready(download) if download.path == self.job.media => {
                ui.colored_label(ACCENT, "Download complete — recording selected");
                if let Some(candidate) = download
                    .source
                    .candidates
                    .iter()
                    .find(|c| c.item == download.selected_item)
                {
                    ui.label(&candidate.title);
                }
                ui.label(format!(
                    "{:.1} MB · {:.0} seconds · {}",
                    download.size as f64 / 1_000_000.,
                    download.duration_s,
                    if download.has_audio {
                        "audio available"
                    } else {
                        "no audio stream"
                    }
                ));
                if download.has_audio {
                    ui.label("Set the flight-data options, then start analysis.");
                    if ui.add_enabled(!self.busy(), egui::Button::new("Analyze downloaded recording"))
                        .help("Start the same analysis as Analyze recording using this selected download and your current flight-data, video-clue and speech settings. Wait for any current task to finish first.").clicked() {
                        self.run(ctx);
                    }
                } else {
                    ui.label("Use Read video clues only to inspect visible text. Location analysis needs audio.");
                }
            }
            MediaFeedback::Ready(_) => {}
        }
    }
    fn worker(
        &mut self,
        ctx: &egui::Context,
        task: impl FnOnce(mpsc::Sender<Message>, Control) + Send + 'static,
    ) {
        self.error = None;
        self.progress = 0.;
        self.control = Control::default().with_offline(self.job.offline);
        let control = self.control.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.receiver = Some(rx);
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                task(tx.clone(), control)
            }));
            if result.is_err() {
                let _ = tx.send(Message::Error(
                    "The worker stopped unexpectedly. Your original files were not changed.".into(),
                ));
            }
            ctx.request_repaint();
        });
    }
    fn run(&mut self, ctx: &egui::Context) {
        let mut job = self.job.clone();
        let clock_error = if self.clock.trim().is_empty() {
            job.start_utc = None;
            None
        } else {
            match soundgeo::data::parse_time(self.clock.trim()) {
                Ok(t) => {
                    job.start_utc = Some(t);
                    None
                }
                Err(e) => Some(e.to_string()),
            }
        };
        job.bounds = self.use_bounds.then_some(self.bounds);
        let geo_error = clock_error.or_else(|| job.validate().err().map(|e| e.to_string()));
        if !self.speech.options.enabled && !self.visual_enabled && geo_error.is_some() {
            self.error = geo_error;
            return;
        }
        let options = self.speech.options.clone();
        let prepared = self.speech.current_prepared(&job);
        let video = self.visual_enabled.then(|| self.video_options());
        let country = self.visual_country.clone();
        let cache = self.visual_cache.clone();
        self.visual_report = None;
        self.visual_error = None;
        self.speech.mark_requested(&job);
        self.report = None;
        self.selected = 0;
        self.status = "Starting analysis".into();
        self.worker(ctx, move |tx, control| {
            let progress = |m: &str, p: f32| {
                let _ = tx.send(Message::Progress(m.into(), p));
            };
            if let Some(options) = video {
                read_video_clues(&tx, &options, &country, &cache, &control, false);
            }
            if let Err(error) = control.check() {
                let _ = tx.send(Message::Error(error.to_string()));
                return;
            }
            if options.enabled {
                let speech = speech_ui::analyze(&job, &options, prepared, &control, &progress);
                match speech {
                    Ok(report) => {
                        let _ = tx.send(Message::SpeechReport(Box::new(report), false));
                    }
                    Err(error) => {
                        let _ = tx.send(Message::Error(format!("Speech analysis: {error:#}")));
                        return;
                    }
                }
            }
            if let Err(error) = control.check() {
                let _ = tx.send(Message::Error(error.to_string()));
                return;
            }
            if let Some(error) = geo_error {
                let _ = tx.send(Message::Error(format!(
                    "Any completed optional results were retained. Geolocation needs attention: {error}"
                )));
                return;
            }
            let result = soundgeo::analyze(&job, &control, &progress);
            let _ = tx.send(match result {
                Ok(r) => Message::Done(Box::new(r)),
                Err(e) => Message::Error(format!("{e:#}")),
            });
        });
    }
    fn load_demo(&mut self, ctx: &egui::Context) {
        let dir = std::env::temp_dir().join(format!("soundgeo-demo-{}", std::process::id()));
        self.status = "Creating synthetic demo".into();
        self.worker(ctx, move |tx, _| {
            let result = soundgeo::demo::create(&dir);
            let _ = tx.send(match result {
                Ok(job) => Message::Demo(Box::new(job)),
                Err(e) => Message::Error(e.to_string()),
            });
        });
    }
    fn poll(&mut self) {
        let messages: Vec<_> = self
            .receiver
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        for message in messages {
            match message {
                Message::SpeechPrepared(prepared) => {
                    self.status = if prepared.ready() {
                        "Speech models ready".into()
                    } else {
                        prepared.report.warnings.join(" ")
                    };
                    self.speech.report = Some(prepared.report.clone());
                    self.speech.prepared = Some(prepared);
                    self.speech.clear_preview();
                    self.progress = 1.;
                    self.receiver = None;
                }
                Message::SpeechReport(report, terminal) => {
                    self.speech.report = Some(*report);
                    self.speech.clear_preview();
                    if terminal {
                        self.status =
                            "Speech analysis finished; review its status and original text".into();
                        self.progress = 1.;
                        self.receiver = None;
                    }
                }
                Message::MediaListing(listing) => {
                    self.media_feedback = MediaFeedback::Info(format!(
                        "Found {} clip(s). Select one above, then download it.",
                        listing.candidates.len()
                    ));
                    self.media_item = listing.candidates.first().map(|c| c.item).unwrap_or(1);
                    self.media_listing = Some(*listing);
                    self.status = "Choose a clip, then download it".into();
                    self.progress = 1.;
                    self.receiver = None;
                }
                Message::MediaDownloaded(download) => {
                    self.job.media = download.path.clone();
                    self.visual_report = None;
                    self.visual_error = None;
                    self.speech.invalidate();
                    self.status = if download.has_audio {
                        "Recording downloaded and ready for analysis"
                    } else {
                        "Video downloaded; no audio stream, so only video clues are available"
                    }
                    .into();
                    self.media_feedback = MediaFeedback::Ready(download);
                    self.progress = 1.;
                    self.receiver = None;
                }
                Message::VideoClues {
                    report,
                    inputs,
                    terminal,
                } => {
                    self.visual_report = Some(*report);
                    self.visual_inputs = inputs;
                    self.visual_error = None;
                    if terminal {
                        self.status = "Video clues ready".into();
                        self.progress = 1.;
                        self.receiver = None;
                    }
                }
                Message::VideoError(error, terminal) => {
                    self.visual_error = Some(error);
                    if terminal {
                        self.status = if self.control.check().is_err() {
                            "Cancelled"
                        } else {
                            "Video clues need attention"
                        }
                        .into();
                        self.receiver = None;
                    }
                }
                Message::VideoData(path) => {
                    self.visual_options.gazetteer = path;
                    self.status = "Place data ready for offline use".into();
                    self.progress = 1.;
                    self.receiver = None;
                }
                Message::Progress(m, p) => {
                    if let MediaFeedback::Working(message) = &mut self.media_feedback {
                        message.clone_from(&m);
                    }
                    self.status = m;
                    self.progress = p;
                }
                Message::Done(report) => {
                    self.report = Some(*report);
                    self.status = if self.visual_error.is_some() {
                        "Location analysis complete; video clues need attention"
                    } else {
                        "Analysis complete"
                    }
                    .into();
                    self.progress = 1.;
                    self.receiver = None;
                }
                Message::Demo(job) => {
                    self.bounds = job.bounds.unwrap();
                    self.use_bounds = true;
                    self.job = *job;
                    self.clock.clear();
                    self.status = "Synthetic demo loaded — click Analyze recording".into();
                    self.receiver = None;
                }
                Message::Prepared(days) => {
                    self.status = format!("{days} flight archives ready for offline analysis");
                    self.progress = 1.;
                    self.receiver = None;
                }
                Message::Downloaded(path) => {
                    let catalog = path.parent().map(|p| p.join("aircraft-types.json"));
                    if catalog.as_ref().is_some_and(|p| p.is_file()) {
                        self.job.type_catalog = catalog;
                    }
                    self.job.tracks = path;
                    self.job.archive = None;
                    self.status = "Flight archive ready".into();
                    self.receiver = None;
                }
                Message::Error(error) => {
                    if matches!(self.media_feedback, MediaFeedback::Working(_)) {
                        self.media_feedback = if error == "Cancelled" {
                            MediaFeedback::Info(
                                "Media task cancelled. You can retry from the URL controls.".into(),
                            )
                        } else {
                            MediaFeedback::Error(error.clone())
                        };
                    }
                    self.status = if error == "Cancelled" {
                        "Cancelled"
                    } else {
                        "Needs attention"
                    }
                    .into();
                    self.error = Some(error);
                    self.receiver = None;
                }
                Message::Catalog(path) => {
                    self.job.type_catalog = Some(path);
                    self.status = "Aircraft type catalog ready".into();
                    self.receiver = None;
                }
            }
        }
    }
    fn input_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.);
        ui.label(RichText::new("YOUR RECORDING").small().color(ACCENT));
        ui.add_space(4.);
        ui.heading("Start with the sound.");
        ui.label(
            RichText::new(
                "Find candidate locations from aircraft flyovers in a video or audio file.",
            )
            .color(MUTED),
        );
        ui.add_space(12.);
        ui.add_enabled_ui(!self.busy(), |ui| {
            ui.label(RichText::new("1  Video or audio").strong());
            file_field(
                ui,
                &mut self.job.media,
                "Choose recording…",
                &[
                    "mp4", "mov", "mkv", "webm", "avi", "wav", "mp3", "flac", "m4a",
                ],
            );
            self.url_controls(ui, ctx);
        });
        // Keep download feedback and cancellation visible beside the URL controls,
        // outside the disabled input group while a worker is running.
        self.media_feedback(ui, ctx);
        ui.add_enabled_ui(!self.busy(),|ui|{
            self.video_controls(ui, ctx);
            ui.label(RichText::new("You can also drop a file into this window.").small().color(MUTED));
            ui.add_space(6.);ui.label(RichText::new("2  Historical flight data").strong());
            ui.checkbox(&mut self.job.offline, "Offline mode: use cached data only").help("Use local recordings, prepared flight data and cached models. Missing downloads require returning online and turning this option off.");
            let mut automatic = self.job.archive.is_some();
            if ui.checkbox(&mut automatic,"Download missing flight data automatically").help("Find and prepare missing historical flight data during analysis. Turn this off to choose your own flight-data file or archive folder.").changed() {
                self.job.archive = automatic.then(|| soundgeo::archive::ArchiveRequest {date_from:self.date.clone(),date_to:None,cache:PathBuf::from("flight-cache"),provider:Default::default()});
            }
            if let Some(request) = &mut self.job.archive {
                use soundgeo::archive::ArchiveProvider;
                egui::ComboBox::from_id_salt("flight_provider").selected_text(match request.provider {
                    ArchiveProvider::AdsbLol => "Worldwide: ADSB.lol",
                    ArchiveProvider::AdsbExchangeSample => "Free evaluation sample: first of each month",
                    ArchiveProvider::AdsbExchangeSampleHires => "Free evaluation sample: high resolution",
                    ArchiveProvider::WebtrakChicago => "Chicago: public airport replay",
                    ArchiveProvider::WebtrakLax => "LAX: public airport replay",
                }).show_ui(ui,|ui|{
                    ui.selectable_value(&mut request.provider,ArchiveProvider::AdsbLol,"Worldwide: ADSB.lol").help("Search worldwide historical ADSB.lol flight data for the selected dates. Coverage depends on received aircraft positions.");
                    ui.selectable_value(&mut request.provider,ArchiveProvider::AdsbExchangeSample,"Free evaluation sample: first of each month").help("Use the ADS-B Exchange monthly evaluation sample. Select the first day of a month and a search area; discovery uses five-minute snapshots.");
                    ui.selectable_value(&mut request.provider,ArchiveProvider::AdsbExchangeSampleHires,"Free evaluation sample: high resolution").help("Use high-resolution tracks from the monthly evaluation sample. Select the first day of a month and a search area; discovery can still miss flights between snapshots.");
                    ui.selectable_value(&mut request.provider,ArchiveProvider::WebtrakChicago,"Chicago: public airport replay").help("Use public airport-replay tracks around Chicago. Choose dates and a search area covered by that source.");
                    ui.selectable_value(&mut request.provider,ArchiveProvider::WebtrakLax,"LAX: public airport replay").help("Use public airport-replay tracks around Los Angeles International Airport. Choose dates and a search area covered by that source.");
                }).response.help("Choose the historical flight-data source. Available dates, geographic coverage and download size differ by source.");
                ui.label(RichText::new("Optional date range. Blank dates find the provider's available days online. Offline mode searches prepared dates.").small().color(MUTED));
                if request.provider.sample_resolution().is_some() {
                    ui.label(RichText::new("Use the first day of a month and set a search area below. This free sample is for testing and evaluation. It can miss flights between five-minute discovery snapshots.").small().color(MUTED));
                    ui.hyperlink_to("Free sample coverage and terms", "https://www.adsbexchange.com/data-products/sample-data/").help("Open the data provider website to review evaluation-sample coverage and usage terms.");
                }
                ui.add(egui::TextEdit::singleline(&mut request.date_from).hint_text("First date, or blank to find dates").desired_width(f32::INFINITY));
                let mut several = request.date_to.is_some();
                if ui.checkbox(&mut several,"Search several possible dates").help("Search an inclusive range of possible recording dates. Enter the first and last UTC dates; a wider range increases download and search work.").changed(){request.date_to=several.then(String::new);}
                if let Some(to)=&mut request.date_to{ui.add(egui::TextEdit::singleline(to).hint_text("Last date, inclusive (YYYY-MM-DD)").desired_width(f32::INFINITY));}
                let mut cache=request.cache.display().to_string();
                if ui.text_edit_singleline(&mut cache).changed(){request.cache=PathBuf::from(cache);}
                if ui.small_button("Choose cache folder…").help("Choose where historical flight downloads and prepared data are stored. Reuse this folder for offline analysis.").clicked() && let Some(path)=rfd::FileDialog::new().pick_folder(){request.cache=path;}
            } else {
            file_field(ui,&mut self.job.tracks,"Choose flight data…",&["csv","json","gz","aa","tar"]);
            if ui.small_button("Or choose an archive folder").help("Select a folder containing local flight archives or prepared flight data to search with automatic downloads turned off.").clicked()&& let Some(p)=rfd::FileDialog::new().pick_folder(){self.job.tracks=p;}
            }
            let prepare_data = self.job.archive.is_some() && ui.add_enabled(!self.job.offline, egui::Button::new("Prepare data for offline use")).help("Download and prepare flight data for the selected provider, dates and search area before going offline. Turn off Offline mode to enable this action.").clicked();
            let refresh_gaps = self.job.archive.as_ref().is_some_and(|request| request.provider.sample_resolution().is_some())
                && ui.add_enabled(!self.job.offline, egui::Button::new("Retry unavailable source files")).help("Retry flight-source files previously recorded as unavailable, then update the prepared data. Requires a connection and Offline mode turned off.").clicked();
            if prepare_data || refresh_gaps {
                let mut job = self.job.clone();
                job.bounds = self.use_bounds.then_some(self.bounds);
                self.status = if refresh_gaps { "Retrying unavailable source files" } else { "Preparing offline data" }.into();
                self.worker(ctx, move |tx, control| {
                    let control = if refresh_gaps { control.with_source_gap_refresh() } else { control };
                    let result = soundgeo::prepare_data(&job, &control, &|m,p| { let _ = tx.send(Message::Progress(m.into(),p)); });
                    let _ = tx.send(match result { Ok(data) => Message::Prepared(data.flight_sources.len()), Err(e) => Message::Error(format!("{e:#}")) });
                });
            }
            ui.label(RichText::new("Searches all dates in the supplied data. The recording stays on your computer.").small().color(MUTED));
            egui::CollapsingHeader::new("Download a public flight archive").show(ui,|ui|{
                ui.label("Choose a possible recording date. Each day is several GB; no API key is needed.");
                ui.add(egui::TextEdit::singleline(&mut self.date).hint_text("YYYY-MM-DD").desired_width(f32::INFINITY));
                if ui.button("Download day…").help("Enter a UTC date as YYYY-MM-DD, then choose a folder for that full day of ADSB.lol flight data. Each day can be several GB. Requires a connection.").clicked(){
                    if chrono::NaiveDate::parse_from_str(&self.date,"%Y-%m-%d").is_err(){self.error=Some("Enter an archive date in YYYY-MM-DD format.".into());}
                    else if let Some(dir)=rfd::FileDialog::new().set_title("Choose flight archive cache").pick_folder(){
                        let date=self.date.clone();self.status="Downloading flight archive".into();
                        self.worker(ctx,move|tx,control|{let result=soundgeo::data::download_day(&date,&dir,&control,&|m,p|{let _=tx.send(Message::Progress(m.into(),p));});let _=tx.send(match result{Ok(p)=>Message::Downloaded(p),Err(e)=>Message::Error(format!("{e:#}"))});});
                    }
                }
                ui.hyperlink_to("Archive coverage and data licence","https://www.adsb.lol/docs/open-data/historical").help("Open ADSB.lol documentation for historical-data coverage and licensing.");
            }).header_response.help("Expand to download a complete day of historical flight data into a folder you choose.");
            ui.add_space(6.);ui.label(RichText::new("3  Recording time").strong());
            ui.add(egui::TextEdit::singleline(&mut self.clock).hint_text("Unknown — infer from flyovers").desired_width(f32::INFINITY));
            ui.label(RichText::new("Optional: 2026-09-08T10:00:00Z. With no time, two separate flyovers are much more useful.").small().color(MUTED));
            if !self.clock.trim().is_empty(){ui.horizontal(|ui|{ui.label("Allow ± seconds");ui.add(egui::DragValue::new(&mut self.job.time_tolerance_s).range(0. ..=86400.));});}
            ui.add_space(6.);
            egui::CollapsingHeader::new("Search area & terrain").show(ui,|ui|{
                self.aircraft_controls(ui,ctx);
                ui.checkbox(&mut self.use_bounds,"Limit the search area").help("Restrict candidate locations to the south, north, west and east bounds entered below. Use independently supported bounds so the true location is not excluded.");
                if self.use_bounds {egui::Grid::new("bounds").num_columns(2).show(ui,|ui|{
                    for (label,value) in [("South",&mut self.bounds.south),("North",&mut self.bounds.north),("West",&mut self.bounds.west),("East",&mut self.bounds.east)]{ui.label(label);ui.add(egui::DragValue::new(value).speed(0.001).max_decimals(5));ui.end_row();}
                });}else{ui.label("Uses the flight-data extent. Narrow a large archive for a finer search.");}
                ui.horizontal(|ui|{ui.label("Fallback elevation (m)");ui.add(egui::DragValue::new(&mut self.job.ground_m).range(-500. ..=9000.));});
                ui.label("Fallback height datum");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.job.ground_datum, soundgeo::geo::ElevationDatum::Egm96, "Sea level (EGM96)").help("Interpret the fallback elevation as height above mean sea level using the EGM96 datum.");
                    ui.selectable_value(&mut self.job.ground_datum, soundgeo::geo::ElevationDatum::Wgs84Ellipsoid, "WGS84 ellipsoid").help("Interpret the fallback elevation as height above the WGS84 reference ellipsoid, as used by ellipsoidal GPS heights.");
                });
                ui.checkbox(&mut self.job.smooth_tracks, "Reduce flight-position noise")
                    .help("Centered 10-second local linear fit. Keeps track endpoints and timestamps; can blur rapid maneuvers.");
                if ui.checkbox(&mut self.job.download_terrain,"Download missing terrain from free sources").help("Fetch missing elevation tiles for the chosen search area and cache them locally. Requires a connection; heights describe the ground, not microphone height.").changed() && self.job.download_terrain && self.job.terrain.is_none() {
                    self.job.terrain=Some(PathBuf::from("terrain-cache"));
                }
                if self.job.download_terrain {
                    ui.label("Set a search area. Terrain gives ground height, not microphone height above ground.");
                    ui.hyperlink_to("Mapzen Terrain Tiles and provider credits", "https://github.com/tilezen/joerd/blob/master/docs/attribution.md").help("Open the terrain-data provider credits and attribution requirements.");
                }
                if ui.button("Choose terrain folder…").help("Select a folder containing elevation tiles, or a cache for terrain downloads. Terrain supplies ground height for the sound-travel calculation.").clicked() && let Some(path)=rfd::FileDialog::new().pick_folder(){self.job.terrain=Some(path);}
                if let Some(p)=&self.job.terrain{ui.label(RichText::new(p.display().to_string()).small());if ui.small_button("Clear terrain").help("Stop using the selected terrain folder and turn off terrain downloads. Use the fallback elevation instead. Cached files are kept.").clicked(){self.job.terrain=None;self.job.download_terrain=false;}}
            }).header_response.help("Expand to set geographic bounds, aircraft filters, quiet-gap handling and elevation data.");
            ui.label("Sound matching method");
            egui::ComboBox::from_id_salt("analysis-method")
                .selected_text(match self.job.analysis_method {soundgeo::AnalysisMethod::Doppler=>"Doppler curves",soundgeo::AnalysisMethod::PassageTiming=>"Flight passage timing"})
                .show_ui(ui,|ui| {
                    ui.selectable_value(&mut self.job.analysis_method,soundgeo::AnalysisMethod::Doppler,"Doppler curves").help("Match descending aircraft-tone pitch curves against flight tracks, fitting sound travel time and a shared recording clock.");
                    ui.selectable_value(&mut self.job.analysis_method,soundgeo::AnalysisMethod::PassageTiming,"Flight passage timing").help("Compare sound-peak times with predicted flight passages. Set a search area and use at least eight minutes of continuous audio; shuffled controls screen for a timing signal.");
                }).response.help("Choose pitch-curve matching or flight-passage timing. Each method has different recording and search-area requirements.");
            if self.job.analysis_method == soundgeo::AnalysisMethod::PassageTiming {
                ui.label("Set a search area. Use at least eight minutes of continuous audio. The app checks 19 shuffled-sound controls before it reports a timing signal.");
                ui.label("Timing results are possible locations. Their accuracy is not guaranteed.");
            }
            egui::CollapsingHeader::new("Advanced audio & search").show(ui,|ui|{
                if self.job.analysis_method == soundgeo::AnalysisMethod::Doppler {
                ui.checkbox(&mut self.job.fast_flyovers,"Also detect short flyovers (500–3900 Hz)").help("Add a detector for short, higher-frequency flyovers to the regular Doppler scan. Lower contrast settings can introduce more false detections.");
                if self.job.fast_flyovers {ui.horizontal(|ui|{ui.label("Short-pass contrast (dB)");ui.add(egui::DragValue::new(&mut self.job.fast_min_snr_db).range(6. ..=60.));});}
                use soundgeo::search::SearchMode;
                egui::ComboBox::from_id_salt("search-mode").selected_text(match self.job.search_mode{SearchMode::Auto=>"Automatic search method",SearchMode::RegionalGrid=>"Regional grid",SearchMode::FlightCorridors=>"Indexed flight corridors"}).show_ui(ui,|ui|{
                    ui.selectable_value(&mut self.job.search_mode,SearchMode::Auto,"Automatic search method").help("Let SoundGeo choose the location-search method for the available flight data and search area.");ui.selectable_value(&mut self.job.search_mode,SearchMode::FlightCorridors,"Indexed flight corridors").help("Generate location hypotheses near indexed flight corridors, then evaluate how well their predicted sounds match the recording.");ui.selectable_value(&mut self.job.search_mode,SearchMode::RegionalGrid,"Regional grid").help("Evaluate locations on a grid inside the search area. Large areas or finer spacing can require more search work.");
                }).response.help("Choose how location hypotheses are generated, or leave the choice automatic.");
                egui::Grid::new("advanced").num_columns(2).show(ui,|ui|{
                    for (label,value) in [("Low frequency (Hz)",&mut self.job.min_hz),("High frequency (Hz)",&mut self.job.max_hz),("Min contrast (dB)",&mut self.job.min_snr_db),("Temperature (°C)",&mut self.job.temperature_c),("Grid target (m)",&mut self.job.grid_m)]{ui.label(label);ui.add(egui::DragValue::new(value).speed(1.));ui.end_row();}
                    ui.label("Media clock drift limit (ppm)");ui.add(egui::DragValue::new(&mut self.job.max_clock_drift_ppm).range(0. ..=5000.));ui.end_row();
                    ui.label("Grid point budget");ui.add(egui::DragValue::new(&mut self.job.max_grid_points).range(100..=50000));ui.end_row();
                    ui.label("Hypotheses per batch");ui.add(egui::DragValue::new(&mut self.job.max_hypotheses).range(100..=100_000_000));ui.end_row();
                    ui.label("Flight-corridor workers");ui.horizontal(|ui|{
                        let mut automatic=self.job.search_threads.is_none();
                        if ui.checkbox(&mut automatic,"Automatic").help("Let SoundGeo use up to four flight-corridor workers. Clear this option to choose the worker limit yourself; more workers can use more CPU.").changed(){self.job.search_threads=(!automatic).then_some(4);}
                        if let Some(workers)=&mut self.job.search_threads{ui.add(egui::DragValue::new(workers).range(1..=std::thread::available_parallelism().map_or(1,|n|n.get())));}
                    }).response.help("Automatic uses up to 4 workers. A higher limit can use more CPU capacity. Searches with terrain tiles use one worker.");ui.end_row();
                });
                } else {
                    egui::Grid::new("timing-options").num_columns(2).show(ui,|ui| {
                        ui.label("Grid spacing (m)");ui.add(egui::DragValue::new(&mut self.job.timing_options.grid_m).range(50. ..=1000.));ui.end_row();
                        ui.label("Grid point limit");ui.add(egui::DragValue::new(&mut self.job.timing_options.maximum_grid_points).range(100..=50000));ui.end_row();
                        ui.label("Maximum flight distance (m)");ui.add(egui::DragValue::new(&mut self.job.timing_options.maximum_passage_distance_m).range(100. ..=5000.));ui.end_row();
                        ui.label("Temperature (°C)");ui.add(egui::DragValue::new(&mut self.job.temperature_c).range(-50. ..=60.));ui.end_row();
                    });
                }
                if ui.button("Choose FFmpeg executable…").help("Select an FFmpeg executable to decode recordings. The Windows download includes FFmpeg; an override is normally unnecessary.").clicked(){self.job.ffmpeg=rfd::FileDialog::new().pick_file();}
            }).header_response.help("Expand to adjust sound detection, location-search settings, CPU use and the media decoder.");
            ui.add_space(10.);
            self.speech_controls(ui, ctx);
            if ui.add_sized([ui.available_width(),42.],egui::Button::new(RichText::new("Analyze recording").strong().color(Color32::from_rgb(9,26,30))).fill(ACCENT)).help("Analyze the selected recording against the flight data. Checked video-clue and speech options run as stages of the same analysis, with progress shown below.").clicked(){self.run(ctx);}
            if ui.add_sized([ui.available_width(),34.],egui::Button::new("Try a synthetic demo")).help("Create and select a small generated recording with matching flight tracks. Then click Analyze recording to try a known synthetic example.").clicked(){self.load_demo(ctx);}
        });
        if self.busy() {
            ui.add_space(12.);
            ui.add(egui::ProgressBar::new(self.progress).animate(true));
            ui.label(&self.status);
            if ui.button("Cancel").help("Request that the current task stop and skip remaining analysis stages. Cancellation may take a moment; the original recording is kept.").clicked() {
                self.control.stop();
                self.status = "Cancelling…".into();
            }
        }
        if let Some(error) = &self.error {
            ui.add_space(10.);
            ui.colored_label(Color32::from_rgb(255, 181, 134), error);
        }
    }
    fn aircraft_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use soundgeo::{aircraft::AircraftFilter, search::SilenceMode};
        ui.label("Aircraft types");
        egui::ComboBox::from_id_salt("aircraft-filter")
            .selected_text(match self.job.aircraft_filter {
                AircraftFilter::All => "All aircraft",
                AircraftFilter::PropellerOrUnknown => "Propellers + unknown types",
                AircraftFilter::PropellerOnly => "Known propellers only",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.job.aircraft_filter,
                    AircraftFilter::PropellerOrUnknown,
                    "Propellers + unknown types",
                ).help("Keep known propeller aircraft and aircraft whose type is unknown. A type catalog improves classification.");
                ui.selectable_value(
                    &mut self.job.aircraft_filter,
                    AircraftFilter::PropellerOnly,
                    "Known propellers only",
                ).help("Keep only aircraft classified as propeller-driven by the type catalog. Missing or incorrect type data can exclude a relevant flight.");
                ui.selectable_value(
                    &mut self.job.aircraft_filter,
                    AircraftFilter::All,
                    "All aircraft",
                ).help("Keep aircraft of every known and unknown type in the flight search.");
            }).response.help("Choose which aircraft types to include. Known-type filters use the aircraft-type catalog.");
        if ui.button("Choose aircraft type catalog…").help("Select an existing aircraft-type JSON catalog to distinguish propeller aircraft from other or unknown types.").clicked() {
            self.job.type_catalog = rfd::FileDialog::new()
                .add_filter("Type catalog", &["json"])
                .pick_file();
        }
        if ui.button("Download type catalog…").help("Download an aircraft-type catalog, save it to the file you choose, and use it for aircraft filtering. Requires a connection.").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("aircraft-types.json")
                .save_file()
        {
            self.worker(ctx, move |tx, control| {
                let result = control
                    .require_network(&path.display().to_string())
                    .and_then(|_| soundgeo::aircraft::download(&path));
                let _ = tx.send(match result {
                    Ok(()) => Message::Catalog(path),
                    Err(e) => Message::Error(e.to_string()),
                });
            });
        }
        if let Some(path) = &self.job.type_catalog {
            ui.label(RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).small());
        }
        ui.label("Quiet intervals between flyovers");
        egui::ComboBox::from_id_salt("silence")
            .selected_text(match self.job.silence_mode {
                SilenceMode::Off => "Ignore quiet gaps",
                SilenceMode::SupportingEvidence => "Use as supporting evidence",
                SilenceMode::Strict => "Reject conflicting flyovers",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.job.silence_mode,
                    SilenceMode::SupportingEvidence,
                    "Use as supporting evidence",
                ).help("Record conflicts between predicted flyovers and quiet audio intervals as supporting evidence for review.");
                ui.selectable_value(
                    &mut self.job.silence_mode,
                    SilenceMode::Strict,
                    "Reject conflicting flyovers",
                ).help("Reject candidates with conflicting predicted flyovers during quiet intervals. This assumes closer known propellers would be audible and can reject a true location.");
                ui.selectable_value(
                    &mut self.job.silence_mode,
                    SilenceMode::Off,
                    "Ignore quiet gaps",
                ).help("Do not use quiet intervals between detected flyovers to evaluate candidate locations.");
            }).response.help("Choose how quiet gaps between flyovers affect candidate evaluation. Strict rejection makes stronger audibility assumptions.");
        ui.label(RichText::new("Strict mode assumes closer known propellers would be audible. It can reject the true location.").small().color(MUTED));
        ui.separator();
    }
    fn video_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("Video clues (optional)").show(ui, |ui| {
            ui.checkbox(&mut self.visual_enabled, "Read video clues during analysis")
                .help("When checked, Analyze recording also reads visible place names and clocks. Missing GeoNames place data downloads automatically; OCR runs locally. The findings appear alongside the location results for you to review.");
            ui.label("Reads visible names and clocks locally. Place data is downloaded once and reused.");
            ui.horizontal(|ui| {
                ui.label("Country code, or ALL");
                ui.add_enabled(self.visual_options.gazetteer.as_os_str().is_empty(), egui::TextEdit::singleline(&mut self.visual_country).desired_width(65.))
                    .help("ALL downloads worldwide GeoNames data (a large download). Use a two-letter code such as GB, US or AU for a smaller country catalog. This selects which visible names can match; it does not set the location-search area.")
                    .on_disabled_hover_text("A custom GeoNames file is selected. Use automatic place data to select a country again.");
            });
            if self.visual_options.gazetteer.as_os_str().is_empty() && self.visual_country.trim().eq_ignore_ascii_case("ALL") {
                ui.label(RichText::new("Worldwide data is large. Choose a country above if known.").small().color(MUTED));
            }
            if ui.button("Read video clues only")
                .help("Run the video-text scan now, using the same automatic place-data setup. This works without flight data and does not run geolocation or speech transcription. Use Analyze recording with the checkbox for the combined workflow.")
                .clicked() {
                let options = self.video_options();
                let country = self.visual_country.clone();
                let cache = self.visual_cache.clone();
                self.visual_report = None;
                self.visual_error = None;
                self.status = "Starting video clues".into();
                self.worker(ctx, move |tx, control| {
                    read_video_clues(&tx, &options, &country, &cache, &control, true);
                });
            }
            ui.collapsing("Advanced video settings", |ui| {
                ui.label("The Windows download includes the OCR tools. Override paths only if needed.");
                file_field_help(ui, &mut self.visual_options.gazetteer, "Choose GeoNames .txt…", &["txt"],
                    "Select an already extracted GeoNames place-name database. This overrides automatic downloads and the country selection. Leave the field empty for automatic setup.");
                if ui.button("Use automatic place data").help("Clear the custom GeoNames file and use the selected country's verified cache. Missing data downloads when you run video clues. No files are deleted.").clicked() {
                    self.visual_options.gazetteer = PathBuf::new();
                }
                if ui.button("Prepare place data…").help("Prepare the selected country's GeoNames data in a folder you choose, for example before going offline. Cached data is verified; missing data requires a connection. The prepared file becomes the custom database for video clues.").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let country = self.visual_country.clone();
                    self.worker(ctx, move |tx, control| {
                        let result = soundgeo::visual_data::prepare(&country, &folder, &control, &|m,p| { let _ = tx.send(Message::Progress(m.into(),p)); });
                        let _ = tx.send(match result { Ok(path) => Message::VideoData(path), Err(e) => Message::Error(format!("{e:#}")) });
                    });
                }
                ui.label(RichText::new(format!("Automatic cache: {}", self.visual_cache.display())).small());
                let mut path = self.visual_options.tesseract.clone().unwrap_or_default();
                file_field_help(ui, &mut path, "Choose Tesseract…", &["exe"],
                    "Optional: select the Tesseract OCR executable, not an installer. An empty field uses the bundled tool or the system installation. Windows users normally leave this empty.");
                self.visual_options.tesseract = (!path.as_os_str().is_empty()).then_some(path);
                if ui.small_button("Choose Tesseract language folder…").help("Optional: select the tessdata folder containing eng.traineddata. Video OCR currently uses English; this does not select the speech transcription or translation language.").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.visual_options.tessdata = Some(folder);
                }
                if let Some(path) = &self.visual_options.tessdata { ui.label(path.display().to_string()); }
                if ui.button("Use default OCR tools").help("Clear the executable and language-folder overrides. Use SoundGeo's bundled Tesseract and English data, or the system installation on other platforms. No files are deleted.").clicked() {
                    self.visual_options.tesseract = None;
                    self.visual_options.tessdata = None;
                }
            }).header_response.help("Expand to override the automatic place-data cache or bundled OCR tools, or prepare data before going offline.");
        }).header_response.help("Expand to include local reading of visible place names and clocks, or run a separate video-text scan.");
    }
    fn video_options(&self) -> soundgeo::visual::Options {
        soundgeo::visual::Options {
            media: self.job.media.clone(),
            ffmpeg: self.job.ffmpeg.clone(),
            bounds: self.use_bounds.then_some(self.bounds),
            ..self.visual_options.clone()
        }
    }
    fn results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.speech.results(ui, ctx, &mut self.error);
        if let Some(error) = &self.visual_error {
            ui.heading("Video clues need attention");
            ui.colored_label(Color32::from_rgb(255, 181, 134), error);
            ui.label("Location analysis can run independently. Review the video settings and retry the clue scan when ready.");
            ui.separator();
        }
        if let Some(report) = &self.visual_report {
            ui.heading("Video text clues");
            if report.places.is_empty() {
                ui.label("No place names matched the prepared data.");
            }
            for place in report.places.iter().take(25) {
                ui.horizontal(|ui| {
                    ui.hyperlink_to(&place.name, &place.source_url).help("Open the GeoNames source for this matched place name in your browser. A visible name does not establish the camera position.");
                    ui.label(format!(
                        "{:.6}, {:.6} · {}",
                        place.latitude, place.longitude, place.country
                    ));
                });
            }
            ui.label("Coordinates refer to named places. Review the text and map to assess the recording location.");
            egui::CollapsingHeader::new("Visible text").show(ui, |ui| {
                for frame in &report.frames {
                    ui.label(format!("{:.1}s · {}", frame.offset_s, frame.view));
                    ui.label(&frame.text);
                }
            }).header_response.help("Expand to inspect the text read from sampled video frames and its time in the recording.");
            if ui.button("Save video clue report…").help("Save the visible text, frame times, all matching places and data-source hashes as JSON. This is a separate report from the location analysis.").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("video-clues.json")
                    .save_file()
                && let Err(error) = soundgeo::visual::write_report(
                    report,
                    &path,
                    &[
                        &self.visual_inputs[0],
                        &self.visual_inputs[1],
                        &self.job.media,
                        &self.visual_options.gazetteer,
                    ],
                )
            {
                self.error = Some(error.to_string());
            }
            ui.separator();
            if self.report.is_none() {
                return;
            }
        }
        let Some(report) = &self.report else {
            ui.add_space(65.);
            ui.label(
                RichText::new("A LOCATION IS A HYPOTHESIS.")
                    .color(ACCENT)
                    .size(12.),
            );
            ui.add_space(12.);
            ui.label(RichText::new("Follow the flyovers.").size(38.).strong());
            ui.add_space(12.);
            ui.label(RichText::new("Aircraft tones change pitch as they pass. SoundGeo compares those curves with recorded flight paths to find places and times that fit.").size(18.).color(MUTED));
            ui.add_space(35.);
            for (n, title, body) in [
                (
                    "01",
                    "Extract & listen",
                    "Audio is decoded locally and scanned for descending Doppler curves.",
                ),
                (
                    "02",
                    "Match the physics",
                    "Flight position, sound travel time, elevation and one shared clock are fitted together.",
                ),
                (
                    "03",
                    "Inspect the evidence",
                    "Compare candidates, observed curves and fit errors. Export the complete report.",
                ),
            ] {
                egui::Frame::new()
                    .fill(PANEL)
                    .corner_radius(10.)
                    .inner_margin(18.)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(n).color(ACCENT).size(20.));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(title).strong().size(18.));
                                ui.label(RichText::new(body).color(MUTED));
                            });
                        });
                    });
                ui.add_space(8.);
            }
            ui.add_space(22.);
            ui.label(
                RichText::new("Experimental research tool · No cloud upload · No AI API key")
                    .color(MUTED),
            );
            return;
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("ANALYSIS").small().color(ACCENT));
                ui.heading(if report.candidate_count() == 0 {
                    "Insufficient evidence"
                } else if report.status == "ambiguous" {
                    "Ambiguous candidates"
                } else {
                    "Candidate locations"
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Export report…").help("Save the location-analysis results, candidates and matching evidence as a JSON report. Speech and video clues have their own export buttons.").clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter("JSON report", &["json"])
                        .set_file_name("soundgeo-report.json")
                        .save_file()
                {
                    if let Err(e) = soundgeo::save_report(report, &p) {
                        self.error = Some(e.to_string());
                    } else {
                        self.status = format!("Saved {}", p.display());
                    }
                }
            });
        });
        ui.label(
            RichText::new(format!(
                "{} · {} sound detections · {} flight segments · {} hypotheses",
                report
                    .media
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                report
                    .timing
                    .as_ref()
                    .map_or(report.events.len(), |t| t.peaks.len()),
                report.track_count,
                report.hypotheses
            ))
            .color(MUTED),
        );
        ui.label(
            RichText::new(format!(
                "{} · {:.0} m seed spacing",
                report.search_method.replace('_', " "),
                report.search_grid_m
            ))
            .small()
            .color(MUTED),
        );
        ui.add_space(12.);
        if let Some(timing) = &report.timing {
            timing_results(ui, ctx, report, timing, &mut self.selected);
            return;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, 0, "Locations").help("Show candidate coordinates, the location plot and competing location fits.");
            ui.selectable_value(&mut self.tab, 1, "Doppler evidence").help("Inspect the detected pitch curves and how predicted aircraft tones fit them.");
            ui.selectable_value(&mut self.tab, 2, "Limitations").help("Read the assumptions and limitations that affect interpretation of these location results.");
        });
        ui.separator();
        if self.tab == 0 {
            if report.candidates.is_empty() {
                ui.add_space(24.);
                ui.label("No location passed the matching threshold. See the evidence and limitations for what was found.");
            } else {
                let current = &report.candidates[self.selected.min(report.candidates.len() - 1)];
                if !current.silence_conflicts.is_empty() {
                    ui.colored_label(
                        Color32::from_rgb(255, 190, 100),
                        format!(
                            "{} predicted flyover(s) conflict with quiet gaps",
                            current.silence_conflicts.len()
                        ),
                    );
                }
                egui::Frame::new()
                    .fill(PANEL)
                    .corner_radius(10.)
                    .inner_margin(18.)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{:.5}, {:.5}",
                                current.latitude, current.longitude
                            ))
                            .size(28.)
                            .color(ACCENT),
                        );
                        ui.label(format!(
                            "Inferred recording start: {}",
                            current.recording_start_utc
                        ));
                        ui.label(format!(
                            "Media clock correction: {:+.0} ppm",
                            (current.recording_clock_rate - 1.) * 1e6
                        ));
                        ui.label(format!(
                            "{} of {} detections matched",
                            current.matches.len(),
                            report.events.len()
                        ));
                        ui.label(
                            RichText::new(format!(
                                "Curve error {:.3}% · a fit metric, not a confidence percentage",
                                current.relative_error_percent
                            ))
                            .color(MUTED),
                        );
                        ui.horizontal(|ui| {
                            ui.hyperlink_to(
                                "Open in OpenStreetMap",
                                format!(
                                    "https://www.openstreetmap.org/?mlat={}&mlon={}#map=15/{}/{}",
                                    current.latitude,
                                    current.longitude,
                                    current.latitude,
                                    current.longitude
                                ),
                            ).help("Open this candidate location in your web browser on OpenStreetMap. Opening the map does not verify the location.");
                            if ui.small_button("Copy coordinates").help("Copy this candidate latitude and longitude to the clipboard in decimal degrees. A candidate still needs independent verification.").clicked() {
                                ctx.copy_text(format!(
                                    "{:.6}, {:.6}",
                                    current.latitude, current.longitude
                                ));
                            }
                        });
                    });
                ui.add_space(12.);
                location_plot(ui, report, self.selected);
                ui.add_space(10.);
                for (i, c) in report.candidates.iter().enumerate() {
                    if ui
                        .selectable_label(
                            self.selected == i,
                            format!(
                                "{:02}   {:.5}, {:.5}     ·     {:.3}% curve error",
                                i + 1,
                                c.latitude,
                                c.longitude,
                                c.relative_error_percent
                            ),
                        ).help("Select this candidate to inspect its coordinates and matching evidence. Compare competing candidates before drawing a location conclusion.")
                        .clicked()
                    {
                        self.selected = i;
                    }
                }
            }
        } else if self.tab == 1 {
            if report.events.is_empty() {
                ui.label("No coherent Doppler sweep detected in the selected frequency band.");
            }
            for (i, e) in report.events.iter().enumerate() {
                let modeled = report.candidates.get(self.selected).and_then(|c| {
                    c.matches
                        .iter()
                        .enumerate()
                        .find(|(j, m)| m.event_index.unwrap_or(*j) == i)
                        .map(|(_, m)| m)
                });
                let e = modeled
                    .and_then(|m| e.variant(m.event_variant))
                    .unwrap_or(e);

                ui.label(
                    RichText::new(format!(
                        "Flyover {}  ·  {:.1}–{:.1} seconds",
                        i + 1,
                        e.start_s,
                        e.end_s
                    ))
                    .strong(),
                );
                ui.label(
                    RichText::new(format!(
                        "{:.1} Hz fitted tone  ·  {:.1} dB contrast  ·  {:.2} Hz straight-path fit",
                        e.base_hz, e.contrast_db, e.fit_rmse_hz
                    ))
                    .small()
                    .color(MUTED),
                );
                curve_plot(ui, e, modeled.map(|m| m.modeled_hz.as_slice()));
                if let Some(m) = modeled {
                    ui.label(format!(
                        "Flight {}  ·  {:.3} Hz error  ·  {:.0}% coverage",
                        m.aircraft,
                        m.rmse_hz,
                        m.coverage * 100.
                    ));
                } else if !report.candidates.is_empty() {
                    ui.label("Unmatched detection — does not support this candidate.");
                }
                ui.add_space(15.);
            }
            ui.label(
                RichText::new(
                    "Teal: observed tone   ·   Amber: predicted from the selected flight/location",
                )
                .small()
                .color(MUTED),
            );
        } else {
            ui.label(
                RichText::new("Read before using a location")
                    .strong()
                    .size(20.),
            );
            for warning in &report.warnings {
                ui.add_space(8.);
                ui.label(format!("• {warning}"));
            }
            ui.add_space(14.);
            ui.label("The report includes the input paths, parameters, observed ridges, matched flight segments, inferred time, and fit error. Keep the source flight data for reproducibility.");
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if !self.busy() {
            ctx.input(|i| {
                if let Some(p) = i.raw.dropped_files.first().and_then(|f| f.path.clone()) {
                    self.job.media = p;
                }
            });
        }
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(18.))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("SG").size(18.).strong().color(ACCENT));
                    ui.label(RichText::new("SoundGeo").size(24.).strong());
                    ui.label(RichText::new(" /  acoustic location workbench").color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(concat!("RESEARCH  ·  v", env!("CARGO_PKG_VERSION")))
                                .small()
                                .color(ACCENT),
                        );
                    });
                });
            });
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status).small().color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.hyperlink_to(
                        "Method inspiration",
                        "https://www.youtube.com/watch?v=UytYrhYLOKI",
                    ).help("Open the source YouTube video that inspired the aircraft-sound location method.");
                });
            });
        });
        egui::SidePanel::left("inputs")
            .exact_width(350.)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("inputs-scroll")
                    .show(ui, |ui| {
                        self.input_panel(ui, ctx);
                    });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(12, 20, 33))
                    .inner_margin(24.),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("results-scroll")
                    .show(ui, |ui| {
                        self.results(ui, ctx);
                    });
            });
        self.schedule_speech(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.control.stop();
    }
}
fn file_field(ui: &mut egui::Ui, path: &mut PathBuf, label: &str, extensions: &[&str]) {
    file_field_help(
        ui,
        path,
        label,
        extensions,
        match label {
            "Choose recording…" => {
                "Select a video or audio recording from this computer. Then choose Analyze recording; the original file is kept."
            }
            "Choose flight data…" => {
                "Select local historical flight data or an archive to compare with the recording. Use the archive-folder button for a directory of data."
            }
            _ => "Enter a file path or use the button to choose a file.",
        },
    );
}
fn file_field_help(
    ui: &mut egui::Ui,
    path: &mut PathBuf,
    label: &str,
    extensions: &[&str],
    help: &str,
) {
    let mut text = path.display().to_string();
    if ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .hint_text("Select a file")
                .desired_width(f32::INFINITY),
        )
        .help(help)
        .changed()
    {
        *path = PathBuf::from(text);
    }
    if ui.button(label).help(help).clicked()
        && let Some(p) = rfd::FileDialog::new()
            .add_filter("Supported files", extensions)
            .pick_file()
    {
        *path = p;
    }
}
fn read_video_clues(
    tx: &mpsc::Sender<Message>,
    options: &soundgeo::visual::Options,
    country: &str,
    cache: &std::path::Path,
    control: &Control,
    terminal: bool,
) {
    let result = soundgeo::visual::analyze_with_data(options, country, cache, control, &|m, p| {
        let _ = tx.send(Message::Progress(m.into(), p));
    });
    let _ = tx.send(match result {
        Ok((report, gazetteer)) => Message::VideoClues {
            report: Box::new(report),
            inputs: [options.media.clone(), gazetteer],
            terminal,
        },
        Err(error) => Message::VideoError(format!("{error:#}"), terminal),
    });
}

fn timing_results(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    report: &Report,
    timing: &soundgeo::timing::Assessment,
    selected: &mut usize,
) {
    ui.label(&timing.explanation);
    if timing.screen == soundgeo::timing::Screen::TimingCandidate
        && let Some(region) = &timing.region
    {
        ui.add_space(10.);
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(10.)
            .inner_margin(18.)
            .show(ui, |ui| {
                ui.label(RichText::new("Suggested area center").color(ACCENT));
                ui.label(
                    RichText::new(format!("{:.5}, {:.5}", region.latitude, region.longitude))
                        .size(28.),
                );
                ui.label(format!(
                    "The competitive search points lie within {:.0} m of this center.",
                    region.enclosing_radius_m
                ));
                ui.label("This is not a verified location error bound.");
                ui.horizontal(|ui| {
                    ui.hyperlink_to(
                        "Open area center in OpenStreetMap",
                        format!(
                            "https://www.openstreetmap.org/?mlat={}&mlon={}#map=15/{}/{}",
                            region.latitude, region.longitude, region.latitude, region.longitude
                        ),
                    ).help("Open the suggested area center in your web browser on OpenStreetMap.");
                    if ui.small_button("Copy area center").help("Copy the suggested area center latitude and longitude to the clipboard. The area spread is not a verified location-error bound.").clicked() {
                        ctx.copy_text(format!("{:.6}, {:.6}", region.latitude, region.longitude));
                    }
                });
            });
    }
    if timing.candidate_count() > 0
        && let Some(search) = &timing.search
        && let Some(candidate) = search
            .candidates
            .get((*selected).min(search.candidates.len().saturating_sub(1)))
    {
        ui.add_space(10.);
        egui::Frame::new().fill(PANEL).corner_radius(10.).inner_margin(18.).show(ui,|ui| {
            ui.label(RichText::new(format!("{:.5}, {:.5}",candidate.latitude,candidate.longitude)).size(28.).color(ACCENT));
            ui.label("Selected search candidate");
            if let Some(time) = chrono::DateTime::from_timestamp_millis((candidate.offset_utc*1000.).round() as i64) {
                ui.label(format!("Inferred start: {}",time.to_rfc3339()));
            }
            ui.label(format!("{} matched sound peaks out of {}",candidate.pairs.len(),timing.peaks.len()));
            ui.label(format!("Timing score: {:.3}. This is not a confidence percentage.",candidate.log_likelihood_ratio));
            if let Some(spread) = timing.competitive_location_spread_m {
                ui.label(format!("Spread of competitive search locations: {:.0} m. This is not an accuracy radius.",spread));
            }
            ui.horizontal(|ui| {
                ui.hyperlink_to("Open in OpenStreetMap",format!("https://www.openstreetmap.org/?mlat={}&mlon={}#map=15/{}/{}",candidate.latitude,candidate.longitude,candidate.latitude,candidate.longitude)).help("Open this candidate location in your web browser on OpenStreetMap. Opening the map does not verify the location.");
                if ui.small_button("Copy coordinates").help("Copy this candidate latitude and longitude to the clipboard in decimal degrees. A candidate still needs independent verification.").clicked() {ctx.copy_text(format!("{:.6}, {:.6}",candidate.latitude,candidate.longitude));}
            });
        });
        ui.add_space(10.);
        egui::ScrollArea::vertical()
            .id_salt("timing-candidates")
            .max_height(180.)
            .show(ui, |ui| {
                for (index, candidate) in search.candidates.iter().enumerate() {
                    if ui
                        .selectable_label(
                            *selected == index,
                            format!(
                                "{:02}  {:.5}, {:.5}  ·  score {:.3}",
                                index + 1,
                                candidate.latitude,
                                candidate.longitude,
                                candidate.log_likelihood_ratio
                            ),
                        ).help("Select this candidate to inspect its coordinates and matching evidence. Compare competing candidates before drawing a location conclusion.")
                        .clicked()
                    {
                        *selected = index;
                    }
                }
            });
        ui.collapsing("Matched sound peaks", |ui| {
            for pair in &candidate.pairs {
                if let Some(peak) = timing.peaks.get(pair.peak) {
                    ui.label(format!(
                        "{:.1} s · flight {} · timing difference {:+.2} s",
                        peak.time_s, pair.track, pair.residual_s
                    ));
                }
            }
        }).header_response.help("Expand to inspect the sound peaks paired with flight passages for the selected timing candidate.");
    }
    ui.collapsing("Sound peaks and shuffled controls", |ui| {
        ui.label(format!(
            "{} sound peaks; {} of {} controls completed.",
            timing.peaks.len(),
            timing.control_maximum_scores.len(),
            timing.required_controls
        ));
        if let Some(maximum) = timing
            .control_maximum_scores
            .iter()
            .copied()
            .reduce(f64::max)
        {
            ui.label(format!("Highest shuffled-control score: {maximum:.3}"));
        }
        for peak in &timing.peaks {
            ui.label(format!(
                "{:.1} s · {:.1} dB above background · {:.1} s wide",
                peak.time_s, peak.excess_db, peak.width_s
            ));
        }
    }).header_response.help("Expand to inspect detected sound peaks and the shuffled-sound control results used to screen the timing signal.");
    ui.collapsing("Limitations", |ui| {
        for warning in &report.warnings {
            ui.label(warning);
        }
    }).header_response.help("Expand to read the assumptions and limitations that affect interpretation of these timing results.");
}

fn location_plot(ui: &mut egui::Ui, report: &Report, selected: usize) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 210.), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8., PANEL);
    let pts = &report.candidates;
    let lat0 = pts.iter().map(|p| p.latitude).fold(f64::INFINITY, f64::min) - 0.003;
    let lat1 = pts
        .iter()
        .map(|p| p.latitude)
        .fold(f64::NEG_INFINITY, f64::max)
        + 0.003;
    let lon0 = pts
        .iter()
        .map(|p| p.longitude)
        .fold(f64::INFINITY, f64::min)
        - 0.003;
    let lon1 = pts
        .iter()
        .map(|p| p.longitude)
        .fold(f64::NEG_INFINITY, f64::max)
        + 0.003;
    for i in 1..5 {
        let x = rect.left() + rect.width() * i as f32 / 5.;
        let y = rect.top() + rect.height() * i as f32 / 5.;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(1.0_f32, Color32::from_rgb(37, 50, 69)),
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            Stroke::new(1.0_f32, Color32::from_rgb(37, 50, 69)),
        );
    }
    for (i, p) in pts.iter().enumerate() {
        let xy = egui::pos2(
            rect.left()
                + 20.
                + ((p.longitude - lon0) / (lon1 - lon0)) as f32 * (rect.width() - 40.),
            rect.bottom()
                - 20.
                - ((p.latitude - lat0) / (lat1 - lat0)) as f32 * (rect.height() - 40.),
        );
        painter.circle_filled(
            xy,
            if i == selected { 8. } else { 5. },
            if i == selected { ACCENT } else { MUTED },
        );
        painter.text(
            xy + Vec2::new(12., 0.),
            egui::Align2::LEFT_CENTER,
            (i + 1).to_string(),
            egui::FontId::proportional(13.),
            Color32::WHITE,
        );
    }
    painter.text(
        rect.left_top() + Vec2::new(12., 12.),
        egui::Align2::LEFT_TOP,
        "North is up   ·   Candidate spread · schematic coordinates",
        egui::FontId::proportional(12.),
        MUTED,
    );
}
fn curve_plot(ui: &mut egui::Ui, event: &soundgeo::audio::Event, modeled: Option<&[f64]>) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 155.), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8., PANEL);
    let min = event
        .ridge
        .iter()
        .map(|p| p.hz)
        .fold(f64::INFINITY, f64::min)
        - 3.;
    let max = event
        .ridge
        .iter()
        .map(|p| p.hz)
        .fold(f64::NEG_INFINITY, f64::max)
        + 3.;
    let xy = |t: f64, hz: f64| {
        egui::pos2(
            rect.left()
                + 40.
                + ((t - event.start_s) / (event.end_s - event.start_s)) as f32
                    * (rect.width() - 52.),
            rect.bottom() - 15. - ((hz - min) / (max - min)) as f32 * (rect.height() - 30.),
        )
    };
    for pair in event.ridge.windows(2) {
        painter.line_segment(
            [
                xy(pair[0].time_s, pair[0].hz),
                xy(pair[1].time_s, pair[1].hz),
            ],
            Stroke::new(2.0_f32, ACCENT),
        );
    }
    if let Some(modeled) = modeled {
        for (i, pair) in modeled.windows(2).enumerate() {
            if pair[0] > 0. && pair[1] > 0. && i + 1 < event.ridge.len() {
                painter.line_segment(
                    [
                        xy(event.ridge[i].time_s, pair[0]),
                        xy(event.ridge[i + 1].time_s, pair[1]),
                    ],
                    Stroke::new(1.5_f32, Color32::from_rgb(255, 190, 100)),
                );
            }
        }
    }
    painter.text(
        rect.left_top() + Vec2::new(7., 8.),
        egui::Align2::LEFT_TOP,
        format!("{max:.0}\nHz"),
        egui::FontId::proportional(11.),
        MUTED,
    );
    painter.text(
        rect.left_bottom() + Vec2::new(7., -8.),
        egui::Align2::LEFT_BOTTOM,
        format!("{min:.0}"),
        egui::FontId::proportional(11.),
        MUTED,
    );
}

#[cfg(test)]
mod video_workflow_tests {
    use super::*;

    #[test]
    fn downloaded_recording_is_selected_and_feedback_survives_later_analysis() {
        let mut app = App::initial(None);
        let (tx, rx) = mpsc::channel();
        app.receiver = Some(rx);
        app.media_feedback = MediaFeedback::Working("Downloading".into());
        tx.send(Message::Progress("Verifying recording".into(), 0.9))
            .unwrap();
        app.poll();
        assert!(app.busy());
        assert!(
            matches!(&app.media_feedback, MediaFeedback::Working(message) if message == "Verifying recording")
        );
        let download = soundgeo::media_url::Download {
            schema: 1,
            source: soundgeo::media_url::Listing {
                schema: 1,
                source_url: "https://example.com/video".into(),
                engine: soundgeo::media_url::Engine::YtDlp,
                engine_version: "fixture".into(),
                impersonate: false,
                fetched_utc: String::new(),
                candidates: vec![],
                notices: vec![],
            },
            selected_item: 1,
            path: "selected-clip.mkv".into(),
            sha256: "fixture".into(),
            size: 1234,
            duration_s: 60.,
            has_audio: true,
            has_video: true,
            downloaded_utc: String::new(),
        };
        tx.send(Message::MediaDownloaded(Box::new(download)))
            .unwrap();
        app.poll();
        assert_eq!(app.job.media, PathBuf::from("selected-clip.mkv"));
        assert!(!app.busy()); // Downloading alone must not start a flight search.
        assert!(
            matches!(&app.media_feedback, MediaFeedback::Ready(download) if download.path == app.job.media)
        );
        let (tx, rx) = mpsc::channel();
        app.receiver = Some(rx);
        tx.send(Message::Progress("Analyzing audio".into(), 0.2))
            .unwrap();
        tx.send(Message::Error("Flight data needs attention".into()))
            .unwrap();
        app.poll();
        assert!(matches!(app.media_feedback, MediaFeedback::Ready(_)));
        assert_eq!(app.job.media, PathBuf::from("selected-clip.mkv"));
    }

    #[test]
    fn media_failure_and_cancellation_leave_visible_feedback() {
        let mut app = App::initial(None);
        for error in ["Download failed", "Cancelled"] {
            let (tx, rx) = mpsc::channel();
            app.receiver = Some(rx);
            app.media_feedback = MediaFeedback::Working("Downloading".into());
            tx.send(Message::Error(error.into())).unwrap();
            app.poll();
            assert!(!app.busy());
            match &app.media_feedback {
                MediaFeedback::Error(message) => assert_eq!(message, "Download failed"),
                MediaFeedback::Info(message) => assert!(message.contains("cancelled")),
                _ => panic!("A stopped media task must leave feedback beside the URL controls"),
            }
        }
    }

    #[test]
    fn optional_video_messages_keep_the_combined_worker_alive() {
        let mut app = App::initial(None);
        let (tx, rx) = mpsc::channel();
        app.receiver = Some(rx);
        tx.send(Message::VideoError("No video stream".into(), false))
            .unwrap();
        app.poll();
        assert!(app.busy());
        assert_eq!(app.visual_error.as_deref(), Some("No video stream"));
        let report = soundgeo::visual::Report {
            method: "video_text".into(),
            status: "no_place_clues".into(),
            offline: true,
            media_sha256: "media".into(),
            gazetteer_sha256: "data".into(),
            gazetteer_source: None,
            ocr_version: "fixture".into(),
            bounds: None,
            frames: vec![],
            places: vec![],
            warnings: vec![],
        };
        tx.send(Message::VideoClues {
            report: Box::new(report),
            inputs: ["original.mp4".into(), "actual-cache.txt".into()],
            terminal: false,
        })
        .unwrap();
        app.poll();
        assert!(app.busy());
        assert!(app.visual_error.is_none());
        app.job.media = "different.mp4".into();
        assert_eq!(app.visual_inputs[0], PathBuf::from("original.mp4"));
        assert_eq!(app.visual_inputs[1], PathBuf::from("actual-cache.txt"));
        tx.send(Message::Error("Geolocation needs attention".into()))
            .unwrap();
        app.poll();
        assert!(!app.busy());
        assert!(app.visual_report.is_some());
    }

    #[test]
    fn checkbox_runs_optional_stage_despite_invalid_flight_setup() {
        let mut app = App::initial(None);
        let ctx = egui::Context::default();
        app.run(&ctx);
        assert!(!app.busy()); // Disabled by default: no optional work.
        assert!(app.error.is_some());
        assert!(app.visual_error.is_none());
        app.visual_enabled = true;
        app.run(&ctx);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.busy() && std::time::Instant::now() < deadline {
            app.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.busy());
        assert!(app.visual_error.as_ref().unwrap().contains("local video"));
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .contains("Geolocation needs attention")
        );
    }

    #[test]
    fn standalone_video_finishes_and_cancel_stays_cancelled() {
        let mut app = App::initial(None);
        let (tx, rx) = mpsc::channel();
        app.receiver = Some(rx);
        app.control.stop();
        read_video_clues(
            &tx,
            &app.video_options(),
            "ALL",
            &app.visual_cache,
            &app.control,
            true,
        );
        app.poll();
        assert!(!app.busy());
        assert_eq!(app.status, "Cancelled");
        assert_eq!(app.visual_error.as_deref(), Some("Cancelled"));
        assert!(app.visual_report.is_none());
    }
}
