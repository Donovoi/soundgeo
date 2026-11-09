use crate::aircraft::{AircraftFilter, AircraftKind, Catalog};
use crate::{
    AnalysisMethod, Control, Job, Progress,
    geo::{Bounds, Point},
    model::{Sample, Track},
};
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    time::Duration,
};

#[path = "data_cache.rs"]
mod cache;

#[derive(Debug)]
pub struct Dataset {
    pub tracks: Vec<Track>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct RawTracks {
    samples: BTreeMap<String, Vec<Sample>>,
    reception_gaps: BTreeMap<String, BTreeSet<u64>>,
}
#[derive(Deserialize)]
struct CsvRow {
    aircraft: String,
    time: String,
    lat: f64,
    lon: f64,
    altitude_m: f64,
    #[serde(default)]
    type_code: String,
    #[serde(default)]
    type_description: String,
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
    load_filtered(&[path.to_owned()], control, &Filter::default())
}

struct Filter {
    bounds: Option<Bounds>,
    time: Option<(f64, f64)>,
    archive_days: Vec<(f64, f64)>,
    aircraft: AircraftFilter,
    catalog: Catalog,
    minimum_positions: usize,
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            bounds: None,
            time: None,
            archive_days: Vec::new(),
            aircraft: AircraftFilter::default(),
            catalog: Catalog::default(),
            minimum_positions: 8,
        }
    }
}
impl Filter {
    fn accepts(&self, p: Point, time: f64) -> bool {
        self.time.is_none_or(|(a, b)| time >= a && time <= b)
            && (self.archive_days.is_empty()
                || self
                    .archive_days
                    .iter()
                    .any(|&(a, b)| time >= a && time < b))
            && self.bounds.is_none_or(|b| b.contains_flight(p))
    }
}
pub fn load_for_job(job: &Job, duration: f64, control: &Control) -> Result<Dataset> {
    load_for_job_sources(job, std::slice::from_ref(&job.tracks), duration, control)
}
pub fn load_for_job_sources(
    job: &Job,
    sources: &[PathBuf],
    duration: f64,
    control: &Control,
) -> Result<Dataset> {
    let filter = Filter {
        archive_days: if let Some(archive) = &job.archive
            && archive.provider.sample_resolution().is_some()
        {
            archive
                .dates()?
                .iter()
                .map(|date| {
                    let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
                    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64;
                    Ok((start, start + 86400.))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        },
        // Passage timing uses consecutive position pairs, rather than a
        // sustained Doppler curve. Keep three real observations as a minimum.
        minimum_positions: match job.analysis_method {
            AnalysisMethod::Doppler => 8,
            AnalysisMethod::PassageTiming => 3,
        },
        bounds: job.bounds,
        aircraft: job.aircraft_filter,
        catalog: job
            .type_catalog
            .as_deref()
            .map(Catalog::read)
            .transpose()?
            .unwrap_or_default(),
        time: job.start_utc.map(|t| {
            (
                t - job.time_tolerance_s - 180.,
                t + job.time_tolerance_s + duration * (1. + job.max_clock_drift_ppm * 1e-6) + 180.,
            )
        }),
    };
    let prepared_cache = cache::PreparedCache::for_job(job, sources, &filter, control)?;
    let mut dataset = match prepared_cache
        .as_ref()
        .and_then(|cache| cache.load().ok().flatten())
    {
        Some(dataset) => dataset,
        None => {
            let dataset = load_filtered(sources, control, &filter)?;
            if let Some(cache) = &prepared_cache {
                // This optional cache must not prevent use of read-only archives.
                let _ = cache.save(&dataset, control);
            }
            dataset
        }
    };
    if job.smooth_tracks {
        for track in &mut dataset.tracks {
            control.check()?;
            track.smooth_positions();
        }
        dataset.warnings.push("Flight positions use a centered 10-second local linear fit to reduce quantization noise. Track endpoints, gaps and timestamps are preserved; curved/accelerating motion may still incur model error.".into());
    }
    Ok(dataset)
}
fn source_paths(sources: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = sources
        .iter()
        .flat_map(|path| {
            if path.is_dir() {
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
            }
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
}
fn load_filtered(sources: &[PathBuf], control: &Control, filter: &Filter) -> Result<Dataset> {
    let mut raw = RawTracks::default();
    let mut kinds = BTreeMap::new();
    let mut skipped = 0usize;
    let mut barometric = 0usize;
    let mut total = 0usize;
    let paths = source_paths(sources);
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
                let provided = filter.catalog.kind(&row.type_code, &row.type_description);
                let kind = if provided == AircraftKind::Unknown {
                    kinds.get(&row.aircraft).copied().unwrap_or_default()
                } else {
                    provided
                };
                kinds.insert(row.aircraft.clone(), kind);
                // CSV type metadata may first appear on a later row or file.
                // Apply aircraft filtering after all rows have resolved it;
                // region/time filtering still bounds the positions retained.
                if !filter.accepts(point, time) {
                    continue;
                }
                raw.samples
                    .entry(row.aircraft)
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
                    &mut kinds,
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
                &mut kinds,
            )?;
        }
    }
    let mut tracks = vec![];
    for (id, mut samples) in raw.samples {
        let kind = kinds.get(&id).copied().unwrap_or_default();
        if !filter.aircraft.accepts(kind) {
            continue;
        }
        control.check()?;
        samples.sort_by(|a, b| a.time.total_cmp(&b.time));
        samples.dedup_by(|a, b| (a.time - b.time).abs() < 0.01);
        let mut segment = vec![];
        let mut index = 0;
        let reception_gaps = raw.reception_gaps.get(&id);
        for s in samples {
            if let Some(prev) = segment.last() {
                let prev: &Sample = prev;
                let dt = s.time - prev.time;
                let speed = (s.xyz - prev.xyz).norm() / dt;
                // Deduplication can retain a source timestamp up to 0.01 seconds
                // from a flagged copy. Preserve the boundary from either copy.
                let reception_gap = reception_gaps.is_some_and(|times| {
                    times
                        .range((prev.time + 0.01).to_bits()..=(s.time + 0.01).to_bits())
                        .next()
                        .is_some()
                });
                if dt > 60. || speed > 280. || reception_gap {
                    if segment.len() >= filter.minimum_positions {
                        tracks.push(Track {
                            id: format!("{id}/{index}"),
                            kind,
                            samples: std::mem::take(&mut segment),
                        });
                    }
                    segment.clear();
                    index += 1;
                }
            }
            segment.push(s);
        }
        if segment.len() >= filter.minimum_positions {
            tracks.push(Track {
                id: format!("{id}/{index}"),
                kind,
                samples: segment,
            });
        }
    }
    if tracks.is_empty() {
        return Err(EmptyFlightData(filter.minimum_positions).into());
    }
    let mut warnings = vec![];
    let reception_gaps: usize = raw.reception_gaps.values().map(BTreeSet::len).sum();
    if reception_gaps > 0 {
        warnings.push(format!("Read {reception_gaps} reception-gap markers. Recorded positions after each gap are retained; interpolation does not cross the gap."));
    }
    let short = tracks
        .iter()
        .filter(|track| track.samples.len() < 8)
        .count();
    if short > 0 {
        warnings.push(format!("Passage timing includes {short} short flight segments with 3 to 7 recorded positions. Positions are not added and gaps are not extended."));
    }
    if skipped > 0 {
        warnings.push(format!(
            "Skipped {skipped} malformed, ground, or missing-altitude trace records."
        ));
    }
    if barometric > 0 {
        warnings.push(format!("{barometric} positions use pressure altitude; weather-dependent height error affects location estimates."));
    }
    let unknown = tracks
        .iter()
        .filter(|t| t.kind == AircraftKind::Unknown)
        .count();
    if unknown > 0 {
        warnings.push(format!("{unknown} flight segments have unknown aircraft type. Load the type catalog or use propeller-only mode to exclude unclassified aircraft."));
    }
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
    raw: &mut RawTracks,
    skipped: &mut usize,
    barometric: &mut usize,
    total: &mut usize,
    filter: &Filter,
    kinds: &mut BTreeMap<String, AircraftKind>,
) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes).context("Invalid readsb JSON")?;
    let id = value["icao"]
        .as_str()
        .or_else(|| value["hex"].as_str())
        .context("Trace is missing icao")?;
    let base = value["timestamp"]
        .as_f64()
        .context("Trace is missing timestamp")?;
    let kind = filter.catalog.kind(
        value["t"]
            .as_str()
            .or_else(|| value["type"].as_str())
            .unwrap_or(""),
        value["desc"].as_str().unwrap_or(""),
    );
    let kind = if kind == AircraftKind::Unknown {
        kinds.get(id).copied().unwrap_or_default()
    } else {
        kind
    };
    kinds.insert(id.to_owned(), kind);
    if !filter.aircraft.accepts(kind) {
        return Ok(());
    }
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
        let t = base + relative;
        // readsb flag 1 marks a new reliable position following a reception gap.
        // Its timestamp and coordinates are valid; the preceding connection is not.
        let reception_gap = row[6].as_u64().unwrap_or(0) & 1 != 0;
        let horizontal = Point { lat, lon, alt: 0. };
        if reception_gap
            && horizontal.valid()
            && t.is_finite()
            && (0. ..=4102444800.).contains(&t)
            && filter.accepts(horizontal, t)
        {
            // Keep the boundary even if this row has no usable airborne altitude.
            raw.reception_gaps
                .entry(id.into())
                .or_default()
                .insert(t.to_bits());
        }
        if row[3].as_str() == Some("ground") {
            *skipped += 1;
            continue;
        }
        // readsb trace index 10 is GNSS altitude in feet; index 3 may be pressure altitude.
        let geometric = row[10].as_f64().or_else(|| row[8]["alt_geom"].as_f64());
        let Some(alt) = geometric.or_else(|| row[3].as_f64()) else {
            *skipped += 1;
            continue;
        };
        let p = Point {
            lat,
            lon,
            alt: alt * 0.3048,
        };
        if !p.valid() || !t.is_finite() || !(0. ..=4102444800.).contains(&t) {
            *skipped += 1;
            continue;
        }
        if !filter.accepts(p, t) {
            continue;
        }
        let samples = raw.samples.entry(id.into()).or_default();
        if !reception_gap
            && samples
                .last()
                .is_some_and(|s| t >= s.time && t - s.time < 2.)
        {
            continue;
        }
        if geometric.is_none() && row[6].as_u64().unwrap_or(0) & 8 == 0 {
            *barometric += 1;
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
        .user_agent("soundgeo/0.2 (historical flight research)")
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(30))
        .build()?;
    progress("Looking up the daily archive", 0.01);
    let dir = destination.join(date);
    let manifest = dir.join("archive-manifest.json");
    // A saved manifest pins the selected release and lets a complete, hash-checked
    // cache work offline. Missing/corrupt parts are fetched from that same release.
    let mut release: Option<Value> = if manifest.is_file() {
        if std::fs::metadata(&manifest)?.len() > 4_000_000 {
            bail!("Archive manifest exceeds 4 MB");
        }
        let value: Value = serde_json::from_reader(File::open(&manifest)?)?;
        let prefix = format!("v{}-planes-readsb-", day.format("%Y.%m.%d"));
        if !value["tag_name"]
            .as_str()
            .is_some_and(|s| s.starts_with(&prefix))
        {
            bail!("Cached archive manifest belongs to a different date");
        }
        Some(value)
    } else {
        None
    };
    for instance in ["prod-0", "prod-1", "staging-0"] {
        if release.is_some() {
            break;
        }
        control.check()?;
        let tag = format!("v{}-planes-readsb-{instance}", day.format("%Y.%m.%d"));
        let url = format!(
            "https://api.github.com/repos/adsblol/globe_history_{year}/releases/tags/{tag}"
        );
        control.require_network(&manifest.display().to_string())?;
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
        .filter(|v| {
            v["name"]
                .as_str()
                .is_some_and(|n| n.ends_with(".tar") || n.contains(".tar."))
        })
        .collect();
    assets.sort_by_key(|v| v["name"].as_str().unwrap_or(""));
    if assets.is_empty() {
        bail!("No archive files in this release.");
    }
    let total: u64 = assets.iter().map(|v| v["size"].as_u64().unwrap_or(0)).sum();
    std::fs::create_dir_all(&dir)?;
    let (mut manifest_tmp, destination) = crate::atomic_file::prepare(&manifest)?;
    serde_json::to_writer_pretty(manifest_tmp.as_file_mut(), &release)?;
    manifest_tmp.persist(destination)?;
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
        let url = asset["browser_download_url"]
            .as_str()
            .context("Missing download URL")?;
        if !url.starts_with("https://github.com/adsblol/") {
            bail!("Unexpected archive host");
        }
        crate::network::fetch_file(
            url,
            &path,
            size,
            asset["digest"].as_str(),
            control,
            &|message, p| {
                progress(
                    message,
                    (completed as f32 + p * size as f32) / total.max(1) as f32,
                )
            },
        )?;
        completed += size;
    }
    let license = "Source: https://www.adsb.lol/docs/open-data/historical\nDaily archive: Open Database License 1.0 https://opendatacommons.org/licenses/odbl/1-0/\nKeep attribution and check share-alike requirements when redistributing derived databases.\n";
    std::fs::write(dir.join("DATA-LICENSE.txt"), license)?;
    std::fs::write(
        dir.join("archive-manifest.json"),
        serde_json::to_vec_pretty(&release)?,
    )?;
    control.check()?;
    let catalog = dir.join("aircraft-types.json");
    if !catalog.is_file() {
        control.require_network(&catalog.display().to_string())?;
        crate::aircraft::download(&catalog).context("Flight archive is saved, but aircraft catalog download failed; retry or import the archive directly")?;
    }
    first.context("No archive parts")
}

#[derive(Debug)]
pub(crate) struct EmptyFlightData(usize);
impl std::fmt::Display for EmptyFlightData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "No usable flight segments after type/region/time filtering. Each needs at least {} valid airborne positions, at most 60 seconds apart.",
            self.0
        )
    }
}
impl std::error::Error for EmptyFlightData {}
