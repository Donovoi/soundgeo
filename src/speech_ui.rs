use super::{App, MUTED, Message, ResponseHelp};
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
        let status = match report.status {
            speech::Status::Ready => {
                "Models ready: choose Analyze speech only or Analyze recording to transcribe"
            }
            speech::Status::Complete => {
                "Complete: review the original text and English translation"
            }
            speech::Status::Partial => {
                "Partial result: review the warnings and completed intervals"
            }
            speech::Status::NoSpeech => {
                "No speech detected: no transcript or translation was produced"
            }
            speech::Status::NeedsLanguage => {
                "Language choice needed: select the spoken language under Speech, then retry Analyze speech only. For music without speech, leave transcription unchecked"
            }
            speech::Status::UnsupportedLanguage => {
                "The detected language is not supported. Review the detection before choosing a different language"
            }
        };
        ui.add(egui::Label::new(status).wrap());
        ui.label(format!(
            "Language: {} · {} intervals",
            report
                .selected_language
                .as_deref()
                .unwrap_or("not established"),
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
            }).header_response.help("Expand to inspect the sampled speech intervals, suggested languages and model scores. These scores are not calibrated probabilities.");
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
                        ui.selectable_value(&mut format, candidate, format!("{candidate:?}")).help("Choose this export format: JSON keeps the full report, TXT gives readable text, and SRT/VTT give timed subtitles.");
                    }
                }).response.help("Choose JSON for the complete report, TXT for readable text, or SRT/VTT for timed subtitles.");
            if format != ExportFormat::Json {
                ui.checkbox(&mut self.export_english, "English translation").help("Export English text when checked, or the original-language transcript when cleared. JSON always preserves the full report.");
            }
            if ui.button("Save speech report…").help("Choose where to save the transcript in the selected format. JSON contains the full report; text and subtitle exports use the selected original or English text.").clicked() {
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
                .add_enabled(self.page > 0, egui::Button::new("Previous intervals")).help("Show the previous page of up to ten speech intervals. Unavailable on the first page.")
                .clicked()
            {
                self.page -= 1;
                self.textures.clear();
            }
            ui.label(format!("Page {} of {pages}", self.page + 1));
            if ui
                .add_enabled(self.page + 1 < pages, egui::Button::new("Next intervals")).help("Show the next page of up to ten speech intervals. Unavailable on the last page.")
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
                if ui.small_button("Copy original").help("Copy the full original-language transcript for this interval to the clipboard, including text that does not fit in the preview.").clicked() {
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
        ).help("Prepare local speech models for the selected recording and include transcription and English translation when you analyze it. First use downloads the required models.");
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
                ).help("Sample speech locally to suggest a language before preparing recognition and translation models. Uncertain detection may need a manual language choice.");
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
                    ).help("Use this spoken language for local transcription and English translation. Choose it when you know the recording language or need to correct automatic detection.");
                }
            }).response.help("Detect the spoken language locally or select it yourself when known. This selects speech models, separately from video OCR.");
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
                ).help("Use the larger local English-translation model for quality. The translator download is about 5.15 GB and needs about 10 GB of free memory.");
                ui.selectable_value(
                    &mut self.speech.options.translation_quality,
                    TranslationQuality::Compact,
                    "Compact · 1.35 GB translator",
                ).help("Use the smaller local English-translation model to reduce download and memory requirements, with a quality tradeoff.");
            }).response.help("Choose the local English-translation model size. English recordings skip the translation download.");
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
            if ui.button("Choose speech cache…").help("Choose where downloaded speech and translation models are verified and reused. Existing models stay in their current folder.").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.speech.options.cache = path;
            }
            if ui.button("Retry speech preparation").help("Retry language detection and preparation of the required local models for this recording. Choose a recording first; missing models require a connection.").clicked() {
                self.speech.invalidate();
            }
        }).header_response.help("Expand to choose speech CPU use and the model cache, or retry language and model preparation.");
        if ui
            .add_enabled(
                self.job.media.is_file(),
                egui::Button::new("Analyze speech only"),
            ).help("Transcribe the selected recording and translate non-English speech to English without searching flight locations. Choose or download a recording to enable this action.")
            .clicked()
        {
            let job = self.job.clone();
            let options = self.speech.options.clone();
            let prepared = self.speech.current_prepared(&job);
            self.speech.mark_requested(&job);
            self.speech_error = None;
            self.worker(ctx, "Transcribing speech and translating to English", move |tx, control| {
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
        self.speech_error = None;
        self.worker(
            ctx,
            "Detecting language and preparing speech models",
            move |tx, control| {
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
            },
        );
    }
}
