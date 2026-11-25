//! URL discovery and local intake. Page metadata is provenance, never search evidence.
use crate::{
    Control, Progress,
    audio::{bundled_tool, quiet_command},
    subprocess,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const MAX_ITEMS: usize = 20;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    #[default]
    Auto,
    YtDlp,
    GalleryDl,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    pub url: String,
    pub cache: PathBuf,
    pub engine: Engine,
    pub impersonate: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            url: String::new(),
            cache: default_cache(),
            engine: Engine::Auto,
            impersonate: false,
        }
    }
}
pub fn default_cache() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("media-cache"))
        .join("SoundGeo/media")
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub item: usize,
    pub title: String,
    pub id: String,
    pub url: String,
    pub duration_s: Option<f64>,
    pub headers: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listing {
    pub schema: u32,
    pub source_url: String,
    pub engine: Engine,
    pub engine_version: String,
    pub impersonate: bool,
    pub fetched_utc: String,
    pub candidates: Vec<Candidate>,
    pub notices: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    pub schema: u32,
    pub source: Listing,
    pub selected_item: usize,
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub duration_s: f64,
    pub has_audio: bool,
    pub has_video: bool,
    pub downloaded_utc: String,
}
pub(crate) fn validate_url(value: &str) -> Result<reqwest::Url> {
    ensure!(
        value.len() <= 16_384 && !value.chars().any(char::is_control),
        "URL is too long or contains control characters"
    );
    let url = reqwest::Url::parse(value)
        .context("Paste a complete http:// or https:// media/page URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "Only HTTP and HTTPS media URLs are supported"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "URLs containing credentials are not supported"
    );
    Ok(url)
}
pub(crate) fn tool(name: &str) -> PathBuf {
    bundled_tool(name).unwrap_or_else(|| name.into())
}
fn hash(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn root(options: &Options) -> Result<PathBuf> {
    validate_url(options.url.trim())?;
    Ok(options.cache.join(hash(&serde_json::to_vec(&(
        options.url.trim(),
        options.engine,
        options.impersonate,
    ))?)))
}
pub(crate) fn lock(root: &Path) -> Result<File> {
    fs::create_dir_all(root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("download.lock"))?;
    file.try_lock()
        .context("Another download is using this cache entry; wait for it to finish")?;
    Ok(file)
}
fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let (mut temp, destination) = crate::atomic_file::prepare(path)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), value)?;
    temp.as_file().sync_all()?;
    temp.persist(destination)?;
    Ok(())
}
fn attempt(root: &Path) -> Result<PathBuf> {
    Ok(tempfile::Builder::new()
        .prefix("attempt-")
        .tempdir_in(root)?
        .keep())
}
fn execute(
    command: &mut Command,
    dir: &Path,
    name: &str,
    control: &Control,
    seconds: u64,
    monitor: impl Fn() -> Result<()>,
) -> Result<PathBuf> {
    let output = dir.join(format!("{name}.out"));
    let error = dir.join(format!("{name}.err"));
    let status = subprocess::run(
        command,
        &output,
        &error,
        control,
        Duration::from_secs(seconds),
        monitor,
    )?;
    ensure!(
        status.success(),
        "Media helper failed ({status}): {}. Logs: {}",
        subprocess::tail(&error),
        dir.display()
    );
    Ok(output)
}
fn version(name: &str, dir: &Path, control: &Control) -> Result<String> {
    let mut command = quiet_command(tool(name));
    command.arg("--version");
    let path = execute(
        &mut command,
        dir,
        &format!("{name}-version"),
        control,
        30,
        || Ok(()),
    )?;
    Ok(fs::read_to_string(path)?
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned())
}
fn yt_command(impersonate: bool) -> Command {
    let mut command = quiet_command(tool("yt-dlp"));
    command.args([
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-update",
        "--no-remote-components",
        "--no-cache-dir",
        "--no-colors",
        "--socket-timeout",
        "30",
        "--retries",
        "3",
        "--fragment-retries",
        "3",
        "--retry-sleep",
        "http:exp=2:30",
        "--no-playlist",
    ]);
    if impersonate {
        command.args(["--impersonate", "chrome"]);
    }
    if let Some(deno) = bundled_tool("deno") {
        command
            .arg("--js-runtimes")
            .arg(format!("deno:{}", deno.display()));
    }
    command
}
fn yt_candidates(value: &Value, source: &str) -> Result<Vec<Candidate>> {
    let entries = value["entries"]
        .as_array()
        .map(|a| a.iter().collect::<Vec<_>>())
        .unwrap_or_else(|| vec![value]);
    entries
        .into_iter()
        .take(MAX_ITEMS)
        .enumerate()
        .filter(|(_, v)| !v.is_null())
        .map(|(index, v)| {
            let url = v["webpage_url"]
                .as_str()
                .filter(|u| validate_url(u).is_ok())
                .unwrap_or(source);
            Ok(Candidate {
                item: index + 1,
                title: v["title"]
                    .as_str()
                    .unwrap_or("Untitled media")
                    .chars()
                    .take(300)
                    .collect(),
                id: v["id"].as_str().unwrap_or_default().into(),
                url: url.into(),
                duration_s: v["duration"].as_f64(),
                headers: BTreeMap::new(),
            })
        })
        .collect()
}
fn gallery_candidates(value: &Value) -> Result<Vec<Candidate>> {
    let mut items = vec![];
    for record in value
        .as_array()
        .context("gallery-dl returned invalid discovery JSON")?
    {
        if record[0].as_u64() != Some(3) {
            continue;
        }
        let Some(raw) = record[1].as_str() else {
            continue;
        };
        let url = raw.strip_prefix("ytdl:").unwrap_or(raw);
        if validate_url(url).is_err() {
            continue;
        }
        let meta = &record[2];
        let ext = meta["extension"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        // Known still images are omitted. Unknown extensions are checked by FFprobe after download.
        if [
            "jpg", "jpeg", "png", "gif", "webp", "avif", "heic", "svg", "zip",
        ]
        .contains(&ext.as_str())
        {
            continue;
        }
        let mut headers = BTreeMap::new();
        for key in ["http_headers", "_http_headers", "headers"] {
            if let Some(map) = meta[key].as_object() {
                for (key, value) in map {
                    if let Some(value) = value.as_str() {
                        headers.insert(key.clone(), value.into());
                    }
                }
            }
        }
        items.push(Candidate {
            item: items.len() + 1,
            title: meta["title"]
                .as_str()
                .or_else(|| meta["filename"].as_str())
                .unwrap_or("Media from gallery")
                .chars()
                .take(300)
                .collect(),
            id: meta["id"].to_string(),
            url: url.into(),
            duration_s: meta["duration"].as_f64(),
            headers,
        });
        if items.len() == MAX_ITEMS {
            break;
        }
    }
    Ok(items)
}
pub fn discover(options: &Options, control: &Control, progress: &Progress<'_>) -> Result<Listing> {
    let root = root(options)?;
    let _lock = lock(&root)?;
    let saved = root.join("listing.json");
    if control.offline {
        control.check()?;
        let listing: Listing = serde_json::from_reader(
            File::open(&saved).context("Offline mode: this URL has no saved media listing")?,
        )?;
        ensure!(
            listing.schema == 1 && listing.source_url == options.url.trim(),
            "Cached URL listing does not match"
        );
        return Ok(listing);
    }
    control.require_network("media URL discovery")?;
    let dir = attempt(&root)?;
    let mut errors = vec![];
    let tries = match options.engine {
        Engine::Auto => vec![
            (Engine::YtDlp, options.impersonate),
            (Engine::YtDlp, true),
            (Engine::GalleryDl, options.impersonate),
        ],
        engine => vec![(engine, options.impersonate)],
    };
    let mut previous = None;
    for (index, (engine, impersonate)) in tries.into_iter().enumerate() {
        if previous == Some((engine, impersonate)) {
            continue;
        }
        previous = Some((engine, impersonate));
        control.check()?;
        progress(
            &format!(
                "Finding media with {engine:?}{}",
                if impersonate {
                    " (browser impersonation)"
                } else {
                    ""
                }
            ),
            0.05,
        );
        let found = (|| -> Result<Listing> {
            let name = if engine == Engine::GalleryDl {
                "gallery-dl"
            } else {
                "yt-dlp"
            };
            let engine_version = version(name, &dir, control)?;
            let mut command = if engine == Engine::GalleryDl {
                let mut command = quiet_command(tool(name));
                command.args([
                    "--config-ignore",
                    "--no-input",
                    "--no-postprocessors",
                    "--range",
                    "1-100",
                    "--post-range",
                    "1-20",
                    "--child-range",
                    "1-20",
                    "--retries",
                    "3",
                    "--http-timeout",
                    "30",
                    "--dump-json",
                ]);
                command
            } else {
                let mut command = yt_command(impersonate);
                command.args([
                    "--dump-single-json",
                    "--flat-playlist",
                    "--playlist-end",
                    "20",
                    "--skip-download",
                ]);
                command
            };
            command.arg("--").arg(options.url.trim());
            let output = execute(
                &mut command,
                &dir,
                &format!("discover-{index}"),
                control,
                180,
                || Ok(()),
            )?;
            let value: Value = serde_json::from_reader(File::open(output)?)?;
            let candidates = if engine == Engine::GalleryDl {
                gallery_candidates(&value)?
            } else {
                yt_candidates(&value, options.url.trim())?
            };
            ensure!(
                !candidates.is_empty(),
                "No video/audio links found among the inspected page items"
            );
            Ok(Listing { schema: 1, source_url: options.url.trim().into(), engine, engine_version, impersonate, fetched_utc: chrono::Utc::now().to_rfc3339(), candidates, notices: vec!["Shows at most 20 media candidates; narrow large galleries to a specific post. Page dates and locations are not used for geolocation.".into()] })
        })();
        match found {
            Ok(mut listing) => {
                listing.notices.extend(errors);
                save(&saved, &listing)?;
                progress("Choose the recording to download", 1.);
                return Ok(listing);
            }
            Err(error) => errors.push(format!("{engine:?}: {error:#}")),
        }
    }
    control.check()?;
    bail!(
        "Could not find downloadable media. The page may need authentication, be unsupported, or contain protected media. {}",
        errors.join("\n")
    )
}
pub fn download(
    options: &Options,
    listing: &Listing,
    item: usize,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Download> {
    ensure!(
        listing.schema == 1 && listing.source_url == options.url.trim(),
        "Media listing belongs to a different URL"
    );
    let candidate = listing
        .candidates
        .iter()
        .find(|c| c.item == item)
        .context("Select a listed media item")?;
    validate_url(&candidate.url)?;
    let root = root(options)?;
    let _lock = lock(&root)?;
    let key = hash(&serde_json::to_vec(&(
        listing.engine,
        listing.impersonate,
        candidate,
    ))?);
    let dir = root.join(key);
    fs::create_dir_all(&dir)?;
    let dir = dir.canonicalize()?;
    let record = dir.join("download.json");
    if let Ok(file) = File::open(&record)
        && let Ok(saved) = serde_json::from_reader::<_, Download>(file)
        && saved.schema == 1
        && saved.source.source_url == listing.source_url
        && saved.selected_item == item
        && saved.path.parent() == Some(dir.as_path())
        && saved.path.metadata().is_ok_and(|m| m.len() == saved.size)
        && crate::network::hash_file(&saved.path, control)? == saved.sha256
    {
        progress("Using verified downloaded recording", 1.);
        return Ok(saved);
    }
    control.require_network("downloaded recording")?;
    // Keep rejected/partial attempts visible. A tool cannot turn a corrupted completed
    // cache into a valid hit merely by reporting that the file already exists.
    if record.exists() {
        bail!(
            "Downloaded recording failed verification. Preserve this cache for inspection and choose a new media cache folder: {}",
            dir.display()
        );
    }
    let logs = attempt(&dir)?;
    save(&logs.join("source.json"), listing)?;
    let mut command = yt_command(listing.impersonate);
    command
        .args([
            "--quiet",
            "--no-progress",
            "--no-simulate",
            "--no-overwrites",
            "--continue",
            "--no-mtime",
            "--max-filesize",
            "8G",
            "--playlist-items",
        ])
        .arg(
            if listing.engine == Engine::GalleryDl {
                1
            } else {
                item
            }
            .to_string(),
        )
        .args([
            "--format",
            "bv*+ba/b",
            "--merge-output-format",
            "mkv",
            "--print",
            "after_move:%(filepath)j",
        ])
        .arg("--output")
        .arg(format!(
            "{}/media.%(ext)s",
            dir.to_string_lossy().replace('%', "%%")
        ));
    if let Some(ffmpeg) = bundled_tool("ffmpeg") {
        command.arg("--ffmpeg-location").arg(ffmpeg);
    }
    crate::network::configure_media_downloader(&mut command, listing.impersonate);
    if listing.engine == Engine::GalleryDl {
        command
            .arg("--add-header")
            .arg(format!("Referer:{}", listing.source_url));
        for (key, value) in &candidate.headers {
            ensure!(
                !key.chars().any(char::is_control)
                    && !key.contains(':')
                    && !value.chars().any(char::is_control),
                "Invalid media HTTP header"
            );
            command.arg("--add-header").arg(format!("{key}:{value}"));
        }
        command.arg("--").arg(&candidate.url);
    } else {
        command.arg("--").arg(&listing.source_url);
    }
    progress(
        "Downloading selected recording; partial transfers can resume",
        0.1,
    );
    let output = execute(&mut command, &logs, "download", control, 7200, || {
        let size: u64 = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum();
        ensure!(
            size <= 16 * 1024 * 1024 * 1024,
            "Media download exceeded 16 GiB including merge files"
        );
        Ok(())
    })?;
    let paths: Vec<String> = fs::read_to_string(output)?
        .lines()
        .filter_map(|line| serde_json::from_str::<String>(line).ok())
        .collect();
    ensure!(
        paths.len() == 1,
        "Expected one completed recording; the page returned {}. Use a specific video URL",
        paths.len()
    );
    let path = PathBuf::from(&paths[0]).canonicalize()?;
    ensure!(
        path.parent() == Some(dir.as_path()),
        "Downloader returned a file outside the media cache"
    );
    progress("Verifying media streams and file hash", 0.9);
    let mut probe = quiet_command(tool("ffprobe"));
    probe
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_entries",
            "format=duration:stream=codec_type,duration",
            "-of",
            "json",
            "-i",
        ])
        .arg(&path);
    let output = execute(&mut probe, &logs, "probe", control, 60, || Ok(()))?;
    let value: Value = serde_json::from_reader(File::open(output)?)?;
    let streams = value["streams"]
        .as_array()
        .context("Downloaded file has no media streams")?;
    let has_audio = streams.iter().any(|s| s["codec_type"] == "audio");
    let has_video = streams.iter().any(|s| s["codec_type"] == "video");
    let duration_s = value["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            streams
                .iter()
                .filter_map(|s| s["duration"].as_str()?.parse::<f64>().ok())
                .max_by(f64::total_cmp)
        })
        .unwrap_or(0.);
    ensure!(
        (has_audio || has_video) && duration_s.is_finite() && duration_s > 0.,
        "Downloaded file is not a playable audio/video recording"
    );
    let saved = Download {
        schema: 1,
        source: listing.clone(),
        selected_item: item,
        sha256: crate::network::hash_file(&path, control)?,
        size: path.metadata()?.len(),
        path,
        duration_s,
        has_audio,
        has_video,
        downloaded_utc: chrono::Utc::now().to_rfc3339(),
    };
    save(&record, &saved)?;
    progress("Recording ready", 1.);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_local_protocols_and_argument_injection() {
        for url in [
            "file:///tmp/a",
            "ftp://example.com/a",
            "--exec=bad",
            "https://example.com/\n--exec",
            "https://name:password@example.com/a",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
        assert!(validate_url("https://example.com/watch?v=a&list=b").is_ok());
    }
    #[test]
    fn gallery_filters_stills_and_preserves_headers_and_video_url() {
        let items = gallery_candidates(&json!([[2, {}], [3, "https://example.com/a.jpg", {"extension":"jpg"}], [3,"https://example.com/a.mp4", {"extension":"mp4", "headers":{"Referer":"https://example.com/"}}]])).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item, 1);
        assert_eq!(items[0].headers["Referer"], "https://example.com/");
    }
    #[test]
    fn multi_video_listing_preserves_playlist_indices() {
        let items = yt_candidates(
            &json!({"entries":[null,{"id":"two","title":"Second"}]}),
            "https://example.com/post",
        )
        .unwrap();
        assert_eq!(items[0].item, 2);
    }
    #[test]
    fn offline_discovery_never_starts_tools_and_lock_releases() {
        let dir = tempfile::tempdir().unwrap();
        let options = Options {
            url: "https://example.com/video".into(),
            cache: dir.path().into(),
            ..Options::default()
        };
        assert!(
            discover(&options, &Control::default().with_offline(true), &|_, _| {})
                .unwrap_err()
                .to_string()
                .contains("Offline mode")
        );
        let root = root(&options).unwrap();
        let held = lock(&root).unwrap();
        assert!(lock(&root).is_err());
        drop(held);
        assert!(lock(&root).is_ok());
    }
}
