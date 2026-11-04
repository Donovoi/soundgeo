use crate::{
    Control, Job, Progress,
    geo::{Bounds, Point},
    model::{Sample, Track},
};
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug)]
pub struct Dataset {
    pub tracks: Vec<Track>,
    pub warnings: Vec<String>,
}
#[derive(Deserialize)]
struct CsvRow {
    aircraft: String,
    time: String,
    lat: f64,
    lon: f64,
    altitude_m: f64,
}
pub fn parse_time(s: &str) -> Result<f64> {
    let v = if let Ok(v) = s.parse::<f64>() {
        v
    } else {
        chrono::DateTime::parse_from_rfc3339(s)
            .context("Use Unix seconds or an RFC3339 time with timezone")?
            .timestamp_millis() as f64
            / 1000.
    };
    if !v.is_finite() || !(0. ..=4102444800.).contains(&v) {
        bail!("Invalid UTC timestamp {s}");
    }
    Ok(v)
}

pub fn load(path: &Path, control: &Control) -> Result<Dataset> {
    load_filtered(path, control, &Filter::default())
}

#[derive(Default)]
struct Filter {
    bounds: Option<Bounds>,
    time: Option<(f64, f64)>,
}
impl Filter {
    fn accepts(&self, p: Point, time: f64) -> bool {
        self.time.is_none_or(|(a, b)| time >= a && time <= b)
            && self.bounds.is_none_or(|b| {
                // Keep aircraft outside the observer's region: audible slant range can be large.
                let lon_margin = 0.6 / p.lat.to_radians().cos().abs().max(0.05);
                p.lat >= b.south - 0.6
                    && p.lat <= b.north + 0.6
                    && p.lon >= b.west - lon_margin
                    && p.lon <= b.east + lon_margin
            })
    }
}
pub fn load_for_job(job: &Job, duration: f64, control: &Control) -> Result<Dataset> {
    let filter = Filter {
        bounds: job.bounds,
        time: job.start_utc.map(|t| {
            (
                t - job.time_tolerance_s - 180.,
                t + job.time_tolerance_s + duration + 180.,
            )
        }),
    };
    load_filtered(&job.tracks, control, &filter)
}
fn load_filtered(path: &Path, control: &Control, filter: &Filter) -> Result<Dataset> {
    let mut raw: BTreeMap<String, Vec<Sample>> = BTreeMap::new();
    let mut skipped = 0usize;
    let mut barometric = 0usize;
    let mut total = 0usize;
    let paths: Vec<PathBuf> = if path.is_dir() {
        walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| {
                let n = p.file_name().unwrap_or_default().to_string_lossy();
                n.ends_with(".csv")
                    || n.starts_with("trace_full_")
                    || n.ends_with(".tar.aa")
                    || n.ends_with(".tar")
            })
            .collect()
    } else {
        vec![path.to_owned()]
    };
    if paths.is_empty() {
        bail!("No CSV or readsb trace files found in this folder.");
    }
    for path in paths {
        control.check()?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.ends_with(".csv") {
            let mut reader = csv::Reader::from_path(&path)?;
            for (i, row) in reader.deserialize::<CsvRow>().enumerate() {
                if i % 10000 == 0 {
                    control.check()?;
                }
                let row=row.with_context(||format!("Invalid CSV row {} in {}. Required headers: aircraft,time,lat,lon,altitude_m",i+2,path.display()))?;
                let point = Point {
                    lat: row.lat,
                    lon: row.lon,
                    alt: row.altitude_m,
                };
                if !point.valid() || row.aircraft.trim().is_empty() {
                    bail!("Invalid position or aircraft at CSV row {}", i + 2);
                }
                let time = parse_time(&row.time)?;
                if !filter.accepts(point, time) {
                    continue;
                }
                raw.entry(row.aircraft)
                    .or_default()
                    .push(Sample::new(time, point));
                total += 1;
                if total > 10_000_000 {
                    bail!(
                        "Flight data exceeds 10 million points. Filter the archive to a smaller region or time range."
                    );
                }
            }
        } else if name.ends_with(".tar.aa") || name.ends_with(".tar") {
            let reader: Box<dyn Read> = if name.ends_with(".aa") {
                Box::new(Parts::new(&path)?)
            } else {
                Box::new(BufReader::new(File::open(&path)?))
            };
            let mut archive = tar::Archive::new(reader);
            for entry in archive.entries()? {
                control.check()?;
                let entry = entry?;
                let p = entry.path()?.into_owned();
                if !entry.header().entry_type().is_file()
                    || !p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .starts_with("trace_full_")
                {
                    continue;
                }
                if entry.size() > 32_000_000 {
                    skipped += 1;
                    continue;
                }
                let bytes = read_limited(GzDecoder::new(entry))?;
                ingest_trace(
                    &bytes,
                    &mut raw,
                    &mut skipped,
                    &mut barometric,
                    &mut total,
                    filter,
                )?;
            }
        } else {
            let file = File::open(&path)?;
            let bytes = if name.ends_with(".gz") {
                read_limited(GzDecoder::new(file))?
            } else {
                read_limited(file)?
            };
            ingest_trace(
                &bytes,
                &mut raw,
                &mut skipped,
                &mut barometric,
                &mut total,
                filter,
            )?;
        }
    }
    let mut tracks = vec![];
    for (id, mut samples) in raw {
        control.check()?;
        samples.sort_by(|a, b| a.time.total_cmp(&b.time));
        samples.dedup_by(|a, b| (a.time - b.time).abs() < 0.01);
        let mut segment = vec![];
        let mut index = 0;
        for s in samples {
            if let Some(prev) = segment.last() {
                let prev: &Sample = prev;
                let dt = s.time - prev.time;
                let speed = (s.xyz - prev.xyz).norm() / dt;
                if dt > 60. || speed > 280. {
                    if segment.len() >= 8 {
                        tracks.push(Track {
                            id: format!("{id}/{index}"),
                            samples: std::mem::take(&mut segment),
                        });
                    }
                    segment.clear();
                    index += 1;
                }
            }
            segment.push(s);
        }
        if segment.len() >= 8 {
            tracks.push(Track {
                id: format!("{id}/{index}"),
                samples: segment,
            });
        }
    }
    if tracks.is_empty() {
        bail!(
            "No usable flight segments. Each needs at least 8 valid airborne positions, at most 60 seconds apart."
        );
    }
    let mut warnings = vec![];
    if skipped > 0 {
        warnings.push(format!(
            "Skipped {skipped} malformed, ground, or missing-altitude trace records."
        ));
    }
    if barometric > 0 {
        warnings.push(format!("{barometric} positions use pressure altitude; weather-dependent height error affects location estimates."));
    }
    warnings.push("Flight data is not filtered by aircraft type. A tonal match alone does not identify a propeller aircraft.".into());
    Ok(Dataset { tracks, warnings })
}
fn read_limited(reader: impl Read) -> Result<Vec<u8>> {
    let mut data = vec![];
    reader.take(32_000_001).read_to_end(&mut data)?;
    if data.len() > 32_000_000 {
        bail!("A trace exceeds the 32 MB decompression limit.");
    }
    Ok(data)
}
fn ingest_trace(
    bytes: &[u8],
    raw: &mut BTreeMap<String, Vec<Sample>>,
    skipped: &mut usize,
    barometric: &mut usize,
    total: &mut usize,
    filter: &Filter,
) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes).context("Invalid readsb JSON")?;
    let id = value["icao"]
        .as_str()
        .or_else(|| value["hex"].as_str())
        .context("Trace is missing icao")?;
    let base = value["timestamp"]
        .as_f64()
        .context("Trace is missing timestamp")?;
    for row in value["trace"]
        .as_array()
        .context("Trace is missing trace array")?
    {
        let (Some(relative), Some(lat), Some(lon)) =
            (row[0].as_f64(), row[1].as_f64(), row[2].as_f64())
        else {
            *skipped += 1;
            continue;
        };
        if row[3].as_str() == Some("ground") || row[6].as_u64().unwrap_or(0) & 1 != 0 {
            *skipped += 1;
            continue;
        }
        // readsb trace index 10 is GNSS altitude in feet; index 3 may be pressure altitude.
        let geometric = row[10].as_f64().or_else(|| row[8]["alt_geom"].as_f64());
        let Some(alt) = geometric.or_else(|| row[3].as_f64()) else {
            *skipped += 1;
            continue;
        };
        if geometric.is_none() && row[6].as_u64().unwrap_or(0) & 8 == 0 {
            *barometric += 1;
        }
        let p = Point {
            lat,
            lon,
            alt: alt * 0.3048,
        };
        let t = base + relative;
        if !p.valid() || !t.is_finite() || !(0. ..=4102444800.).contains(&t) {
            *skipped += 1;
            continue;
        }
        if !filter.accepts(p, t) {
            continue;
        }
        let samples = raw.entry(id.into()).or_default();
        if samples
            .last()
            .is_some_and(|s| t >= s.time && t - s.time < 2.)
        {
            continue;
        }
        samples.push(Sample::new(t, p));
        *total += 1;
        if *total > 10_000_000 {
            bail!("Flight data exceeds 10 million points. Import a smaller regional/time subset.");
        }
    }
    Ok(())
}

