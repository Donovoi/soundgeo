use super::{App, MUTED, Message};
use eframe::egui::{self, RichText};
use soundgeo::{
    Control, Job, Progress,
    speech::{self, ExportFormat, Options, Prepared, TranslationQuality},
};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(super) struct State {
    pub options: Options,
    pub prepared: Option<Arc<Prepared>>,
    pub report: Option<speech::Report>,
    request_key: Option<String>,
    renderer: Option<speech::text::TextRenderer>,
    font_error: Option<String>,
    textures: HashMap<usize, (egui::TextureHandle, bool)>,
    preview_width: usize,
    preview_scale: f32,
    page: usize,
    export_format: Option<ExportFormat>,
    export_english: bool,
}

impl State {
    fn key(&self, job: &Job) -> String {
        let metadata = job.media.metadata().ok();
        format!(
            "{}|{:?}|{:?}|{}|{}",
            job.media.display(),
            job.ffmpeg,
            metadata
                .as_ref()
                .and_then(|m| m.modified().ok())
                .zip(metadata.as_ref().map(|m| m.len())),
            job.offline,
            serde_json::to_string(&self.options).unwrap_or_default()
        )
    }

    pub fn current_prepared(&self, job: &Job) -> Option<Arc<Prepared>> {
        (self.request_key.as_ref() == Some(&self.key(job)))
            .then(|| self.prepared.clone())
            .flatten()
    }

    pub fn mark_requested(&mut self, job: &Job) {
        self.request_key = Some(self.key(job));
    }

    pub fn clear_preview(&mut self) {
        self.textures.clear();
        self.page = 0;
    }
    pub fn invalidate(&mut self) {
        self.prepared = None;
        self.report = None;
        self.request_key = None;
        self.clear_preview();
    }

    pub fn results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, error: &mut Option<String>) {
        let Some(report) = &self.report else {
            return;
        };
        ui.heading("Speech transcript and English translation");
        ui.label(format!(
            "Status: {:?} · Language: {} · {} intervals",
            report.status,
            report
                .selected_language
                .as_deref()
                .unwrap_or("awaiting choice"),
            report.segments.len()
        ));
        ui.label(
            RichText::new(format!("Recording: {}", report.source.path.display()))
                .small()
                .color(MUTED),
        );
        for warning in &report.warnings {
            ui.label(warning);
        }
        if !report.detected.is_empty() {
            ui.collapsing("Language detection details", |ui| {
                ui.label("Model scores are not calibrated probabilities. Detection samples up to three speech intervals.");
                for detection in &report.detected {
                    ui.label(format!("{:.1}–{:.1}s: {} · score {:.3}", detection.start_s,
                        detection.end_s, detection.language, detection.score));
                }
            });
        }
        if report.segments.is_empty() {
            ui.separator();
            return;
        }
        ui.label("Review original speech before relying on a translation. Speech text does not change the location acceptance rules.");
        let mut format = self.export_format.unwrap_or(ExportFormat::Json);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("speech-export")
                .selected_text(format!("{format:?}"))
                .show_ui(ui, |ui| {
                    for candidate in [
                        ExportFormat::Json,
                        ExportFormat::Txt,
                        ExportFormat::Srt,
                        ExportFormat::Vtt,
                    ] {
                        ui.selectable_value(&mut format, candidate, format!("{candidate:?}"));
                    }
                });
            if format != ExportFormat::Json {
                ui.checkbox(&mut self.export_english, "English translation");
            }
            if ui.button("Save speech report…").clicked() {
                let extension = format!("{format:?}").to_lowercase();
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(format!("speech-report.{extension}"))
                    .save_file()
                    && let Err(value) =
                        speech::write_report(report, &path, format, self.export_english)
                {
                    *error = Some(value.to_string());
                }
            }
        });
        self.export_format = Some(format);
        let pages = report.segments.len().div_ceil(10);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.page > 0, egui::Button::new("Previous intervals"))
                .clicked()
            {
                self.page -= 1;
                self.textures.clear();
            }
            ui.label(format!("Page {} of {pages}", self.page + 1));
            if ui
                .add_enabled(self.page + 1 < pages, egui::Button::new("Next intervals"))
                .clicked()
            {
                self.page += 1;
                self.textures.clear();
            }
        });
        let scale = ctx.pixels_per_point();
        let width = (ui.available_width() * scale) as usize;
        if self.preview_width != width || self.preview_scale != scale {
            self.textures.clear();
            self.preview_width = width;
            self.preview_scale = scale;
        }
        if self.renderer.is_none() && self.font_error.is_none() {
            match speech::text::TextRenderer::bundled() {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(value) => self.font_error = Some(value.to_string()),
            }
        }
        if let Some(value) = &self.font_error {
            ui.label(format!(
                "Original-script preview unavailable: {value}. Copy or export the original text."
            ));
        }
        for (index, segment) in report
            .segments
            .iter()
            .enumerate()
            .skip(self.page * 10)
            .take(10)
        {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{:.2}–{:.2}s · {}",
                    segment.start_s,
                    segment.end_s,
                    if segment.timestamp_kind == "speech_interval" {
                        "speech interval"
                    } else {
                        "recognizer segment"
                    }
                ));
                if ui.small_button("Copy original").clicked() {
                    ctx.copy_text(segment.original.clone());
                }
            });
            if !self.textures.contains_key(&index)
                && let Some(renderer) = &mut self.renderer
            {
                match renderer.render(&segment.original, width, scale) {
                    Ok(rendered) => {
                        let clipped = rendered.clipped || rendered.missing_glyphs > 0;
                        self.textures.insert(
                            index,
                            (
                                ctx.load_texture(
                                    format!("speech-{index}"),
                                    rendered.image,
                                    egui::TextureOptions::LINEAR,
                                ),
                                clipped,
                            ),
                        );
                    }
                    Err(value) => {
                        *error = Some(value.to_string());
                    }
                }
            }
            if let Some((texture, clipped)) = self.textures.get(&index) {
                ui.image((texture.id(), texture.size_vec2() / scale));
                if *clipped {
                    ui.label("Preview is incomplete. Copy or export the full original text.");
                }
            }
            if let Some(english) = &segment.english {
                ui.label(RichText::new("English").strong());
                ui.label(english);
            } else {
                ui.label("English translation is not available for this interval.");
            }
        }
        ui.separator();
    }
}

