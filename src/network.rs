use crate::{Control, Progress};
use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

pub(crate) fn hash_file(path: &Path, control: &Control) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 262144];
    loop {
        control.check()?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
pub(crate) async fn cancelable<F: Future>(future: F, control: &Control) -> Result<F::Output> {
    control.require_network("network request")?;
    let mut pending = std::pin::pin!(future);
    loop {
        control.check()?;
        if let Ok(result) = tokio::time::timeout(Duration::from_millis(100), &mut pending).await {
            return Ok(result);
        }
    }
}
pub fn fetch_file(
    url: &str,
    path: &Path,
    size: u64,
    digest: Option<&str>,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<()> {
    if size >= 8 * 1024 * 1024
        && let Some(tool) = aria2()
    {
        return fetch_aria(
            &[Transfer {
                url,
                path,
                size,
                digest,
            }],
            &tool,
            control,
            progress,
        );
    }
    fetch_native(url, path, size, digest, control, progress)
}
fn fetch_native(
    url: &str,
    path: &Path,
    size: u64,
    digest: Option<&str>,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<()> {
    control.check()?;
    let expected = expected_digest(digest)?;
    let stamp = path.with_extension(format!(
        "{}.sha256",
        path.extension().unwrap_or_default().to_string_lossy()
    ));
    let local = std::fs::read_to_string(&stamp).ok();
    if path.metadata().is_ok_and(|m| m.len() == size)
        && let Some(hash) = expected.as_ref().or(local.as_ref())
        && hash_file(path, control)? == hash.trim()
    {
        progress("Using verified cached archive part", 1.);
        return Ok(());
    }
    control.require_network(&path.display().to_string())?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let (mut temp, destination) = crate::atomic_file::prepare(path)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let hash = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(7200))
            .user_agent("soundgeo/0.2")
            .build()?;
        let mut response = cancelable(client.get(url).send(), control)
            .await??
            .error_for_status()?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        while let Some(bytes) = cancelable(response.chunk(), control).await?? {
            received += bytes.len() as u64;
            if received > size {
                bail!("Archive part is larger than its published size");
            }
            temp.write_all(&bytes)?;
            hasher.update(&bytes);
            progress(
                &format!(
                    "Downloading {:.2} / {:.2} GB",
                    received as f64 / 1e9,
                    size as f64 / 1e9
                ),
                received as f32 / size.max(1) as f32,
            );
        }
        if received != size {
            bail!("Incomplete archive part: received {received}, expected {size} bytes");
        }
        let hash = format!("{:x}", hasher.finalize());
        if expected.as_ref().is_some_and(|e| e != &hash) {
            bail!("Archive SHA-256 mismatch; downloaded part was discarded");
        }
        Ok::<_, anyhow::Error>(hash)
    })?;
    control.check()?;
    temp.as_file().sync_all()?;
    temp.persist(destination)?;
    std::fs::write(stamp, hash)?;
    Ok(())
}

