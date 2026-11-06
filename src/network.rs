use crate::{Control, Progress};
use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

fn hash_file(path: &Path, control: &Control) -> Result<String> {
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
    control.check()?;
    let expected = digest
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(str::to_ascii_lowercase);
    if expected
        .as_ref()
        .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        bail!("Invalid archive SHA-256 digest");
    }
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
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
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
    temp.persist(path)?;
    std::fs::write(stamp, hash)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn server(
        body: &'static [u8],
        declared: usize,
        stall: bool,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let _ = connection.read(&mut request);
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