pub(super) fn analyze(
    job: &Job,
    options: &Options,
    prepared: Option<Arc<Prepared>>,
    control: &Control,
    progress: &Progress<'_>,
) -> anyhow::Result<speech::Report> {
    let prepared = match prepared {
        Some(prepared) => prepared,
        None => Arc::new(
            speech::prepare(
                &job.media,
                &soundgeo::audio::ffmpeg_path(job),
                options,
                control,
                progress,
            )?
            .ok_or_else(|| anyhow::anyhow!("Speech analysis is disabled"))?,
        ),
    };
    speech::transcribe(&prepared, control, progress)
}

impl App {
    pub(super) fn speech_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.checkbox(
            &mut self.speech.options.enabled,
            "Transcribe speech and translate to English",
        );
        ui.label(RichText::new("Local, open-source models. First use downloads up to 7.4 GB; quality translation needs about 10 GB of free memory.").small().color(MUTED));
        if !self.speech.options.enabled {
            return;
        }
        egui::ComboBox::from_id_salt("speech-language")
            .selected_text(
                self.speech
                    .options
                    .language
                    .as_deref()
                    .unwrap_or("Detect automatically"),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.speech.options.language,
                    None,
                    "Detect automatically",
                );
                for (code, name) in speech::LANGUAGES {
                    let label = match code {
                        "hi" => "Hindi (Devanagari)",
                        "ur" => "Urdu (Arabic script)",
                        _ => name,
                    };
                    ui.selectable_value(
                        &mut self.speech.options.language,
                        Some(code.into()),
                        label,
                    );
                }
            });
        egui::ComboBox::from_id_salt("speech-quality")
            .selected_text(format!(
                "{:?} translation",
                self.speech.options.translation_quality
            ))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.speech.options.translation_quality,
                    TranslationQuality::Quality,
                    "Quality · 5.15 GB translator",
                );
                ui.selectable_value(
                    &mut self.speech.options.translation_quality,
                    TranslationQuality::Compact,
                    "Compact · 1.35 GB translator",
                );
            });
        ui.label(RichText::new("English recordings skip translation downloads. Downloaded models are verified and reused offline.").small().color(MUTED));
        if !self.job.media.is_file() {
            ui.label("Choose or download a recording to detect its language and prepare models.");
        }
        ui.collapsing("Speech settings", |ui| {
            ui.add(egui::Slider::new(&mut self.speech.options.threads, 1..=8).text("CPU workers"));
            ui.label(format!(
                "Model cache: {}",
                self.speech.options.cache.display()
            ));
            if ui.button("Choose speech cache…").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.speech.options.cache = path;
            }
            if ui.button("Retry speech preparation").clicked() {
                self.speech.invalidate();
            }
        });
        if ui
            .add_enabled(
                self.job.media.is_file(),
                egui::Button::new("Analyze speech only"),
            )
            .clicked()
        {
            let job = self.job.clone();
            let options = self.speech.options.clone();
            let prepared = self.speech.current_prepared(&job);
            self.speech.mark_requested(&job);
            self.worker(ctx, move |tx, control| {
                let result = analyze(&job, &options, prepared, &control, &|message, fraction| {
                    let _ = tx.send(Message::Progress(message.into(), fraction));
                });
                let _ = tx.send(match result {
                    Ok(report) => Message::SpeechReport(Box::new(report), true),
                    Err(error) => Message::Error(format!("{error:#}")),
                });
            });
        }
    }

    pub(super) fn schedule_speech(&mut self, ctx: &egui::Context) {
        if self.busy() || !self.speech.options.enabled || !self.job.media.is_file() {
            return;
        }
        let key = self.speech.key(&self.job);
        if self.speech.request_key.as_ref() == Some(&key) {
            return;
        }
        self.speech.invalidate();
        self.speech.request_key = Some(key);
        let job = self.job.clone();
        let options = self.speech.options.clone();
        self.worker(ctx, move |tx, control| {
            let result = speech::prepare(
                &job.media,
                &soundgeo::audio::ffmpeg_path(&job),
                &options,
                &control,
                &|message, fraction| {
                    let _ = tx.send(Message::Progress(message.into(), fraction));
                },
            );
            let _ = tx.send(match result {
                Ok(Some(prepared)) => Message::SpeechPrepared(Arc::new(prepared)),
                Ok(None) => Message::Error("Speech analysis is disabled".into()),
                Err(error) => Message::Error(format!("{error:#}")),
            });
        });
    }
}