struct Parts {
    paths: Vec<PathBuf>,
    index: usize,
    current: BufReader<File>,
}
impl Parts {
    fn new(first: &Path) -> Result<Self> {
        let prefix = first.to_string_lossy().trim_end_matches(".aa").to_string();
        let mut paths = vec![];
        for a in b'a'..=b'z' {
            for b in b'a'..=b'z' {
                let p = PathBuf::from(format!("{prefix}.{}{}", a as char, b as char));
                if p.exists() {
                    paths.push(p);
                } else {
                    break;
                }
            }
        }
        if paths.is_empty() {
            bail!("Missing first archive part");
        }
        Ok(Self {
            current: BufReader::new(File::open(&paths[0])?),
            paths,
            index: 0,
        })
    }
}
impl Read for Parts {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let n = self.current.read(buf)?;
            if n > 0 || self.index + 1 == self.paths.len() {
                return Ok(n);
            }
            self.index += 1;
            self.current = BufReader::new(File::open(&self.paths[self.index])?);
        }
    }
}

/// Downloads original split archives without extracting untrusted paths. No video is uploaded.
pub fn download_day(
    date: &str,
    destination: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .context("Use a date such as 2026-09-08")?;
    let year = day.format("%Y");
    let client = reqwest::blocking::Client::builder()
        .user_agent("soundgeo/0.1 (historical flight research)")
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .build()?;
    progress("Looking up the daily archive", 0.01);
    let mut release = None;
    for instance in ["prod-0", "prod-1", "staging-0"] {
        control.check()?;
        let tag = format!("v{}-planes-readsb-{instance}", day.format("%Y.%m.%d"));
        let url = format!(
            "https://api.github.com/repos/adsblol/globe_history_{year}/releases/tags/{tag}"
        );
        let response = client.get(url).send()?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        let value: Value = response.error_for_status()?.json()?;
        release = Some(value);
        break;
    }
    let release=release.context("No public flight archive found for this day. Try another date or import your own flight CSV.")?;
    let mut assets: Vec<_> = release["assets"]
        .as_array()
        .context("Archive has no assets")?
        .iter()
        .filter(|v| v["name"].as_str().is_some_and(|n| n.contains(".tar.")))
        .collect();
    assets.sort_by_key(|v| v["name"].as_str().unwrap_or(""));
    if assets.is_empty() {
        bail!("No split archive files in this release.");
    }
    let total: u64 = assets.iter().map(|v| v["size"].as_u64().unwrap_or(0)).sum();
    let dir = destination.join(date);
    std::fs::create_dir_all(&dir)?;
    let mut completed = 0u64;
    let mut first = None;
    for asset in assets {
        control.check()?;
        let name = asset["name"].as_str().context("Missing asset name")?;
        if name.contains(['/', '\\']) || name.contains("..") {
            bail!("Invalid archive filename");
        }
        let path = dir.join(name);
        let size = asset["size"].as_u64().context("Missing asset size")?;
        if first.is_none() {
            first = Some(path.clone());
        }
        if path.metadata().is_ok_and(|m| m.len() == size) {
            completed += size;
            continue;
        }
        let url = asset["browser_download_url"]
            .as_str()
            .context("Missing download URL")?;
        if !url.starts_with("https://github.com/adsblol/") {
            bail!("Unexpected archive host");
        }
        let mut response = client
            .get(url)
            .timeout(Duration::from_secs(7200))
            .send()?
            .error_for_status()?;
        let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
        let mut buffer = [0u8; 262144];
        let mut written = 0u64;
        loop {
            control.check()?;
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            temp.write_all(&buffer[..n])?;
            written += n as u64;
            progress(
                &format!(
                    "Downloading {:.2} / {:.2} GB",
                    (completed + written) as f64 / 1e9,
                    total as f64 / 1e9
                ),
                (completed + written) as f32 / total.max(1) as f32,
            );
        }
        if written != size {
            bail!(
                "Incomplete archive part: received {written}, expected {size} bytes. Retry to reuse completed parts."
            );
        }
        temp.persist(&path)?;
        completed += written;
    }
    let license = "Source: https://www.adsb.lol/docs/open-data/historical\nDaily archive: Open Database License 1.0 https://opendatacommons.org/licenses/odbl/1-0/\nKeep attribution and check share-alike requirements when redistributing derived databases.\n";
    std::fs::write(dir.join("DATA-LICENSE.txt"), license)?;
    first.context("No archive parts")
}
