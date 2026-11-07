//! Free GeoNames preparation for offline video text matching.
use crate::{Control, Progress, visual::hash_file};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    pub attribution: String,
    pub license: String,
    pub archive_sha256: String,
    pub text_sha256: String,
}

fn source_path(text: &Path) -> PathBuf {
    text.with_extension("source.json")
}

pub(crate) fn verify_source(text: &Path, hash: &str) -> Result<Option<Source>> {
    let path = source_path(text);
    if !path.exists() {
        return Ok(None);
    }
    let source: Source =
        serde_json::from_reader(File::open(path)?).context("Invalid GeoNames source record")?;
    if source.text_sha256 != hash {
        bail!(
            "GeoNames data does not match its source hash. Restore the prepared data before analysis."
        );
    }
    Ok(Some(source))
}

fn stem(country: &str) -> Result<String> {
    let country = country.to_ascii_uppercase();
    if country == "ALL" {
        return Ok("allCountries".into());
    }
    if country.len() != 2 || !country.bytes().all(|c| c.is_ascii_uppercase()) {
        bail!("Use a two-letter country code or ALL for worldwide place data");
    }
    Ok(country)
}

fn archive_size(response: &reqwest::Response) -> Result<u64> {
    // A HEAD response has an empty body. Its body size hint can be zero even
    // when Content-Length correctly describes the requested archive.
    let size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .context("GeoNames did not provide its archive size")?
        .to_str()?
        .parse()?;
    if size == 0 || size > 4 * 1024 * 1024 * 1024 {
        bail!("Unexpected GeoNames archive size");
    }
    Ok(size)
}

fn extract(archive: &Path, entry_name: &str, output: &Path, control: &Control) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    let mut entry = zip
        .by_name(entry_name)
        .context("GeoNames archive is missing its text file")?;
    if entry.size() == 0 || entry.size() > 10 * 1024 * 1024 * 1024 {
        bail!("Unexpected GeoNames text size");
    }
    // Extract only the requested country file. ZIP paths never become output paths.
    let mut temp =
        tempfile::NamedTempFile::new_in(output.parent().context("Missing output folder")?)?;
    let mut buffer = [0; 128 * 1024];
    let mut total = 0;
    loop {
        control.check()?;
        let n = entry.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > entry.size() {
            bail!("GeoNames text exceeds its declared size");
        }
        temp.write_all(&buffer[..n])?;
    }
    if total != entry.size() {
        bail!("GeoNames text is incomplete");
    }
    temp.persist(output)?;
    Ok(())
}

pub fn prepare(
    country: &str,
    folder: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    let stem = stem(country)?;
    let text = folder.join(format!("{stem}.txt"));
    if text.is_file() {
        let hash = hash_file(&text, control)?;
        if verify_source(&text, &hash)?.is_some() {
            progress("Verified cached GeoNames data", 1.);
            return Ok(text);
        }
        bail!(
            "The existing GeoNames file has no source record. Select another output folder to prepare verified data."
        );
    }
    let url = format!("https://download.geonames.org/export/dump/{stem}.zip");
    control.require_network(&url)?;
    std::fs::create_dir_all(folder)?;
    let archive = folder.join(format!("{stem}.zip"));
    let runtime = tokio::runtime::Runtime::new()?;
    let size = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let response = crate::network::cancelable(client.head(&url).send(), control)
            .await??
            .error_for_status()?;
        archive_size(&response)
    })?;
    crate::network::fetch_file(&url, &archive, size, None, control, progress)?;
    progress("Extracting GeoNames place data", 0.95);
    extract(&archive, &format!("{stem}.txt"), &text, control)?;
    let source = Source {
        url,
        attribution: "GeoNames".into(),
        license: "https://creativecommons.org/licenses/by/4.0/".into(),
        archive_sha256: hash_file(&archive, control)?,
        text_sha256: hash_file(&text, control)?,
    };
    let mut temp = tempfile::NamedTempFile::new_in(folder)?;
    serde_json::to_writer_pretty(&mut temp, &source)?;
    temp.persist(source_path(&text))?;
    progress("GeoNames data ready for offline use", 1.);
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn head_uses_archive_header_despite_empty_response_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 1024];
            let n = stream.read(&mut request).unwrap();
            assert!(request[..n].starts_with(b"HEAD /"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 422529142\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            let response = client
                .head(format!("http://{address}/archive.zip"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.content_length(), Some(0));
            assert_eq!(archive_size(&response).unwrap(), 422529142);
        });
        server.join().unwrap();
    }
    #[test]
    fn country_and_offline_integrity() {
        assert_eq!(stem("us").unwrap(), "US");
        assert_eq!(stem("all").unwrap(), "allCountries");
        assert!(stem("../US").is_err());
        let dir = tempfile::tempdir().unwrap();
        let control = Control::default().with_offline(true);
        assert!(
            prepare("US", dir.path(), &control, &|_, _| {})
                .unwrap_err()
                .to_string()
                .contains("Offline")
        );
        let text = dir.path().join("US.txt");
        std::fs::write(&text, "preserved data").unwrap();
        let source = Source {
            url: "https://download.geonames.org/export/dump/US.zip".into(),
            attribution: "GeoNames".into(),
            license: "CC BY 4.0".into(),
            archive_sha256: "test fixture".into(),
            text_sha256: hash_file(&text, &control).unwrap(),
        };
        serde_json::to_writer(File::create(source_path(&text)).unwrap(), &source).unwrap();
        assert_eq!(
            prepare("US", dir.path(), &control, &|_, _| {}).unwrap(),
            text
        );
        std::fs::write(&text, "changed data").unwrap();
        assert!(prepare("US", dir.path(), &control, &|_, _| {}).is_err());
        assert_eq!(std::fs::read_to_string(&text).unwrap(), "changed data");
    }
    #[test]
    fn extracts_only_requested_entry() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("US.zip");
        let mut writer = zip::ZipWriter::new(File::create(&archive).unwrap());
        writer
            .start_file("../escape.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"ignore this").unwrap();
        writer
            .start_file("US.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"country data").unwrap();
        writer.finish().unwrap();
        let output = dir.path().join("US.txt");
        extract(&archive, "US.txt", &output, &Control::default()).unwrap();
        assert_eq!(std::fs::read_to_string(output).unwrap(), "country data");
        assert!(
            extract(
                &archive,
                "GB.txt",
                &dir.path().join("GB.txt"),
                &Control::default()
            )
            .is_err()
        );
        assert!(!dir.path().join("escape.txt").exists());
    }
}
