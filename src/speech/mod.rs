//! Local speech evidence. This module does not supply geolocation observations.
mod models;
mod parsers;
mod pipeline;
mod policy;
mod report;
mod runtime;
pub mod text;

pub use models::ModelFile;
pub use pipeline::{Prepared, prepare, transcribe};
pub use report::{
    Detection, ExportFormat, ModelUse, Report, Segment, Source, Status, write_report,
};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The initial processing scope; detection also considers languages outside it.
pub const LANGUAGES: [(&str, &str); 10] = [
    ("en", "English"),
    ("zh", "Mandarin Chinese"),
    ("hi", "Hindi"),
    ("es", "Spanish"),
    ("ar", "Arabic"),
    ("fr", "French"),
    ("bn", "Bengali"),
    ("pt", "Portuguese"),
    ("id", "Indonesian"),
    ("ur", "Urdu"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// No model, helper, or download is started while this is false.
    pub enabled: bool,
    /// None detects language from audio. A manual choice is recorded as such.
    pub language: Option<String>,
    pub cache: PathBuf,
    pub threads: usize,
    pub translation_quality: TranslationQuality,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TranslationQuality {
    #[default]
    Quality,
    Compact,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: false,
            language: None,
            cache: default_cache(),
            threads: 2,
            translation_quality: TranslationQuality::default(),
        }
    }
}

impl Options {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=32).contains(&self.threads),
            "Speech workers must be 1–32"
        );
        if let Some(language) = &self.language {
            anyhow::ensure!(
                LANGUAGES.iter().any(|(code, _)| code == language),
                "Choose one of the ten supported speech languages, or automatic detection"
            );
        }
        anyhow::ensure!(
            !self.cache.as_os_str().is_empty(),
            "Choose a speech cache folder"
        );
        Ok(())
    }
}

pub fn default_cache() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("speech-cache"));
    base.join("SoundGeo/speech")
}
