use super::{
    Detection, ModelFile, ModelUse, Options, Report, Segment, Source, Status, TranslationQuality,
    models,
    parsers::{self, Span},
    policy::{self, LanguageChoice},
    runtime::{self, RATE},
};
use crate::{Control, Progress, network};
use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

/// Prepared audio and verified weights. Keep this alive until analysis finishes.
/// None of these observations participate in geolocation acceptance.
pub struct Prepared {
    pub report: Report,
    pub checkpoint: PathBuf,
    directory: tempfile::TempDir,
    samples: Vec<i16>,
    chunks: Vec<Span>,
    recognizer: Option<(ModelFile, PathBuf)>,
    translator: Option<(ModelFile, PathBuf)>,
}

impl Prepared {
    pub fn ready(&self) -> bool {
        self.recognizer.is_some() && self.report.selected_language.is_some()
    }

    fn save(&self, report: &Report) -> Result<()> {
        report.validate()?;
        let parent = self
            .checkpoint
            .parent()
            .context("Missing speech checkpoint folder")?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut file, report)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(&self.checkpoint)
            .map_err(|error| error.error)?;
        Ok(())
    }
}

fn provenance(
    model: &ModelFile,
    stage: &str,
    family: &str,
    executable: &str,
    control: &Control,
) -> Result<ModelUse> {
    let (_, path) = runtime::command(family, executable)?;
    Ok(ModelUse {
        stage: stage.into(),
        id: model.id.clone(),
        revision: model.revision.clone(),
        sha256: model.sha256.clone(),
        runtime: format!("{family}/{executable}"),
        runtime_sha256: network::hash_file(&path, control)?,
    })
}