pub(crate) fn aria2() -> Option<std::path::PathBuf> {
    crate::audio::bundled_tool("aria2c").or_else(|| {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|p| {
                    p.join(if cfg!(windows) {
                        "aria2c.exe"
                    } else {
                        "aria2c"
                    })
                })
                .find(|p| p.is_file())
        })
    })
}
pub(crate) fn configure_media_downloader(command: &mut std::process::Command, impersonate: bool) {
    command.args(["--concurrent-fragments", "8"]);
    // curl_cffi must own the media request when impersonating; aria2 cannot
    // reproduce its TLS fingerprint. Native HLS keeps per-fragment auth intact.
    if !impersonate && let Some(path) = aria2() {
        // yt-dlp uses the protocol key "http" for HTTP and HTTPS. An unknown
        // "https" key is parsed as part of the executable path and silently
        // falls back to its single-connection native downloader.
        command.arg("--downloader").arg(format!("http:{}", path.display()))
            .args(["--downloader", "dash,m3u8:native", "--downloader-args", "aria2c:--no-conf --split=16 --max-connection-per-server=16 --min-split-size=8M --max-tries=3 --retry-wait=5 --file-allocation=none --max-overall-download-limit=0"]);
    }
}
pub(crate) struct Transfer<'a> {
    pub url: &'a str,
    pub path: &'a Path,
    pub size: u64,
    pub digest: Option<&'a str>,
}
pub(crate) fn fetch_files(
    files: &[Transfer<'_>],
    control: &Control,
    progress: &Progress<'_>,
) -> Result<()> {
    if let Some(tool) = aria2() {
        return fetch_aria(files, &tool, control, progress);
    }
    use rayon::prelude::*;
    let completed = std::sync::atomic::AtomicUsize::new(0);
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()?
        .install(|| {
            files.par_iter().try_for_each(|file| {
                fetch_native(
                    file.url,
                    file.path,
                    file.size,
                    file.digest,
                    control,
                    &|message, _| {
                        progress(
                            message,
                            completed.load(std::sync::atomic::Ordering::Relaxed) as f32
                                / files.len().max(1) as f32,
                        )
                    },
                )?;
                completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok::<_, anyhow::Error>(())
            })
        })
}
fn stamp(path: &Path) -> std::path::PathBuf {
    path.with_extension(format!(
        "{}.sha256",
        path.extension().unwrap_or_default().to_string_lossy()
    ))
}
fn expected_digest(digest: Option<&str>) -> Result<Option<String>> {
    let expected = digest.map(|d| d.strip_prefix("sha256:").unwrap_or("").to_ascii_lowercase());
    if expected
        .as_ref()
        .is_some_and(|d| d.len() != 64 || !d.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        bail!("Invalid archive SHA-256 digest");
    }
    Ok(expected)
}
fn fetch_aria(
    files: &[Transfer<'_>],
    tool: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<()> {
    use anyhow::{Context, ensure};
    let mut pending = vec![];
    let mut locks = vec![];
    let mut input = String::new();
    for file in files {
        control.check()?;
        crate::media_url::validate_url(file.url)?;
        let expected = expected_digest(file.digest)?;
        let saved = std::fs::read_to_string(stamp(file.path)).ok();
        if file.path.metadata().is_ok_and(|m| m.len() == file.size)
            && let Some(hash) = expected.as_ref().or(saved.as_ref())
            && hash_file(file.path, control)? == hash.trim()
        {
            continue;
        }
        control.require_network("archive transfer")?;
        let key = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(file.url, file.size, &expected))?)
        );
        let parent = file
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let cache = parent.join(".soundgeo-downloads").join(key);
        locks.push(crate::media_url::lock(&cache)?);
        let cache = cache.canonicalize()?;
        ensure!(
            !cache.to_string_lossy().chars().any(char::is_control),
            "Download folder contains control characters"
        );
        input.push_str(&format!(
            "{}\n dir={}\n out=payload\n",
            file.url,
            cache.display()
        ));
        if let Some(hash) = &expected {
            input.push_str(&format!(" checksum=sha-256={hash}\n"));
        }
        pending.push((file, cache, expected));
    }
    if pending.is_empty() {
        progress("Using verified cached archive parts", 1.);
        return Ok(());
    }
    let attempt = tempfile::Builder::new()
        .prefix("attempt-")
        .tempdir_in(&pending[0].1)?
        .keep();
    let input_path = attempt.join("transfers.txt");
    std::fs::write(&input_path, input)?;
    let mut command = crate::audio::quiet_command(tool);
    command
        .args([
            "--no-conf",
            "--continue=true",
            "--auto-file-renaming=false",
            "--allow-overwrite=true",
            "--check-integrity=true",
            "--file-allocation=none",
            "--max-concurrent-downloads=16",
            "--optimize-concurrent-downloads=true",
            "--min-split-size=8M",
            "--max-overall-download-limit=0",
            "--auto-save-interval=5",
            "--connect-timeout=20",
            "--timeout=30",
            "--max-tries=3",
            "--retry-wait=5",
            "--summary-interval=5",
            "--console-log-level=warn",
            "--enable-color=false",
            "--download-result=hide",
            "--input-file",
        ])
        .arg(&input_path);
    let connections = if pending.len() == 1 { 16 } else { 4 };
    command.args([
        format!("--split={connections}"),
        format!("--max-connection-per-server={connections}"),
    ]);
    let out = attempt.join("aria2.out");
    let err = attempt.join("aria2.err");
    let total: u64 = pending.iter().map(|(f, _, _)| f.size).sum();
    let result = crate::subprocess::run(
        &mut command,
        &out,
        &err,
        control,
        Duration::from_secs(7200),
        || {
            for (file, cache, _) in &pending {
                let length = cache
                    .join("payload")
                    .metadata()
                    .map(|m| m.len())
                    .unwrap_or(0);
                ensure!(
                    length <= file.size,
                    "Archive part exceeds its published size"
                );
            }
            // Sparse ranged files can have their final length immediately.
            // Show aria2's received-byte status, never a percentage from length.
            let tail = crate::subprocess::tail(&out);
            let transfer = tail
                .lines()
                .rev()
                .find(|line| line.starts_with("[#") && line.contains("DL:"));
            let message = transfer.map_or_else(
                || format!("Downloading {:.2} GB across {} files; up to {connections} connections per file", total as f64 / 1e9, pending.len()),
                |line| format!("Downloading: {line}"),
            );
            progress(&message, 0.);
            Ok(())
        },
    );
    control.check()?;
    let mut failures = vec![];
    for (file, cache, expected) in pending {
        let path = cache.join("payload");
        if cache.join("payload.aria2").exists()
            || !path.metadata().is_ok_and(|m| m.len() == file.size)
        {
            failures.push(file.path.display().to_string());
            continue;
        }
        let hash = hash_file(&path, control)?;
        if expected.as_ref().is_some_and(|value| value != &hash) {
            // Preserve rejected bytes but ensure the next attempt starts fresh.
            let rejected = tempfile::Builder::new()
                .prefix("rejected-")
                .tempdir_in(&cache)?
                .keep();
            std::fs::rename(&path, rejected.join("payload"))?;
            failures.push(format!("{} (SHA-256 mismatch)", file.path.display()));
            continue;
        }
        let (mut temp, destination) = crate::atomic_file::prepare(file.path)?;
        std::io::copy(&mut std::fs::File::open(&path)?, temp.as_file_mut())?;
        temp.as_file().sync_all()?;
        temp.persist(destination)?;
        std::fs::write(stamp(file.path), hash)?;
        std::fs::remove_file(path)?;
    }
    if !failures.is_empty() {
        bail!(
            "Incomplete archive transfers: {}. Retry to resume. {} {}. Logs: {}",
            failures.join(", "),
            crate::subprocess::tail(&out),
            crate::subprocess::tail(&err),
            attempt.display()
        );
    }
    ensure!(
        result.context("aria2 transfer stopped")?.success(),
        "aria2 reported failure; verified completed parts are saved. Logs: {}",
        attempt.display()
    );
    progress("All archive parts verified", 1.);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aria2_batch_verifies_all_files_and_rejects_corrupted_offline_cache() {
        let tool = aria2().expect("Install aria2 for downloader qualification tests");
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.part");
        let second = dir.path().join("second.part");
        let body = b"verified archive fixture";
        let digest = format!("sha256:{:x}", Sha256::digest(body));
        let (url1, thread1) = server(body, body.len(), false);
        let (url2, thread2) = server(body, body.len(), false);
        let files = [
            Transfer {
                url: &url1,
                path: &first,
                size: body.len() as u64,
                digest: Some(&digest),
            },
            Transfer {
                url: &url2,
                path: &second,
                size: body.len() as u64,
                digest: Some(&digest),
            },
        ];
        let result = fetch_aria(&files, &tool, &Control::default(), &|_, _| {});
        thread1.join().unwrap();
        thread2.join().unwrap();
        result.unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), body);
        assert_eq!(std::fs::read(&second).unwrap(), body);
        fetch_aria(
            &files,
            &tool,
            &Control::default().with_offline(true),
            &|_, _| {},
        )
        .unwrap();
        std::fs::write(&first, vec![0; body.len()]).unwrap();
        assert!(
            fetch_aria(
                &files,
                &tool,
                &Control::default().with_offline(true),
                &|_, _| {}
            )
            .unwrap_err()
            .to_string()
            .contains("Offline mode")
        );
        assert_eq!(std::fs::read(&second).unwrap(), body);
    }
    #[test]
    fn invalid_digest_never_starts_a_download() {
        let dir = tempfile::tempdir().unwrap();
        for digest in ["md5:abc", "sha256:abc", "sha256:gggg"] {
            let error = fetch_native(
                "http://127.0.0.1:1/unused",
                &dir.path().join("part"),
                1,
                Some(digest),
                &Control::default(),
                &|_, _| {},
            )
            .unwrap_err();
            assert!(error.to_string().contains("SHA-256"));
        }
    }
    fn server(
        body: &'static [u8],
        declared: usize,
        stall: bool,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut connection = loop {
                match listener.accept() {
                    Ok((connection, _)) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            start.elapsed() < Duration::from_secs(10),
                            "Fixture received no request"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            // Accepted sockets can inherit the listener's nonblocking mode.
            connection.set_nonblocking(false).unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut buffer = [0; 2048];
                let count = connection.read(&mut buffer).unwrap();
                assert!(count > 0, "Fixture request ended before its headers");
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() <= 16384, "Fixture request headers too large");
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
            );
            let _ = connection.write_all(header.as_bytes());
            if stall {
                std::thread::sleep(Duration::from_millis(700));
            }
            let _ = connection.write_all(body);
        });
        (format!("http://{address}/part"), thread)
    }
    #[test]
    fn fixture_waits_for_complete_request_headers() {
        let body = b"fragmented request fixture";
        let (url, thread) = server(body, body.len(), false);
        let address = url
            .strip_prefix("http://")
            .unwrap()
            .strip_suffix("/part")
            .unwrap();
        let mut connection = std::net::TcpStream::connect(address).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        connection
            .write_all(b"GET /part HTTP/1.1\r\nHost: localhost\r\n")
            .unwrap();
        let early = connection.read(&mut [0; 1]);
        assert!(
            matches!(early, Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)),
            "Fixture responded before the complete request headers"
        );
        connection.write_all(b"\r\n").unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut response = Vec::new();
        connection.read_to_end(&mut response).unwrap();
        thread.join().unwrap();
        assert!(response.ends_with(body));
    }
    #[test]
    fn download_verifies_digest_and_reuses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("day.tar.aa");
        let body = b"archive fixture";
        let hash = format!("sha256:{:x}", Sha256::digest(body));
        let (url, thread) = server(body, body.len(), false);
        fetch_file(
            &url,
            &path,
            body.len() as u64,
            Some(&hash),
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        thread.join().unwrap();
        // The server is gone; success here proves that verified caching works.
        fetch_file(
            &url,
            &path,
            body.len() as u64,
            Some(&hash),
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), body);
        std::fs::write(&path, b"corrupt fixture").unwrap();
        assert!(
            fetch_file(
                &url,
                &path,
                body.len() as u64,
                Some(&hash),
                &Control::default(),
                &|_, _| {}
            )
            .is_err()
        );
    }
    #[test]
    fn interrupted_and_bad_digest_downloads_leave_no_final_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("part");
        let (url, thread) = server(b"short", 100, false);
        assert!(fetch_file(&url, &path, 100, None, &Control::default(), &|_, _| {}).is_err());
        thread.join().unwrap();
        assert!(!path.exists());
        let (url, thread) = server(b"data", 4, false);
        let bad = format!("sha256:{}", "0".repeat(64));
        assert!(fetch_file(&url, &path, 4, Some(&bad), &Control::default(), &|_, _| {}).is_err());
        thread.join().unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn cancellation_interrupts_a_stalled_network_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("part");
        let (url, server_thread) = server(b"data", 4, true);
        let control = Control::default();
        let other = control.clone();
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            other.stop();
        });
        let start = std::time::Instant::now();
        let result = fetch_file(&url, &path, 4, None, &control, &|_, _| {});
        assert!(result.unwrap_err().to_string().contains("Cancelled"));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(!path.exists());
        cancel.join().unwrap();
        server_thread.join().unwrap();
    }
}
