use crate::{Control, Progress, network};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
};

pub(crate) fn registry(id: &str) -> Result<ModelFile> {
    let models: Vec<ModelFile> = serde_json::from_str(include_str!("models.json"))?;
    models
        .into_iter()
        .find(|model| model.id == id)
        .context("Unknown speech model")
}

/// Immutable, published model metadata. Weights are verified before inference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelFile {
    pub id: String,
    pub name: String,
    pub url: String,
    pub revision: String,
    pub sha256: String,
    pub size: u64,
    pub filename: String,
    pub license: String,
}

impl ModelFile {
    pub fn validate(&self) -> Result<()> {
        let hex = |value: &str, length| {
            value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
        };
        ensure!(
            hex(&self.sha256, 64) && hex(&self.revision, 40),
            "Invalid speech model hash or revision"
        );
        ensure!(
            self.size > 0 && self.size <= 16 * 1024 * 1024 * 1024,
            "Invalid speech model size"
        );
        ensure!(
            !self.filename.is_empty()
                && self
                    .filename
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                && self.filename != "."
                && self.filename != "..",
            "Invalid speech model filename"
        );
        let url = reqwest::Url::parse(&self.url)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str() == Some("huggingface.co")
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path().contains(&format!("/resolve/{}/", self.revision))
                && url.path().ends_with(&format!("/{}", self.filename)),
            "Speech models require a pinned HTTPS publisher URL"
        );
        Ok(())
    }

    pub fn cache_path(&self, cache: &Path) -> Result<PathBuf> {
        self.validate()?;
        Ok(cache.join("models").join(&self.sha256).join(&self.filename))
    }

    /// A cache stamp is not sufficient: always hash the exact file being loaded.
    pub fn ensure(
        &self,
        cache: &Path,
        control: &Control,
        progress: &Progress<'_>,
    ) -> Result<PathBuf> {
        control.check()?;
        // Small detection weights ship with the application. Large recognizer
        // and translation weights are only fetched into the user's cache.
        if matches!(self.id.as_str(), "silero-vad" | "ecapa-lid")
            && let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let bundled = parent.join("speech-models").join(&self.filename);
            if bundled.is_file() {
                ensure!(
                    bundled.metadata()?.len() == self.size
                        && network::hash_file(&bundled, control)? == self.sha256,
                    "Bundled speech detector is damaged; download SoundGeo again"
                );
                return Ok(bundled);
            }
        }
        let path = self.cache_path(cache)?;
        let parent = path.parent().context("Missing speech model cache parent")?;
        std::fs::create_dir_all(parent)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(parent.join("download.lock"))?;
        lock.try_lock().context(
            "This speech model is in use by another download. Wait for it to finish, then retry.",
        )?;
        progress(
            &format!("Checking {} ({:.0} MB)", self.name, self.size as f64 / 1e6),
            0.,
        );
        network::fetch_file(
            &self.url,
            &path,
            self.size,
            Some(&format!("sha256:{}", self.sha256)),
            control,
            &|message, fraction| progress(&format!("{}: {message}", self.name), fraction),
        )?;
        control.check()?;
        // fetch_file hashes cached files and completed downloads against the
        // explicit publisher digest. Avoid hashing multi-GB weights twice.
        progress(&format!("{} verified and ready", self.name), 1.);
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn model() -> ModelFile {
        let revision = "a".repeat(40);
        ModelFile {
            id: "fixture".into(),
            name: "Fixture".into(),
            url: format!("https://huggingface.co/example/model/resolve/{revision}/weights.bin"),
            revision,
            sha256: format!("{:x}", Sha256::digest(b"valid")),
            size: 5,
            filename: "weights.bin".into(),
            license: "MIT".into(),
        }
    }

    #[test]
    fn offline_models_recheck_bytes_and_reject_corrupt_cache() {
        let cache = tempfile::tempdir().unwrap();
        let model = model();
        let path = model.cache_path(cache.path()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"valid").unwrap();
        let offline = Control::default().with_offline(true);
        assert_eq!(
            model.ensure(cache.path(), &offline, &|_, _| {}).unwrap(),
            path
        );
        std::fs::write(&path, b"other").unwrap();
        assert!(model.ensure(cache.path(), &offline, &|_, _| {}).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"other");
    }

    #[test]
    fn model_manifest_rejects_mutable_urls_and_escaping_filenames() {
        let mut model = model();
        model.validate().unwrap();
        model.url = model.url.replace(&model.revision, "main");
        assert!(model.validate().is_err());
        model.filename = "../weights.bin".into();
        assert!(model.cache_path(Path::new("cache")).is_err());
    }

    #[test]
    fn a_second_model_download_cannot_reuse_a_locked_destination() {
        let cache = tempfile::tempdir().unwrap();
        let model = model();
        let path = model.cache_path(cache.path()).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"valid").unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.parent().unwrap().join("download.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(
            model
                .ensure(
                    cache.path(),
                    &Control::default().with_offline(true),
                    &|_, _| {}
                )
                .is_err()
        );
    }
}
