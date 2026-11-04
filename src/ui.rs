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
            job: Job::default(),
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
        self.control = Control::default();
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
                Message::Downloaded(path) => {
                    self.job.tracks = path;
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
            file_field(ui,&mut self.job.tracks,"Choose flight data…",&["csv","json","gz","aa","tar"]);
            if ui.small_button("Or choose an archive folder").clicked()&& let Some(p)=rfd::FileDialog::new().pick_folder(){self.job.tracks=p;}
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
                ui.checkbox(&mut self.use_bounds,"Limit the search area");
                if self.use_bounds {egui::Grid::new("bounds").num_columns(2).show(ui,|ui|{
                    for (label,value) in [("South",&mut self.bounds.south),("North",&mut self.bounds.north),("West",&mut self.bounds.west),("East",&mut self.bounds.east)]{ui.label(label);ui.add(egui::DragValue::new(value).speed(0.001).max_decimals(5));ui.end_row();}
                });}else{ui.label("Uses the flight-data extent. Narrow a large archive for a finer search.");}
                ui.horizontal(|ui|{ui.label("Fallback elevation (m)");ui.add(egui::DragValue::new(&mut self.job.ground_m).range(-500. ..=9000.));});
                if ui.button("Choose SRTM terrain folder…").clicked(){self.job.terrain=rfd::FileDialog::new().pick_folder();}
                if let Some(p)=&self.job.terrain{ui.label(RichText::new(p.display().to_string()).small());if ui.small_button("Clear terrain").clicked(){self.job.terrain=None;}}
            });
            egui::CollapsingHeader::new("Advanced audio & search").show(ui,|ui|{
                egui::Grid::new("advanced").num_columns(2).show(ui,|ui|{
                    for (label,value) in [("Low frequency (Hz)",&mut self.job.min_hz),("High frequency (Hz)",&mut self.job.max_hz),("Min contrast (dB)",&mut self.job.min_snr_db),("Temperature (°C)",&mut self.job.temperature_c),("Grid target (m)",&mut self.job.grid_m)]{ui.label(label);ui.add(egui::DragValue::new(value).speed(1.));ui.end_row();}
                    ui.label("Grid point budget");ui.add(egui::DragValue::new(&mut self.job.max_grid_points).range(100..=50000));ui.end_row();
                });
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
                ui.heading(if report.candidates.is_empty() {
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
        ui.label(RichText::new(format!("{} flyovers   /   {} flight segments   /   {:.0} s audio   /   {:.0} m initial grid",report.events.len(),report.track_count,report.duration_s,report.search_grid_m)).color(MUTED));
        ui.add_space(12.);
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
                let modeled = report
                    .candidates
                    .get(self.selected)
                    .and_then(|c| c.matches.get(i));
                curve_plot(ui, e, modeled.map(|m| m.modeled_hz.as_slice()));
                if let Some(m) = modeled {
                    ui.label(format!(
                        "Flight {}  ·  {:.3} Hz error  ·  {:.0}% coverage",
                        m.aircraft,
                        m.rmse_hz,
                        m.coverage * 100.
                    ));
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
                        ui.label(RichText::new("EXPERIMENTAL  ·  v0.1").small().color(ACCENT));
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
            Stroke::new(1., Color32::from_rgb(37, 50, 69)),
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            Stroke::new(1., Color32::from_rgb(37, 50, 69)),
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
            Stroke::new(2., ACCENT),
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
                    Stroke::new(1.5, Color32::from_rgb(255, 190, 100)),
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