/// Decode and detect on checkbox enable, then fetch only the required weights.
/// With speech disabled this performs no I/O, subprocess execution, or download.
pub fn prepare(
    media: &Path,
    ffmpeg: &Path,
    options: &Options,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Option<Prepared>> {
    if !options.enabled {
        return Ok(None);
    }
    options.validate()?;
    control.check()?;
    std::fs::create_dir_all(&options.cache)?;
    let directory = tempfile::tempdir_in(&options.cache)?;
    progress(
        "Fingerprinting the original recording for speech evidence",
        0.,
    );
    let sha256 = network::hash_file(media, control)?;
    let size = media.metadata()?.len();
    let decoded = runtime::decode(media, directory.path(), ffmpeg, control, progress)?;
    ensure!(
        media.metadata()?.len() == size && network::hash_file(media, control)? == sha256,
        "The recording changed during speech preparation. Select it again."
    );
    let source = Source {
        path: media.to_path_buf(),
        sha256,
        size,
        duration_s: decoded.samples.len() as f64 / f64::from(RATE),
        audio_stream: 0,
        decoded_sample_rate: RATE,
        decoded_channels: 1,
        decoder_version: decoded.decoder_version,
    };
    let report = Report {
        schema: 1,
        application_version: env!("CARGO_PKG_VERSION").into(),
        status: Status::Partial,
        source,
        options: options.clone(),
        detected: vec![],
        selected_language: None,
        models: vec![],
        segments: vec![],
        warnings: vec![],
        created_utc: chrono::Utc::now().to_rfc3339(),
    };
    let reports = options.cache.join("reports");
    std::fs::create_dir_all(&reports)?;
    let checkpoint = reports.join(format!(
        "{}-{}.json",
        &report.source.sha256[..16],
        directory
            .path()
            .file_name()
            .context("Missing speech session identifier")?
            .to_string_lossy()
    ));
    let mut prepared = Prepared {
        report,
        checkpoint,
        directory,
        samples: decoded.samples,
        chunks: vec![],
        recognizer: None,
        translator: None,
    };
    let vad = models::registry("silero-vad")?;
    let vad_path = vad.ensure(&options.cache, control, progress)?;
    let output = prepared.directory.path().join("voice-activity.txt");
    let (mut command, _) = runtime::command("transcribe", "transcribe-cli")?;
    command
        .arg("-m")
        .arg(vad_path)
        .args(["--backend", "cpu", "--threads"])
        .arg(options.threads.to_string())
        .args(["--vad-threshold", "0.5", "-q", "-o"])
        .arg(&output)
        .arg(prepared.directory.path().join("speech-16k.wav"));
    runtime::capture(
        &mut command,
        prepared.directory.path(),
        "Finding speech intervals",
        control,
        progress,
        Duration::from_secs(1800),
        || Ok(()),
    )?;
    prepared.report.models.push(provenance(
        &vad,
        "voice_activity",
        "transcribe",
        "transcribe-cli",
        control,
    )?);
    let spans = parsers::vad(&runtime::read_text(&output)?, prepared.samples.len() as u64)?;
    prepared.chunks = parsers::chunks(&spans, &prepared.samples, 24 * RATE as usize)?;
    if prepared.chunks.is_empty() {
        prepared.report.status = Status::NoSpeech;
        prepared.report.warnings.push("No speech intervals were detected. No recognizer or translation weights were downloaded.".into());
        prepared.save(&prepared.report)?;
        progress("No speech detected", 1.);
        return Ok(Some(prepared));
    }
    if options.language.is_none() {
        let detector = models::registry("ecapa-lid")?;
        let path = detector.ensure(&options.cache, control, progress)?;
        let mut indices = vec![0, prepared.chunks.len() / 2, prepared.chunks.len() - 1];
        indices.dedup();
        for index in indices {
            let span = prepared.chunks[index];
            if span.end - span.start < u64::from(RATE) / 2 {
                continue;
            }
            let wav = prepared
                .directory
                .path()
                .join(format!("detect-{index}.wav"));
            runtime::write_wav(
                &wav,
                &prepared.samples[span.start as usize..span.end as usize],
                control,
            )?;
            let output = prepared
                .directory
                .path()
                .join(format!("detect-{index}.txt"));
            let (mut command, _) = runtime::command("transcribe", "transcribe-cli")?;
            command
                .arg("-m")
                .arg(&path)
                .args(["--backend", "cpu", "--threads"])
                .arg(options.threads.to_string())
                .args(["--top", "5", "-q", "-o"])
                .arg(&output)
                .arg(&wav);
            runtime::capture(
                &mut command,
                prepared.directory.path(),
                "Detecting the spoken language",
                control,
                progress,
                Duration::from_secs(180),
                || Ok(()),
            )?;
            let candidates = parsers::language_candidates(&runtime::read_text(&output)?)?;
            prepared.report.detected.push(Detection {
                start_s: span.start as f64 / f64::from(RATE),
                end_s: span.end as f64 / f64::from(RATE),
                language: candidates[0].0.clone(),
                score: candidates[0].1,
                alternatives: candidates[1..].to_vec(),
            });
        }
        prepared.report.models.push(provenance(
            &detector,
            "language_detection",
            "transcribe",
            "transcribe-cli",
            control,
        )?);
    }
    let language = match policy::choose_language(
        options.language.as_deref(),
        &prepared.report.detected,
    ) {
        LanguageChoice::Ready(code) => code,
        LanguageChoice::NeedsChoice(message) => {
            prepared.report.status = Status::NeedsLanguage;
            prepared.report.warnings.push(message.clone());
            prepared.save(&prepared.report)?;
            progress(&message, 1.);
            return Ok(Some(prepared));
        }
        LanguageChoice::Unsupported(code) => {
            prepared.report.status = Status::UnsupportedLanguage;
            let message = format!(
                "Detected language {code} is outside the ten supported languages. Choose a supported language only if detection was wrong."
            );
            prepared.report.warnings.push(message.clone());
            prepared.save(&prepared.report)?;
            progress(&message, 1.);
            return Ok(Some(prepared));
        }
    };
    prepared.report.selected_language = Some(language.clone());
    let recognizer = models::registry(match language.as_str() {
        "bn" => "bengali-asr",
        "ur" => "whisper-large",
        _ => "qwen-asr",
    })?;
    progress(
        &format!(
            "Preparing local speech models for {language}; downloads are reused on later runs"
        ),
        0.,
    );
    let recognizer_path = recognizer.ensure(&options.cache, control, progress)?;
    prepared.recognizer = Some((recognizer, recognizer_path));
    if language != "en" {
        let translator = models::registry(match options.translation_quality {
            TranslationQuality::Quality => "translation-quality",
            TranslationQuality::Compact => "translation-compact",
        })?;
        let path = translator.ensure(&options.cache, control, progress)?;
        prepared.translator = Some((translator, path));
    }
    prepared.report.status = Status::Ready;
    prepared.save(&prepared.report)?;
    progress(
        "Speech models ready. Analyze the recording to transcribe and translate.",
        1.,
    );
    Ok(Some(prepared))
}

/// Originals are checkpointed before translation. Failures return an explicitly
/// partial report so useful evidence survives a failed model or cancellation.
pub fn transcribe(
    prepared: &Prepared,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Report> {
    let mut report = prepared.report.clone();
    if !prepared.ready() {
        return Ok(report);
    }
    // Checkpoints written during inference must remain explicitly incomplete.
    report.status = Status::Partial;
    let result = run(prepared, &mut report, control, progress);
    match result {
        Ok(()) => report.status = Status::Complete,
        Err(error) => {
            report.status = Status::Partial;
            report.warnings.push(format!(
                "Speech analysis stopped: {error:#}. Completed original text was preserved."
            ));
        }
    }
    prepared.save(&report)?;
    Ok(report)
}

fn run(
    prepared: &Prepared,
    report: &mut Report,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<()> {
    control.check()?;
    ensure!(
        network::hash_file(&report.source.path, control)? == report.source.sha256,
        "The source recording changed after speech preparation"
    );
    let language = report
        .selected_language
        .clone()
        .context("Missing speech language")?;
    let (model, path) = prepared.recognizer.as_ref().context("Missing recognizer")?;
    let whisper = matches!(model.id.as_str(), "whisper-large" | "bengali-asr");
    let interval_only = model.id == "bengali-asr";
    let (family, executable) = if whisper {
        ("whisper", "whisper-cli")
    } else {
        ("transcribe", "transcribe-cli")
    };
    report.models.push(provenance(
        model,
        "transcription",
        family,
        executable,
        control,
    )?);
    let directory = tempfile::tempdir_in(prepared.directory.path())?;
    for (index, span) in prepared.chunks.iter().copied().enumerate() {
        control.check()?;
        let wav = directory.path().join(format!("chunk-{index}.wav"));
        runtime::write_wav(
            &wav,
            &prepared.samples[span.start as usize..span.end as usize],
            control,
        )?;
        let prefix = directory.path().join(format!("chunk-{index}"));
        let (mut command, _) = runtime::command(family, executable)?;
        command.arg("-m").arg(path).arg("-l").arg(&language);
        if whisper {
            command
                .arg("-f")
                .arg(&wav)
                .arg("-t")
                .arg(report.options.threads.to_string())
                .args(["--no-gpu", "-oj", "-of"])
                .arg(&prefix)
                .args(["-np", "-tp", "0", "-tpi", "0"]);
            if interval_only {
                command.arg("--no-timestamps");
            }
        } else {
            command
                .args(["--backend", "cpu", "--threads"])
                .arg(report.options.threads.to_string())
                .args(["--n-ctx", "4096", "--timestamps", "none", "-q", "-o"])
                .arg(prefix.with_extension("txt"))
                .arg(&wav);
        }
        let label = format!(
            "Transcribing speech interval {} of {}",
            index + 1,
            prepared.chunks.len()
        );
        runtime::capture(
            &mut command,
            directory.path(),
            &label,
            control,
            progress,
            Duration::from_secs(1800),
            || Ok(()),
        )?;
        let segments = if interval_only {
            parsers::whisper_interval(
                &runtime::read_text(&prefix.with_extension("json"))?,
                &language,
                span,
            )?
        } else if whisper {
            parsers::whisper(
                &runtime::read_text(&prefix.with_extension("json"))?,
                &language,
                span,
            )?
        } else {
            let original = runtime::read_text(&prefix.with_extension("txt"))?
                .trim()
                .to_owned();
            ensure!(
                !original.is_empty() && !original.contains('\0'),
                "Recognizer returned no usable original text"
            );
            vec![Segment {
                start_s: span.start as f64 / f64::from(RATE),
                end_s: span.end as f64 / f64::from(RATE),
                timestamp_kind: "speech_interval".into(),
                language: language.clone(),
                original,
                english: None,
                warnings: vec![
                    "Times identify a detected speech interval, not word-level alignment.".into(),
                ],
            }]
        };
        ensure!(
            !segments.is_empty(),
            "Recognizer returned no transcript for a detected speech interval"
        );
        report.segments.extend(segments);
        prepared.save(report)?;
    }
    if language == "en" {
        for segment in &mut report.segments {
            segment.english = Some(segment.original.clone());
        }
        return Ok(());
    }
    let (model, path) = prepared
        .translator
        .as_ref()
        .context("Missing translation model")?;
    report.models.push(provenance(
        model,
        "translation",
        "llama",
        "llama-completion",
        control,
    )?);
    for index in 0..report.segments.len() {
        control.check()?;
        let prompt = directory.path().join(format!("translate-{index}.txt"));
        std::fs::write(
            &prompt,
            format!(
                "Translate the following text into English. Note that you should only output the translated result without any additional explanation:\n{}",
                report.segments[index].original
            ),
        )?;
        let (mut command, _) = runtime::command("llama", "llama-completion")?;
        command
            .arg("-m")
            .arg(path)
            .arg("-f")
            .arg(&prompt)
            .args([
                "--jinja", "-cnv", "-st", "-ngl", "0", "-n", "1024", "-c", "4096", "-t",
            ])
            .arg(report.options.threads.to_string())
            .args([
                "--temp",
                "0",
                "--seed",
                "42",
                "--no-display-prompt",
                "--simple-io",
                "--no-warmup",
            ]);
        let output = runtime::capture(
            &mut command,
            directory.path(),
            &format!(
                "Translating interval {} of {} to English",
                index + 1,
                report.segments.len()
            ),
            control,
            progress,
            Duration::from_secs(1800),
            || Ok(()),
        )?;
        report.segments[index].english = Some(parsers::translation(&output)?);
        prepared.save(report)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_speech_does_not_touch_missing_media_tools_or_cache() {
        let directory = tempfile::tempdir().unwrap();
        let options = Options {
            cache: directory.path().join("must-not-exist"),
            ..Default::default()
        };
        let control = Control::default().with_offline(true);
        control.stop();
        assert!(
            prepare(
                Path::new("absent-media"),
                Path::new("absent-decoder"),
                &options,
                &control,
                &|_, _| panic!("Disabled speech must not start work")
            )
            .unwrap()
            .is_none()
        );
        assert!(!options.cache.exists());
    }
}
