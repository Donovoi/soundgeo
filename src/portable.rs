//! Validated extraction for the single-file Windows distribution.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub const MAGIC: &[u8; 16] = b"SoundGeoBundle01";
const TRAILER_LEN: u64 = 56;
const MAX_PAYLOAD: u64 = 2 * 1024 * 1024 * 1024;

/// Exposes only the ZIP section, excluding the launcher and its integrity trailer.
pub struct Payload {
    file: File,
    start: u64,
    len: u64,
    position: u64,
}

impl Read for Payload {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = buffer.len().min((self.len - self.position) as usize);
        let read = self.file.read(&mut buffer[..count])?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for Payload {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let next = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.len) + i128::from(n),
        };
        if next < 0 || next > i128::from(self.len) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Seek outside bundled ZIP",
            ));
        }
        self.position = next as u64;
        self.file
            .seek(SeekFrom::Start(self.start + self.position))?;
        Ok(self.position)
    }
}

pub fn open_payload(path: &Path) -> Result<Payload> {
    let mut file = File::open(path)?;
    let total = file.metadata()?.len();
    ensure!(total >= TRAILER_LEN, "Incomplete SoundGeo download");
    file.seek(SeekFrom::End(-(TRAILER_LEN as i64)))?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.read_exact(&mut trailer)?;
    ensure!(
        &trailer[..16] == MAGIC,
        "This executable has no SoundGeo runtime bundle"
    );
    let len = u64::from_le_bytes(trailer[16..24].try_into()?);
    ensure!(
        len > 0 && len <= MAX_PAYLOAD && len <= total - TRAILER_LEN,
        "Invalid runtime bundle length"
    );
    let start = total - TRAILER_LEN - len;
    file.seek(SeekFrom::Start(start))?;
    let mut payload = Payload {
        file,
        start,
        len,
        position: 0,
    };
    let mut hasher = Sha256::new();
    std::io::copy(&mut payload, &mut hasher)?;
    ensure!(
        hasher.finalize().as_slice() == &trailer[24..],
        "The SoundGeo download is damaged; download it again"
    );
    payload.rewind()?;
    Ok(payload)
}

#[derive(Deserialize)]
struct Build {
    files: BTreeMap<String, String>,
}

fn safe_path(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains(['\\', ':', '\0']),
        "Invalid bundled path"
    );
    for part in name.split('/') {
        let device = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        ensure!(
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' ']),
            "Invalid bundled path"
        );
        ensure!(
            !matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(device.len() == 4
                    && (device.starts_with("COM") || device.starts_with("LPT"))
                    && matches!(device.as_bytes()[3], b'1'..=b'9')),
            "Reserved bundled path"
        );
    }
    Ok(PathBuf::from(name))
}

/// Destination must be a new empty directory owned by this invocation.
/// No existing runtime is modified, so concurrent launches cannot replace code
/// underneath each other, and interrupted extractions are never reused.
pub fn extract<R: Read + Seek>(reader: R, destination: &Path) -> Result<PathBuf> {
    ensure!(
        fs::read_dir(destination)?.next().is_none(),
        "Extraction folder must be empty"
    );
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut extracted = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        ensure!(
            entry.is_file() && !entry.is_symlink(),
            "Unexpected bundled entry"
        );
        let relative = safe_path(&entry.name()?)?;
        total = total
            .checked_add(entry.size())
            .context("Bundle too large")?;
        ensure!(total <= MAX_PAYLOAD, "Bundle too large");
        let target = destination.join(&relative);
        fs::create_dir_all(target.parent().context("Missing bundle folder")?)?;
        let mut output = File::create_new(&target)?;
        let copied = std::io::copy(&mut entry, &mut output)?;
        ensure!(copied == entry.size(), "Truncated bundled file");
        output.flush()?;
        let digest = hash_file(&target)?;
        ensure!(
            extracted.insert(relative, digest).is_none(),
            "Duplicate bundled path"
        );
    }
    let roots = fs::read_dir(destination)?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        roots.len() == 1 && roots[0].file_type()?.is_dir(),
        "Expected one application folder"
    );
    let folder = roots[0].path();
    let build: Build = serde_json::from_reader(File::open(folder.join("BUILD.json"))?)?;
    let actual = extracted
        .into_iter()
        .filter_map(|(path, hash)| {
            let relative = path.strip_prefix(folder.file_name()?).ok()?;
            (relative != Path::new("BUILD.json"))
                .then(|| (relative.to_string_lossy().replace('\\', "/"), hash))
        })
        .collect::<BTreeMap<_, _>>();
    ensure!(
        actual == build.files,
        "Bundled files do not match BUILD.json"
    );
    for required in [
        "soundgeo.exe",
        "soundgeo-cli.exe",
        "ffmpeg.exe",
        "ffprobe.exe",
        "tesseract.exe",
        "tessdata/eng.traineddata",
        "opengl32.dll",
    ] {
        ensure!(
            folder.join(required).is_file(),
            "Missing bundled component: {required}"
        );
    }
    Ok(folder)
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn extract_to(path: &Path, destination: &Path) -> Result<PathBuf> {
    if destination.exists() {
        bail!(
            "Extraction destination already exists: {}",
            destination.display()
        );
    }
    let payload = open_payload(path)?;
    fs::create_dir_all(destination)?;
    extract(payload, destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn portable_paths_reject_windows_escape_and_device_names() {
        for name in [
            "../x",
            "/x",
            "a/../../x",
            "C:/x",
            "a\\x",
            "a/b:stream",
            "a/CON.txt",
            "a/LPT1",
            "a/b.",
            "a/b ",
            "a//b",
        ] {
            assert!(safe_path(name).is_err(), "{name}");
        }
        assert_eq!(
            safe_path("app/tessdata/eng.traineddata").unwrap(),
            PathBuf::from("app/tessdata/eng.traineddata")
        );
    }

    #[test]
    fn portable_payload_checks_truncation_hash_and_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.exe");
        let bytes = b"payload";
        let mut file = File::create(&path).unwrap();
        file.write_all(b"executable-prefix").unwrap();
        file.write_all(bytes).unwrap();
        file.write_all(MAGIC).unwrap();
        file.write_all(&(bytes.len() as u64).to_le_bytes()).unwrap();
        file.write_all(&Sha256::digest(bytes)).unwrap();
        drop(file);
        let mut payload = open_payload(&path).unwrap();
        assert!(payload.seek(SeekFrom::End(1)).is_err());
        assert!(payload.seek(SeekFrom::Current(-1)).is_err());
        let mut read = Vec::new();
        payload.read_to_end(&mut read).unwrap();
        assert_eq!(read, bytes);
        drop(payload);
        let mut corrupt = fs::read(&path).unwrap();
        corrupt[17] ^= 1;
        fs::write(&path, &corrupt).unwrap();
        assert!(open_payload(&path).is_err());
        fs::write(&path, &corrupt[..20]).unwrap();
        assert!(open_payload(&path).is_err());
    }

    #[test]
    fn portable_extraction_rejects_unlisted_files_and_preserves_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("app/BUILD.json", options).unwrap();
        zip.write_all(br#"{"files":{}}"#).unwrap();
        zip.start_file("app/unlisted.dll", options).unwrap();
        zip.write_all(b"unlisted").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(extract(Cursor::new(&bytes), dir.path()).is_err());
        let marker = dir.path().join("keep.txt");
        fs::write(&marker, b"keep").unwrap();
        assert!(extract(Cursor::new(bytes), dir.path()).is_err());
        assert_eq!(fs::read(marker).unwrap(), b"keep");
    }
}
