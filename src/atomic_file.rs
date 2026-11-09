use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Prepare an atomic replacement in the target's existing parent directory.
pub(crate) fn prepare(target: &Path) -> Result<(tempfile::NamedTempFile, PathBuf)> {
    let name = target.file_name().context("Output file has no name")?;
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .with_context(|| format!("Could not resolve output folder for {}", target.display()))?;
    // On Windows, canonical paths include the extended-length prefix needed by
    // tempfile's native attribute and rename calls for both source and target.
    let file = tempfile::NamedTempFile::new_in(&parent)?;
    Ok((file, parent.join(name)))
}
