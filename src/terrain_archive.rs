//! Free, bounded Skadi downloads. Files are usable only after gzip and size checks.
use crate::{Control, Progress, geo::Bounds, network::cancelable};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
    time::Duration,
};

const SOURCE: &str = "https://elevation-tiles-prod.s3.amazonaws.com/skadi";
const MAX_TILE_BYTES: usize = 3601 * 3601 * 2;
const MAX_COMPRESSED_BYTES: usize = 32 * 1024 * 1024;
const MAX_TILES: usize = 64;
pub const ATTRIBUTION: &str = include_str!("../assets/terrain-attribution.txt");

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TileRecord {
    pub tile: String,
    pub source_url: String,
    pub sha256: String,
    pub compressed_sha256: String,
    pub size_bytes: usize,
    pub downloaded_utc: String,
}

/// Include the north/east boundary because Bounds::contains includes it too.
pub fn tiles(bounds: Bounds) -> Result<Vec<String>> {
    bounds.validate()?;
    let south = bounds.south.floor() as i32;
    let north = bounds.north.floor() as i32;
    let west = bounds.west.floor() as i32;
    let east = bounds.east.floor() as i32;
    if (north - south + 1) as usize * (east - west + 1) as usize > MAX_TILES {
        bail!(
            "Terrain download covers more than {MAX_TILES} tiles. Use a smaller search area or supply local terrain."
        );
    }
    let mut result = Vec::new();
    for lat in south..=north {
        for lon in west..=east {
            let lon = if lon == 180 { -180 } else { lon };
            result.push(format!(
                "{}{:02}{}{:03}",
                if lat < 0 { "S" } else { "N" },
                lat.abs(),
                if lon < 0 { "W" } else { "E" },
                lon.abs()
            ));
        }
    }
    Ok(result)
}

fn valid_size(size: usize) -> bool {
    size == 1201 * 1201 * 2 || size == MAX_TILE_BYTES
}

fn cached(root: &Path, tile: &str, url: &str, control: &Control) -> Result<Option<TileRecord>> {
    let record: TileRecord = match std::fs::read(root.join(format!("{tile}.terrain.json")))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
    {
        Some(record) => record,
        None => return Ok(None),
    };
    if record.tile != tile || record.source_url != url || !valid_size(record.size_bytes) {
        return Ok(None);
    }
    let path = root.join(format!("{tile}.hgt"));
    if !path
        .metadata()
        .is_ok_and(|m| m.len() == record.size_bytes as u64)
    {
        return Ok(None);
    }
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        control.check()?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok((format!("{:x}", hash.finalize()) == record.sha256).then_some(record))
}

fn write_atomic(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(root.join(name))?;
    Ok(())
}

