use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use soundgeo::{Control, Job, Report, geo::Bounds};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

const ACCENT: Color32 = Color32::from_rgb(76, 224, 194);
const MUTED: Color32 = Color32::from_rgb(147, 164, 188);
const PANEL: Color32 = Color32::from_rgb(21, 31, 47);
enum Message {
    Progress(String, f32),
    Done(Box<Report>),
    Demo(Box<Job>),
    Downloaded(PathBuf),
    Catalog(PathBuf),
    Prepared(usize),
    Error(String),
}
pub struct App {
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
        Self {
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
        }
    }
    fn busy(&self) -> bool {
        self.receiver.is_some()
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
        job.start_utc = if self.clock.trim().is_empty() {
            None
        } else {
            match soundgeo::data::parse_time(self.clock.trim()) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.error = Some(e.to_string());
                    return;
                }
            }
        };
        job.bounds = self.use_bounds.then_some(self.bounds);
        if let Err(e) = job.validate() {
            self.error = Some(e.to_string());
            return;
        }
        self.report = None;
        self.selected = 0;
        self.status = "Starting analysis".into();
        self.worker(ctx, move |tx, control| {
            let progress = |m: &str, p: f32| {
                let _ = tx.send(Message::Progress(m.into(), p));
            };
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
                Message::Progress(m, p) => {
                    self.status = m;
                    self.progress = p;
                }
                Message::Done(report) => {
                    self.report = Some(*report);
                    self.status = "Analysis complete".into();
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
        ui.add_enabled_ui(!self.busy(),|ui|{
            ui.label(RichText::new("1  Video or audio").strong());
            file_field(ui,&mut self.job.media,"Choose recording…",&["mp4","mov","mkv","webm","avi","wav","mp3","flac","m4a"]);
            ui.label(RichText::new("You can also drop a file into this window.").small().color(MUTED));
            ui.add_space(6.);ui.label(RichText::new("2  Historical flight data").strong());
            ui.checkbox(&mut self.job.offline, "Offline mode: use cached data only");
            let mut automatic = self.job.archive.is_some();
            if ui.checkbox(&mut automatic,"Download missing flight data automatically").changed() {
                self.job.archive = automatic.then(|| soundgeo::archive::ArchiveRequest {date_from:self.date.clone(),date_to:None,cache:PathBuf::from("flight-cache"),provider:Default::default()});
            }
            if let Some(request) = &mut self.job.archive {
                use soundgeo::archive::ArchiveProvider;
                egui::ComboBox::from_id_salt("flight_provider").selected_text(match request.provider {
                    ArchiveProvider::AdsbLol => "Worldwide: ADSB.lol",
                    ArchiveProvider::AdsbExchangeSample => "Free evaluation sample: first of each month",
                    ArchiveProvider::WebtrakChicago => "Chicago: public airport replay",
                    ArchiveProvider::WebtrakLax => "LAX: public airport replay",
                }).show_ui(ui,|ui|{
                    ui.selectable_value(&mut request.provider,ArchiveProvider::AdsbLol,"Worldwide: ADSB.lol");
                    ui.selectable_value(&mut request.provider,ArchiveProvider::AdsbExchangeSample,"Free evaluation sample: first of each month");
                    ui.selectable_value(&mut request.provider,ArchiveProvider::WebtrakChicago,"Chicago: public airport replay");
                    ui.selectable_value(&mut request.provider,ArchiveProvider::WebtrakLax,"LAX: public airport replay");
                });
                ui.label(RichText::new("Optional date range. Leave dates blank to search every cached date.").small().color(MUTED));
                if request.provider == ArchiveProvider::AdsbExchangeSample {
                    ui.label(RichText::new("Use the first day of a month and set a search area below. This free sample is for testing and evaluation. It can miss flights between five-minute discovery snapshots.").small().color(MUTED));
                    ui.hyperlink_to("Free sample coverage and terms", "https://www.adsbexchange.com/data-products/sample-data/");
                }
                ui.add(egui::TextEdit::singleline(&mut request.date_from).hint_text("First date, or blank for all cached dates").desired_width(f32::INFINITY));
                let mut several = request.date_to.is_some();
                if ui.checkbox(&mut several,"Search several possible dates").changed(){request.date_to=several.then(String::new);}
                if let Some(to)=&mut request.date_to{ui.add(egui::TextEdit::singleline(to).hint_text("Last date, inclusive (YYYY-MM-DD)").desired_width(f32::INFINITY));}
                let mut cache=request.cache.display().to_string();
                if ui.text_edit_singleline(&mut cache).changed(){request.cache=PathBuf::from(cache);}
                if ui.small_button("Choose cache folder…").clicked() && let Some(path)=rfd::FileDialog::new().pick_folder(){request.cache=path;}
            } else {
            file_field(ui,&mut self.job.tracks,"Choose flight data…",&["csv","json","gz","aa","tar"]);
            if ui.small_button("Or choose an archive folder").clicked()&& let Some(p)=rfd::FileDialog::new().pick_folder(){self.job.tracks=p;}
            }
            if self.job.archive.is_some() && ui.add_enabled(!self.job.offline, egui::Button::new("Prepare data for offline use")).clicked() {
                let mut job = self.job.clone();
                job.bounds = self.use_bounds.then_some(self.bounds);
                self.status = "Preparing offline data".into();
                self.worker(ctx, move |tx, control| {
                    let result = soundgeo::prepare_data(&job, &control, &|m,p| { let _ = tx.send(Message::Progress(m.into(),p)); });
                    let _ = tx.send(match result { Ok(data) => Message::Prepared(data.flight_sources.len()), Err(e) => Message::Error(format!("{e:#}")) });
                });
            }
            ui.label(RichText::new("Searches all dates in the supplied data. The recording stays on your computer.").small().color(MUTED));
            egui::CollapsingHeader::new("Download a public flight archive").show(ui,|ui|{
                ui.label("Choose a possible recording date. Each day is several GB; no API key is needed.");
                ui.add(egui::TextEdit::singleline(&mut self.date).hint_text("YYYY-MM-DD").desired_width(f32::INFINITY));
                if ui.button("Download day…").clicked(){
                    if chrono::NaiveDate::parse_from_str(&self.date,"%Y-%m-%d").is_err(){self.error=Some("Enter an archive date in YYYY-MM-DD format.".into());}
                    else if let Some(dir)=rfd::FileDialog::new().set_title("Choose flight archive cache").pick_folder(){
                        let date=self.date.clone();self.status="Downloading flight archive".into();
                        self.worker(ctx,move|tx,control|{let result=soundgeo::data::download_day(&date,&dir,&control,&|m,p|{let _=tx.send(Message::Progress(m.into(),p));});let _=tx.send(match result{Ok(p)=>Message::Downloaded(p),Err(e)=>Message::Error(format!("{e:#}"))});});
                    }
                }
                ui.hyperlink_to("Archive coverage and data licence","https://www.adsb.lol/docs/open-data/historical");
            });
            ui.add_space(6.);ui.label(RichText::new("3  Recording time").strong());
            ui.add(egui::TextEdit::singleline(&mut self.clock).hint_text("Unknown — infer from flyovers").desired_width(f32::INFINITY));
            ui.label(RichText::new("Optional: 2026-09-08T10:00:00Z. With no time, two separate flyovers are much more useful.").small().color(MUTED));
            if !self.clock.trim().is_empty(){ui.horizontal(|ui|{ui.label("Allow ± seconds");ui.add(egui::DragValue::new(&mut self.job.time_tolerance_s).range(0. ..=86400.));});}
            ui.add_space(6.);
            egui::CollapsingHeader::new("Search area & terrain").show(ui,|ui|{
                self.aircraft_controls(ui,ctx);
                ui.checkbox(&mut self.use_bounds,"Limit the search area");
                if self.use_bounds {egui::Grid::new("bounds").num_columns(2).show(ui,|ui|{
                    for (label,value) in [("South",&mut self.bounds.south),("North",&mut self.bounds.north),("West",&mut self.bounds.west),("East",&mut self.bounds.east)]{ui.label(label);ui.add(egui::DragValue::new(value).speed(0.001).max_decimals(5));ui.end_row();}
                });}else{ui.label("Uses the flight-data extent. Narrow a large archive for a finer search.");}
                ui.horizontal(|ui|{ui.label("Fallback elevation (m)");ui.add(egui::DragValue::new(&mut self.job.ground_m).range(-500. ..=9000.));});
                ui.label("Fallback height datum");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.job.ground_datum, soundgeo::geo::ElevationDatum::Egm96, "Sea level (EGM96)");
                    ui.selectable_value(&mut self.job.ground_datum, soundgeo::geo::ElevationDatum::Wgs84Ellipsoid, "WGS84 ellipsoid");
                });
                ui.checkbox(&mut self.job.smooth_tracks, "Reduce flight-position noise")
                    .on_hover_text("Centered 10-second local linear fit. Keeps track endpoints and timestamps; can blur rapid maneuvers.");
                if ui.checkbox(&mut self.job.download_terrain,"Download missing terrain from free sources").changed() && self.job.download_terrain && self.job.terrain.is_none() {
                    self.job.terrain=Some(PathBuf::from("terrain-cache"));
                }
                if self.job.download_terrain {
                    ui.label("Set a search area. Terrain gives ground height, not microphone height above ground.");
                    ui.hyperlink_to("Mapzen Terrain Tiles and provider credits", "https://github.com/tilezen/joerd/blob/master/docs/attribution.md");
                }
                if ui.button("Choose terrain folder…").clicked() && let Some(path)=rfd::FileDialog::new().pick_folder(){self.job.terrain=Some(path);}
                if let Some(p)=&self.job.terrain{ui.label(RichText::new(p.display().to_string()).small());if ui.small_button("Clear terrain").clicked(){self.job.terrain=None;self.job.download_terrain=false;}}
            });
            ui.label("Sound matching method");
            egui::ComboBox::from_id_salt("analysis-method")
                .selected_text(match self.job.analysis_method {soundgeo::AnalysisMethod::Doppler=>"Doppler curves",soundgeo::AnalysisMethod::PassageTiming=>"Flight passage timing"})
                .show_ui(ui,|ui| {
                    ui.selectable_value(&mut self.job.analysis_method,soundgeo::AnalysisMethod::Doppler,"Doppler curves");
                    ui.selectable_value(&mut self.job.analysis_method,soundgeo::AnalysisMethod::PassageTiming,"Flight passage timing");
                });
            if self.job.analysis_method == soundgeo::AnalysisMethod::PassageTiming {
                ui.label("Set a search area. Use at least eight minutes of continuous audio. The app checks 19 shuffled-sound controls before it reports a timing signal.");
                ui.label("Timing results are possible locations. Their accuracy is not guaranteed.");
            }
            egui::CollapsingHeader::new("Advanced audio & search").show(ui,|ui|{
                if self.job.analysis_method == soundgeo::AnalysisMethod::Doppler {
                ui.checkbox(&mut self.job.fast_flyovers,"Also detect short flyovers (500–3900 Hz)");
                if self.job.fast_flyovers {ui.horizontal(|ui|{ui.label("Short-pass contrast (dB)");ui.add(egui::DragValue::new(&mut self.job.fast_min_snr_db).range(6. ..=60.));});}
                use soundgeo::search::SearchMode;
                egui::ComboBox::from_id_salt("search-mode").selected_text(match self.job.search_mode{SearchMode::Auto=>"Automatic search method",SearchMode::RegionalGrid=>"Regional grid",SearchMode::FlightCorridors=>"Indexed flight corridors"}).show_ui(ui,|ui|{
                    ui.selectable_value(&mut self.job.search_mode,SearchMode::Auto,"Automatic search method");ui.selectable_value(&mut self.job.search_mode,SearchMode::FlightCorridors,"Indexed flight corridors");ui.selectable_value(&mut self.job.search_mode,SearchMode::RegionalGrid,"Regional grid");
                });
                egui::Grid::new("advanced").num_columns(2).show(ui,|ui|{
                    for (label,value) in [("Low frequency (Hz)",&mut self.job.min_hz),("High frequency (Hz)",&mut self.job.max_hz),("Min contrast (dB)",&mut self.job.min_snr_db),("Temperature (°C)",&mut self.job.temperature_c),("Grid target (m)",&mut self.job.grid_m)]{ui.label(label);ui.add(egui::DragValue::new(value).speed(1.));ui.end_row();}
                    ui.label("Media clock drift limit (ppm)");ui.add(egui::DragValue::new(&mut self.job.max_clock_drift_ppm).range(0. ..=5000.));ui.end_row();
                    ui.label("Grid point budget");ui.add(egui::DragValue::new(&mut self.job.max_grid_points).range(100..=50000));ui.end_row();
                    ui.label("Indexed hypothesis limit");ui.add(egui::DragValue::new(&mut self.job.max_hypotheses).range(100..=100_000_000));ui.end_row();
                });
                } else {
                    egui::Grid::new("timing-options").num_columns(2).show(ui,|ui| {
                        ui.label("Grid spacing (m)");ui.add(egui::DragValue::new(&mut self.job.timing_options.grid_m).range(50. ..=1000.));ui.end_row();
                        ui.label("Grid point limit");ui.add(egui::DragValue::new(&mut self.job.timing_options.maximum_grid_points).range(100..=50000));ui.end_row();
                        ui.label("Maximum flight distance (m)");ui.add(egui::DragValue::new(&mut self.job.timing_options.maximum_passage_distance_m).range(100. ..=5000.));ui.end_row();
                        ui.label("Temperature (°C)");ui.add(egui::DragValue::new(&mut self.job.temperature_c).range(-50. ..=60.));ui.end_row();
                    });
                }
                if ui.button("Choose FFmpeg executable…").clicked(){self.job.ffmpeg=rfd::FileDialog::new().pick_file();}
            });
            ui.add_space(10.);
            if ui.add_sized([ui.available_width(),42.],egui::Button::new(RichText::new("Analyze recording").strong().color(Color32::from_rgb(9,26,30))).fill(ACCENT)).clicked(){self.run(ctx);}
            if ui.add_sized([ui.available_width(),34.],egui::Button::new("Try a synthetic demo")).clicked(){self.load_demo(ctx);}
        });
        if self.busy() {
            ui.add_space(12.);
            ui.add(egui::ProgressBar::new(self.progress).animate(true));
            ui.label(&self.status);
            if ui.button("Cancel").clicked() {
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
                );
                ui.selectable_value(
                    &mut self.job.aircraft_filter,
                    AircraftFilter::PropellerOnly,
                    "Known propellers only",
                );
                ui.selectable_value(
                    &mut self.job.aircraft_filter,
                    AircraftFilter::All,
                    "All aircraft",
                );
            });
        if ui.button("Choose aircraft type catalog…").clicked() {
            self.job.type_catalog = rfd::FileDialog::new()
                .add_filter("Type catalog", &["json"])
                .pick_file();
        }
        if ui.button("Download type catalog…").clicked()
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
                );
                ui.selectable_value(
                    &mut self.job.silence_mode,
                    SilenceMode::Strict,
                    "Reject conflicting flyovers",
                );
                ui.selectable_value(
                    &mut self.job.silence_mode,
                    SilenceMode::Off,
                    "Ignore quiet gaps",
                );
            });
        ui.label(RichText::new("Strict mode assumes closer known propellers would be audible. It can reject the true location.").small().color(MUTED));
        ui.separator();
    }
    fn results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
                if ui.button("Export report…").clicked()
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
            ui.selectable_value(&mut self.tab, 0, "Locations");
            ui.selectable_value(&mut self.tab, 1, "Doppler evidence");
            ui.selectable_value(&mut self.tab, 2, "Limitations");
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
                            );
                            if ui.small_button("Copy coordinates").clicked() {
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
                        )
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
                    );
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
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.control.stop();
    }
}
fn file_field(ui: &mut egui::Ui, path: &mut PathBuf, label: &str, extensions: &[&str]) {
    let mut text = path.display().to_string();
    if ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .hint_text("Select a file")
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        *path = PathBuf::from(text);
    }
    if ui.button(label).clicked()
        && let Some(p) = rfd::FileDialog::new()
            .add_filter("Supported files", extensions)
            .pick_file()
    {
        *path = p;
    }
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
                    );
                    if ui.small_button("Copy area center").clicked() {
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
                ui.hyperlink_to("Open in OpenStreetMap",format!("https://www.openstreetmap.org/?mlat={}&mlon={}#map=15/{}/{}",candidate.latitude,candidate.longitude,candidate.latitude,candidate.longitude));
                if ui.small_button("Copy coordinates").clicked() {ctx.copy_text(format!("{:.6}, {:.6}",candidate.latitude,candidate.longitude));}
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
                        )
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
        });
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
    });
    ui.collapsing("Limitations", |ui| {
        for warning in &report.warnings {
            ui.label(warning);
        }
    });
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