pub fn ensure(
    bounds: Bounds,
    root: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Vec<TileRecord>> {
    ensure_from(bounds, root, SOURCE, control, progress)
}

fn ensure_from(
    bounds: Bounds,
    root: &Path,
    source: &str,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Vec<TileRecord>> {
    let names = tiles(bounds)?;
    control.check()?;
    std::fs::create_dir_all(root).context("Could not create the terrain cache")?;
    write_atomic(root, "TERRAIN-ATTRIBUTION.txt", ATTRIBUTION.as_bytes())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .user_agent("soundgeo/0.3")
        .build()?;
    let mut records = Vec::new();
    for (i, tile) in names.iter().enumerate() {
        control.check()?;
        let url = format!("{source}/{}/{tile}.hgt.gz", &tile[..3]);
        if let Some(record) = cached(root, tile, &url, control)? {
            progress(
                &format!("Using verified terrain {tile}"),
                (i + 1) as f32 / names.len() as f32,
            );
            records.push(record);
            continue;
        }
        progress(
            &format!("Downloading free terrain {tile}"),
            i as f32 / names.len() as f32,
        );
        control.require_network(&root.join(format!("{tile}.hgt")).display().to_string())?;
        let compressed = runtime
            .block_on(async {
                let mut response = cancelable(client.get(&url).send(), control)
                    .await??
                    .error_for_status()?;
                if response
                    .content_length()
                    .is_some_and(|n| n > MAX_COMPRESSED_BYTES as u64)
                {
                    bail!("Terrain response exceeds the download size limit");
                }
                let mut bytes = Vec::new();
                while let Some(chunk) = cancelable(response.chunk(), control).await?? {
                    if bytes.len() + chunk.len() > MAX_COMPRESSED_BYTES {
                        bail!("Terrain response exceeds the download size limit");
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok::<_, anyhow::Error>(bytes)
            })
            .with_context(|| {
                format!("Could not download terrain {tile}; verified earlier tiles remain cached")
            })?;
        // Read through EOF to check the gzip CRC and length trailer. A decompression
        // cap prevents a small, invalid response from allocating unbounded memory.
        let mut decoder = flate2::bufread::GzDecoder::new(compressed.as_slice());
        let mut decoded = Vec::new();
        let mut buffer = [0; 65536];
        loop {
            control.check()?;
            let n = decoder
                .read(&mut buffer)
                .context("Invalid terrain gzip data")?;
            if n == 0 {
                break;
            }
            if decoded.len() + n > MAX_TILE_BYTES {
                bail!("Terrain tile is too large");
            }
            decoded.extend_from_slice(&buffer[..n]);
        }
        if !decoder.get_ref().is_empty() {
            bail!("Terrain response contains trailing data");
        }
        if !valid_size(decoded.len()) {
            bail!("Terrain tile has an invalid HGT size");
        }
        control.check()?;
        let record = TileRecord {
            tile: tile.clone(),
            source_url: url,
            sha256: format!("{:x}", Sha256::digest(&decoded)),
            compressed_sha256: format!("{:x}", Sha256::digest(&compressed)),
            size_bytes: decoded.len(),
            downloaded_utc: chrono::Utc::now().to_rfc3339(),
        };
        write_atomic(root, &format!("{tile}.hgt"), &decoded)?;
        write_atomic(
            root,
            &format!("{tile}.terrain.json"),
            &serde_json::to_vec_pretty(&record)?,
        )?;
        records.push(record);
        progress(
            &format!("Verified terrain {tile}"),
            (i + 1) as f32 / names.len() as f32,
        );
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::{Point, Terrain, geoid_undulation};

    fn area() -> Bounds {
        Bounds {
            south: 61.1,
            north: 61.3,
            west: -149.9,
            east: -149.7,
        }
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn server(body: Vec<u8>, extra_declared_bytes: usize) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let _ = stream.read(&mut request);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len() + extra_declared_bytes
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        });
        (format!("http://{address}"), thread)
    }

    #[test]
    fn terrain_tile_selection_includes_boundaries_and_date_line() {
        assert_eq!(tiles(area()).unwrap(), ["N61W150"]);
        assert_eq!(
            tiles(Bounds {
                south: -1.1,
                north: -1.,
                west: 179.9,
                east: 180.
            })
            .unwrap(),
            ["S02E179", "S02W180", "S01E179", "S01W180"]
        );
        assert_eq!(
            tiles(Bounds {
                south: -0.1,
                north: 0.,
                west: -0.1,
                east: 0.
            })
            .unwrap(),
            ["S01W001", "S01E000", "N00W001", "N00E000"]
        );
        assert!(
            tiles(Bounds {
                south: -80.,
                north: 80.,
                west: -170.,
                east: 170.
            })
            .is_err()
        );
        assert!(
            tiles(Bounds {
                north: f64::NAN,
                ..area()
            })
            .is_err()
        );
    }

    #[test]
    fn terrain_download_cache_hash_and_elevation_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = [0, 23].repeat(1201 * 1201);
        let body = gzip(&bytes);
        let (url, thread) = server(body.clone(), 0);
        let records =
            ensure_from(area(), dir.path(), &url, &Control::default(), &|_, _| {}).unwrap();
        thread.join().unwrap();
        assert_eq!(records[0].sha256, format!("{:x}", Sha256::digest(&bytes)));
        assert_eq!(
            records[0].compressed_sha256,
            format!("{:x}", Sha256::digest(&body))
        );
        // No server remains. Reuse must verify and read the local file offline.
        assert_eq!(
            ensure_from(
                area(),
                dir.path(),
                &url,
                &Control::default().with_offline(true),
                &|_, _| {}
            )
            .unwrap(),
            records
        );
        let p = Point {
            lat: 61.2,
            lon: -149.8,
            alt: 0.,
        };
        let mut terrain = Terrain::new(Some(dir.path().to_path_buf()), 999.);
        assert!((terrain.elevation(p) - 23. - geoid_undulation(p.lat, p.lon)).abs() < 1e-8);
        let mut corrupt = bytes.clone();
        corrupt[0] = 1;
        std::fs::write(dir.path().join("N61W150.hgt"), &corrupt).unwrap();
        assert!(
            cached(
                dir.path(),
                "N61W150",
                &records[0].source_url,
                &Control::default()
            )
            .unwrap()
            .is_none()
        );
        // A repair uses a newly fetched, complete tile; same length is insufficient.
        let (repair_url, thread) = server(body, 0);
        let repaired = ensure_from(
            area(),
            dir.path(),
            &repair_url,
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        thread.join().unwrap();
        assert_eq!(repaired[0].sha256, records[0].sha256);
        assert_eq!(
            std::fs::read(dir.path().join("N61W150.hgt")).unwrap(),
            bytes
        );
        assert!(dir.path().join("TERRAIN-ATTRIBUTION.txt").is_file());
    }

    #[test]
    fn bad_terrain_responses_never_publish_a_tile() {
        let good = gzip(&vec![0; 1201 * 1201 * 2]);
        let mut bad_crc = good.clone();
        let n = bad_crc.len();
        bad_crc[n - 8] ^= 1;
        let mut trailing = good.clone();
        trailing.push(0);
        for (body, extra) in [
            (b"<html>error</html>".to_vec(), 0),
            (gzip(b"short"), 0),
            (bad_crc, 0),
            (good, 50),
            (trailing, 0),
            (gzip(&vec![0; MAX_TILE_BYTES + 1]), 0),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (url, thread) = server(body, extra);
            assert!(
                ensure_from(area(), dir.path(), &url, &Control::default(), &|_, _| {}).is_err()
            );
            thread.join().unwrap();
            assert!(!dir.path().join("N61W150.hgt").exists());
            assert!(!dir.path().join("N61W150.terrain.json").exists());
        }
    }

    #[test]
    fn terrain_cancellation_and_date_line_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let control = Control::default();
        control.stop();
        let output = dir.path().join("not-created");
        assert!(ensure(area(), &output, &control, &|_, _| {}).is_err());
        assert!(!output.exists());
        std::fs::write(dir.path().join("N61W180.hgt"), [0, 17].repeat(1201 * 1201)).unwrap();
        let mut terrain = Terrain::new(Some(dir.path().to_path_buf()), 999.);
        let west = terrain.elevation(Point {
            lat: 61.2,
            lon: -180.,
            alt: 0.,
        });
        let east = terrain.elevation(Point {
            lat: 61.2,
            lon: 180.,
            alt: 0.,
        });
        assert_eq!(west, east);
        assert!((west - 17. - geoid_undulation(61.2, 180.)).abs() < 1e-8);
    }
}
