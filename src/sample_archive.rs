//! The provider's documented free first-of-month evaluation sample.
//! Public demo access: https://www.adsbexchange.com/pull-data/
//! These credentials are published by the provider for this sample bucket.
use crate::{Control, Progress, geo::Bounds, network::cancelable};
use anyhow::{Context, Result, ensure};
use chrono::{Datelike, NaiveDate, Utc};
use flate2::read::GzDecoder;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const HOST: &str = "6ff2cd7dae70306649e2c1e1500e2e0a.r2.cloudflarestorage.com";
const BUCKET: &str = "adsbx-sample-data";
// Provider-published sample credentials.
const ACCESS: &str = "7b8d31d9d2f2d73ffb2208614db599fa";
const SECRET: &str = "8bd955340e355418a5204e52a055005b6ca761f896abe0cccdd741638e790a76";
const MAX_COMPRESSED: usize = 20_000_000;
const MAX_JSON: u64 = 128_000_000;
const SNAPSHOT_INTERVAL_S: f64 = 300.;
const TRACE_DAY_S: f64 = 86400.;

#[path = "sample_recovery.rs"]
mod recovery;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceResolution {
    #[default]
    Standard,
    High,
}

impl TraceResolution {
    pub(crate) fn object_prefix(self) -> &'static str {
        match self {
            Self::Standard => "traces",
            Self::High => "hires-traces",
        }
    }

    pub(crate) fn cache_prefix(self) -> &'static str {
        match self {
            Self::Standard => "adsbx-sample",
            Self::High => "adsbx-hires-sample",
        }
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hmac(key: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(bytes);
    mac.finalize().into_bytes().to_vec()
}
fn authorization(
    host: &str,
    path: &str,
    query: &str,
    stamp: &str,
    access: &str,
    secret: &str,
) -> String {
    let empty = hash(b"");
    let signed = "host;x-amz-content-sha256;x-amz-date";
    let headers = format!("host:{host}\nx-amz-content-sha256:{empty}\nx-amz-date:{stamp}\n");
    let canonical = format!("GET\n{path}\n{query}\n{headers}\n{signed}\n{empty}");
    let day = &stamp[..8];
    let scope = format!("{day}/auto/s3/aws4_request");
    let string = format!(
        "AWS4-HMAC-SHA256\n{stamp}\n{scope}\n{}",
        hash(canonical.as_bytes())
    );
    let key = hmac(format!("AWS4{secret}").as_bytes(), day.as_bytes());
    let key = hmac(&hmac(&hmac(&key, b"auto"), b"s3"), b"aws4_request");
    let signature: String = hmac(&key, string.as_bytes())
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect();
    format!(
        "AWS4-HMAC-SHA256 Credential={access}/{scope}, SignedHeaders={signed}, Signature={signature}"
    )
}

pub fn validate(date: &str, bounds: Option<Bounds>) -> Result<Bounds> {
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    ensure!(
        day.day() == 1,
        "The free ADS-B Exchange sample covers only the first day of each month; choose a first-of-month date or another free source"
    );
    ensure!(
        day >= NaiveDate::from_ymd_opt(2016, 1, 1).unwrap() && day < Utc::now().date_naive(),
        "Choose a sample date from 2016 through yesterday UTC"
    );
    validate_bounds(bounds)
}
pub(crate) fn validate_bounds(bounds: Option<Bounds>) -> Result<Bounds> {
    let bounds = bounds.context("The free sample download needs search-area bounds")?;
    bounds.validate()?;
    Ok(bounds)
}

fn ident(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let hex = lower.strip_prefix('~').unwrap_or(&lower);
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(lower)
}
fn parse(bytes: &[u8]) -> Result<Value> {
    ensure!(
        bytes.len() <= MAX_COMPRESSED,
        "Sample object exceeds the download limit"
    );
    let mut decoded = Vec::new();
    GzDecoder::new(bytes)
        .take(MAX_JSON + 1)
        .read_to_end(&mut decoded)?;
    ensure!(
        decoded.len() as u64 <= MAX_JSON,
        "Sample object exceeds the decoded size limit"
    );
    Ok(serde_json::from_slice(&decoded)?)
}
fn snapshot(value: &Value, expected: f64, bounds: Bounds) -> Result<BTreeSet<String>> {
    let now = value["now"]
        .as_f64()
        .context("Sample snapshot lacks its time")?;
    // Discovery snapshots can lag their filename by more than a minute.
    // Keep the offset within one discovery interval. Snapshots select aircraft
    // IDs; full traces retain their own timestamps for location analysis.
    ensure!(
        (now - expected).abs() < SNAPSHOT_INTERVAL_S,
        "Sample snapshot timestamp {now} is outside one five-minute discovery interval around {expected}"
    );
    let aircraft = value["aircraft"]
        .as_array()
        .context("Sample snapshot lacks aircraft")?;
    let mut result = BTreeSet::new();
    for aircraft in aircraft {
        let (Some(lat), Some(lon), Some(id)) = (
            aircraft["lat"].as_f64(),
            aircraft["lon"].as_f64(),
            aircraft["hex"].as_str().and_then(ident),
        ) else {
            continue;
        };
        if bounds.contains(crate::geo::Point { lat, lon, alt: 0. }) {
            result.insert(id);
        }
    }
    Ok(result)
}
fn trace(value: &Value, expected: f64, id: &str) -> Result<(usize, usize)> {
    ensure!(
        value["icao"]
            .as_str()
            .or_else(|| value["hex"].as_str())
            .and_then(ident)
            .as_deref()
            == Some(id),
        "Sample trace has the wrong aircraft identity"
    );
    let timestamp = value["timestamp"]
        .as_f64()
        .context("Sample trace lacks its date")?;
    ensure!(
        (expected - TRACE_DAY_S..expected + TRACE_DAY_S).contains(&timestamp),
        "Sample trace has the wrong date"
    );
    let positions = value["trace"]
        .as_array()
        .context("Sample trace lacks positions")?;
    // A source trace can continue a flight across either midnight boundary.
    // Preserve adjacent-day positions; analysis filters to the selected dates.
    ensure!(
        positions
            .iter()
            .all(|row| row[0].as_f64().is_some_and(|offset| {
                (expected - TRACE_DAY_S..expected + 2. * TRACE_DAY_S)
                    .contains(&(timestamp + offset))
            })),
        "Sample trace has an invalid observation time or a position outside the requested and adjacent UTC days"
    );
    ensure!(
        (positions.is_empty() && timestamp >= expected)
            || positions.iter().any(|row| {
                (expected..expected + TRACE_DAY_S).contains(&(timestamp + row[0].as_f64().unwrap()))
            }),
        "Sample trace has no positions on the requested UTC day"
    );
    Ok((
        positions
            .iter()
            .filter(|row| timestamp + row[0].as_f64().unwrap() < expected)
            .count(),
        positions
            .iter()
            .filter(|row| timestamp + row[0].as_f64().unwrap() >= expected + TRACE_DAY_S)
            .count(),
    ))
}

pub(crate) struct Client {
    http: reqwest::Client,
    endpoint: String,
}
impl Client {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(60))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("soundgeo/0.3 (free sample evaluation)")
                .build()?,
            endpoint: format!("https://{HOST}"),
        })
    }
    async fn get(&self, key: &str, control: &Control) -> Result<Option<Vec<u8>>> {
        self.get_query(key, "", control).await
    }
    pub(crate) async fn listing(
        &self,
        prefix: &str,
        continuation: Option<&str>,
        control: &Control,
    ) -> Result<(String, Vec<u8>)> {
        let mut fields = vec![
            ("delimiter", "/"),
            ("list-type", "2"),
            ("max-keys", "1000"),
            ("prefix", prefix),
        ];
        if let Some(token) = continuation {
            fields.push(("continuation-token", token));
        }
        let query = crate::availability::query(&fields);
        let url = format!("{}/{BUCKET}/?{query}", self.endpoint);
        let bytes = self
            .get_query("", &query, control)
            .await?
            .context("The free sample date index is unavailable")?;
        Ok((url, bytes))
    }
    async fn get_query(
        &self,
        key: &str,
        query: &str,
        control: &Control,
    ) -> Result<Option<Vec<u8>>> {
        control.require_network(key)?;
        let path = format!("/{BUCKET}/{key}");
        // Keys contain only validated dates, fixed names, hex digits and '~'.
        ensure!(
            path.bytes()
                .all(|v| v.is_ascii_alphanumeric() || b"/-._~".contains(&v)),
            "Invalid sample object key"
        );
        let host = self
            .endpoint
            .split_once("://")
            .context("Invalid sample endpoint")?
            .1;
        let mut last = None;
        for attempt in 0..3 {
            control.check()?;
            let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let suffix = if query.is_empty() {
                String::new()
            } else {
                format!("?{query}")
            };
            let request = self
                .http
                .get(format!("{}{path}{suffix}", self.endpoint))
                .header("x-amz-content-sha256", hash(b""))
                .header("x-amz-date", &stamp)
                .header(
                    "authorization",
                    authorization(host, &path, query, &stamp, ACCESS, SECRET),
                );
            let result = async {
                let mut response = cancelable(request.send(), control).await??;
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                response = response.error_for_status()?;
                ensure!(
                    response
                        .content_length()
                        .is_none_or(|n| n <= MAX_COMPRESSED as u64),
                    "Sample object exceeds the download limit"
                );
                let mut bytes = Vec::new();
                while let Some(chunk) = cancelable(response.chunk(), control).await?? {
                    ensure!(
                        bytes.len().saturating_add(chunk.len()) <= MAX_COMPRESSED,
                        "Sample object exceeds the download limit"
                    );
                    bytes.extend_from_slice(&chunk);
                }
                Ok::<_, anyhow::Error>(Some(bytes))
            }
            .await;
            match result {
                Ok(bytes) => return Ok(bytes),
                Err(error) => {
                    control.check()?;
                    if let Some(error) = error.downcast_ref::<reqwest::Error>()
                        && error.status().is_some_and(|s| {
                            s.is_client_error() && s != reqwest::StatusCode::TOO_MANY_REQUESTS
                        })
                    {
                        return Err(anyhow::anyhow!(
                            "Free sample request was refused ({:?}); no paid bucket or alternate account was used",
                            error.status()
                        ));
                    }
                    last = Some(error);
                }
            }
            if attempt < 2 {
                cancelable(
                    tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))),
                    control,
                )
                .await?;
            }
        }
        Err(last.context("Sample download failed")?)
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Object {
    file: String,
    sha256: String,
    bytes: usize,
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct SnapshotReplacement {
    requested_file: String,
    source_file: String,
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct SnapshotClockOffset {
    file: String,
    seconds: f64,
}
#[derive(Serialize, Deserialize)]
struct TraceCarryover {
    file: String,
    preceding_day_positions: usize,
    #[serde(default)]
    following_day_positions: usize,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    #[serde(default)]
    resolution: TraceResolution,
    date: String,
    bounds: Bounds,
    source: String,
    snapshots: Vec<Object>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    missing_snapshot_slots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    snapshot_replacements: Vec<SnapshotReplacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    snapshot_clock_offsets: Vec<SnapshotClockOffset>,
    aircraft: Vec<String>,
    traces: Vec<Object>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trace_carryover: Vec<TraceCarryover>,
    missing_aircraft: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    snapshot_recoveries: Vec<recovery::Recovered>,
}

fn snapshot_name(minute: usize) -> String {
    format!("{:02}{:02}00Z.json.gz", minute / 60, minute % 60)
}

fn snapshot_choices(index: usize) -> Vec<usize> {
    let minute = index * 5;
    let mut choices = vec![minute];
    if minute > 0 {
        choices.push(minute - 1);
    }
    if minute + 1 < 1440 {
        choices.push(minute + 1);
    }
    choices
}

fn snapshot_plan_valid(manifest: &Manifest, names: &[String]) -> bool {
    let missing: BTreeSet<_> = manifest.missing_snapshot_slots.iter().collect();
    if ![1, 2, 3, 4, 5].contains(&manifest.schema)
        || manifest.snapshots.is_empty()
        || (manifest.schema < 3 && !missing.is_empty())
        || missing.len() != manifest.missing_snapshot_slots.len()
        || missing.iter().any(|name| !names.contains(name))
        || manifest.snapshots.len() + missing.len() != names.len()
    {
        return false;
    }
    let mut replacements = Vec::new();
    let mut objects = manifest.snapshots.iter();
    for (index, requested) in names.iter().enumerate() {
        if missing.contains(requested) {
            continue;
        }
        let Some(object) = objects.next() else {
            return false;
        };
        if object.file == *requested {
            continue;
        }
        if manifest.schema < 2
            || !snapshot_choices(index)
                .into_iter()
                .any(|m| object.file == snapshot_name(m))
        {
            return false;
        }
        replacements.push(SnapshotReplacement {
            requested_file: requested.clone(),
            source_file: object.file.clone(),
        });
    }
    let offset_files: BTreeSet<_> = manifest
        .snapshot_clock_offsets
        .iter()
        .map(|offset| &offset.file)
        .collect();
    replacements == manifest.snapshot_replacements
        && offset_files.len() == manifest.snapshot_clock_offsets.len()
        && manifest.snapshot_clock_offsets.iter().all(|offset| {
            offset.seconds.abs() > 60.
                && offset.seconds.abs() < SNAPSHOT_INTERVAL_S
                && manifest.snapshots.iter().any(|o| o.file == offset.file)
        })
}
fn cached_aircraft_set_valid(manifest: &Manifest, allow_missing: bool) -> bool {
    if !allow_missing
        && (!manifest.missing_aircraft.is_empty() || !manifest.missing_snapshot_slots.is_empty())
    {
        return false;
    }
    let aircraft: BTreeSet<_> = manifest.aircraft.iter().cloned().collect();
    let missing: BTreeSet<_> = manifest.missing_aircraft.iter().cloned().collect();
    let traced: Option<BTreeSet<String>> = manifest
        .traces
        .iter()
        .map(|object| {
            object
                .file
                .strip_prefix("trace_full_")
                .and_then(|s| s.strip_suffix(".json.gz"))
                .map(String::from)
        })
        .collect();
    let Some(traced) = traced else {
        return false;
    };
    aircraft.len() == manifest.aircraft.len()
        && missing.len() == manifest.missing_aircraft.len()
        && traced.len() == manifest.traces.len()
        && aircraft.iter().all(|id| ident(id).as_deref() == Some(id))
        && traced.is_disjoint(&missing)
        && traced.union(&missing).cloned().collect::<BTreeSet<_>>() == aircraft
        && manifest.trace_carryover.iter().all(|carryover| {
            (carryover.preceding_day_positions > 0 || carryover.following_day_positions > 0)
                && manifest.traces.iter().any(|o| o.file == carryover.file)
        })
}

fn valid_object(path: &Path, object: &Object, control: &Control) -> bool {
    if control.check().is_err()
        || object.bytes > MAX_COMPRESSED
        || !path
            .metadata()
            .is_ok_and(|m| m.len() == object.bytes as u64)
    {
        return false;
    }
    std::fs::read(path).is_ok_and(|bytes| hash(&bytes) == object.sha256)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Cache file has no parent")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
async fn object<F: Fn(&Value) -> Result<()>>(
    client: &Client,
    key: &str,
    path: &Path,
    rejected: &Path,
    control: &Control,
    check: F,
) -> Result<Option<(Object, Value)>> {
    let stamp_path = path.with_file_name(format!(
        "receipt-{}.json",
        path.file_name()
            .context("Missing object name")?
            .to_string_lossy()
    ));
    if let Ok(raw) = std::fs::read(&stamp_path)
        && let Ok(stamp) = serde_json::from_slice::<Object>(&raw)
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy() == stamp.file)
        && valid_object(path, &stamp, control)
        && let Ok(value) = parse(&std::fs::read(path)?)
        && check(&value).is_ok()
    {
        return Ok(Some((stamp, value)));
    }
    let Some(bytes) = client.get(key, control).await? else {
        return Ok(None);
    };
    let value = parse(&bytes).with_context(|| format!("Invalid free sample object {key}"))?;
    check(&value).with_context(|| format!("Invalid free sample object {key}"))?;
    if path.exists() {
        let mut old = std::fs::File::open(path)?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            control.check()?;
            let count = old.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        let old_hash = format!("{:x}", digest.finalize());
        if old_hash != hash(&bytes) {
            // Preserve rejected cache bytes outside the imported trace directory.
            std::fs::create_dir_all(rejected)?;
            let name = format!(
                "{}-{}",
                path.file_name().unwrap().to_string_lossy(),
                old_hash
            );
            let backup = rejected.join(name);
            if !backup.exists() {
                let mut input = std::fs::File::open(path)?;
                let mut temporary = tempfile::NamedTempFile::new_in(rejected)?;
                loop {
                    control.check()?;
                    let count = input.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    temporary.write_all(&buffer[..count])?;
                }
                temporary.as_file().sync_all()?;
                temporary.persist(backup)?;
            }
        }
    }
    let record = Object {
        file: path
            .file_name()
            .context("Missing cache file name")?
            .to_string_lossy()
            .into_owned(),
        sha256: hash(&bytes),
        bytes: bytes.len(),
    };
    control.check()?;
    atomic_write(path, &bytes)?;
    atomic_write(&stamp_path, &serde_json::to_vec(&record)?)?;
    Ok(Some((record, value)))
}

pub fn download_day(
    date: &str,
    cache: &Path,
    bounds: Option<Bounds>,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    download_day_with_resolution(
        date,
        cache,
        bounds,
        TraceResolution::Standard,
        control,
        progress,
    )
}

pub fn download_day_with_resolution(
    date: &str,
    cache: &Path,
    bounds: Option<Bounds>,
    resolution: TraceResolution,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    control.check()?;
    let bounds = validate(date, bounds)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(download(
        resolution,
        &Client::new()?,
        date,
        cache,
        bounds,
        control,
        progress,
    ))
}
async fn download(
    resolution: TraceResolution,
    client: &Client,
    date: &str,
    cache: &Path,
    bounds: Bounds,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    ensure!(
        !control.offline || !control.refresh_source_gaps,
        "Source gaps cannot be refreshed in offline mode"
    );
    let tag = hash(&serde_json::to_vec(&bounds)?);
    let root = cache.join(format!(
        "{}-{date}-{}",
        resolution.cache_prefix(),
        &tag[..16]
    ));
    let shared = cache.join(format!("adsbx-sample-{date}-snapshots"));
    let traces = root.join("traces");
    let manifest_path = root.join("sample-manifest.json");
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64;
    let names: Vec<_> = (0..288).map(|i| snapshot_name(i * 5)).collect();
    if let Ok(bytes) = std::fs::read(&manifest_path)
        && let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes)
        && manifest.date == date
        && manifest.resolution == resolution
        && serde_json::to_vec(&manifest.bounds)? == serde_json::to_vec(&bounds)?
        && snapshot_plan_valid(&manifest, &names)
        && cached_aircraft_set_valid(&manifest, !control.refresh_source_gaps)
        && recovery::cache_valid(&manifest, &traces, &shared, control)
        && manifest
            .snapshots
            .iter()
            .all(|o| valid_object(&shared.join(&o.file), o, control))
        && manifest.traces.iter().all(|o| {
            let id = o
                .file
                .strip_prefix("trace_full_")
                .and_then(|n| n.strip_suffix(".json.gz"));
            id.is_some_and(|id| {
                ident(id).as_deref() == Some(id)
                    && valid_object(&traces.join(&o.file), o, control)
                    && std::fs::read(traces.join(&o.file)).is_ok_and(|bytes| {
                        parse(&bytes).is_ok_and(|value| trace(&value, start, id).is_ok())
                    })
            })
        })
    {
        control.check()?;
        progress("Using verified full-day free sample cache", 1.);
        return Ok(traces);
    }
    let prefix = date.replace('-', "/");
    let mut manifest = Manifest {
        schema: 5,
        resolution,
        date: date.into(),
        bounds,
        source: "https://www.adsbexchange.com/data-products/sample-data/".into(),
        snapshots: vec![],
        missing_snapshot_slots: vec![],
        snapshot_replacements: vec![],
        snapshot_clock_offsets: vec![],
        aircraft: vec![],
        traces: vec![],
        trace_carryover: vec![],
        missing_aircraft: vec![],
        snapshot_recoveries: vec![],
    };
    let mut ids = BTreeSet::new();
    let mut recovery_plan = recovery::Plan::new();
    for (index, name) in names.iter().enumerate() {
        control.check()?;
        let mut selected = None;
        for minute in snapshot_choices(index) {
            let source_name = snapshot_name(minute);
            let expected = start + minute as f64 * 60.;
            let result = object(
                client,
                &format!("readsb-hist/{prefix}/{source_name}"),
                &shared.join(&source_name),
                &shared.join("rejected"),
                control,
                |v| snapshot(v, expected, bounds).map(|_| ()),
            )
            .await?;
            if let Some((record, value)) = result {
                selected = Some((record, value, expected));
                break;
            }
        }
        let Some((record, value, expected)) = selected else {
            // Only confirmed HTTP 404 responses reach this branch. Offline
            // cache loss, malformed data, refusal and transport errors stop.
            manifest.missing_snapshot_slots.push(name.clone());
            progress(
                &format!(
                    "Free sample source gap: {name} and neighboring minutes are unavailable; recorded in the manifest"
                ),
                0.7 * (index + 1) as f32 / 288.,
            );
            continue;
        };
        ids.extend(snapshot(&value, expected, bounds)?);
        recovery::discover(&mut recovery_plan, &value, start, bounds);
        ensure!(
            ids.len() <= 10000,
            "More than 10,000 aircraft were found; narrow the sample search area"
        );
        if record.file != *name {
            manifest.snapshot_replacements.push(SnapshotReplacement {
                requested_file: name.clone(),
                source_file: record.file.clone(),
            });
        }
        let offset = value["now"].as_f64().context("Missing snapshot time")? - expected;
        if offset.abs() > 60. {
            manifest.snapshot_clock_offsets.push(SnapshotClockOffset {
                file: record.file.clone(),
                seconds: offset,
            });
        }
        manifest.snapshots.push(record);
        progress(
            &format!(
                "Full-day free sample: {}/288 slots checked, {} source gaps, {} aircraft",
                index + 1,
                manifest.missing_snapshot_slots.len(),
                ids.len()
            ),
            0.7 * (index + 1) as f32 / 288.,
        );
    }
    ensure!(
        !manifest.snapshots.is_empty(),
        "No discovery snapshots are available for {date}; no usable daily cache was created"
    );
    std::fs::create_dir_all(&traces)?;
    manifest.aircraft = ids.iter().cloned().collect();
    for (index, id) in ids.iter().enumerate() {
        control.check()?;
        let file = format!("trace_full_{id}.json.gz");
        let key = format!(
            "{}/{prefix}/{}/trace_full_{id}.json",
            resolution.object_prefix(),
            &id[id.len() - 2..]
        );
        if let Some((record, value)) = object(
            client,
            &key,
            &traces.join(file),
            &root.join("rejected"),
            control,
            |v| trace(v, start, id).map(|_| ()),
        )
        .await?
        {
            let (preceding_day_positions, following_day_positions) = trace(&value, start, id)?;
            if preceding_day_positions > 0 || following_day_positions > 0 {
                manifest.trace_carryover.push(TraceCarryover {
                    file: record.file.clone(),
                    preceding_day_positions,
                    following_day_positions,
                });
            }
            manifest.traces.push(record);
        } else {
            manifest.missing_aircraft.push(id.clone());
        }
        progress(
            &format!(
                "Full-day free traces: {}/{}; {} unavailable",
                index + 1,
                ids.len(),
                manifest.missing_aircraft.len()
            ),
            0.7 + 0.2 * (index + 1) as f32 / ids.len().max(1) as f32,
        );
    }
    recovery::recover(
        client,
        &mut manifest,
        recovery_plan,
        start,
        &root,
        control,
        progress,
    )
    .await?;
    control.check()?;
    atomic_write(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
    atomic_write(&root.join("SOURCE.txt"), b"ADS-B Exchange free first-of-month testing/evaluation sample. https://www.adsbexchange.com/data-products/sample-data/ . All 288 discovery slots were checked. A missing snapshot can use an available neighboring minute from the same day. Actual source files, replacement mappings, and slots unavailable at the scheduled and checked neighboring filenames are recorded in sample-manifest.json. Full-day traces were downloaded for regional identities. Missing traces with an airborne discovery position are recovered from five-second snapshots within five minutes of that position. Derived traces, original snapshot objects, attempted slots and source gaps are recorded separately. Flights entirely between discovery snapshots can be absent. Missing full-day traces remain listed in the manifest. Reference data is not bundled or redistributed by SoundGeo.\n")?;
    progress(
        "Free sample download complete; coverage limits are in the manifest",
        1.,
    );
    Ok(traces)
}

pub fn coverage_warning(path: &Path) -> Result<String> {
    Ok(coverage(path)?.0)
}

// Called after archive preparation has verified the manifest and source files.
pub(crate) fn coverage(path: &Path) -> Result<(String, bool)> {
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(
        path.parent()
            .context("Missing sample cache directory")?
            .join("sample-manifest.json"),
    )?)?;
    let replacements = if manifest.snapshot_replacements.is_empty() {
        String::new()
    } else {
        format!(
            " Missing snapshot slots filled from neighboring minutes: {}. The manifest records each requested and actual source filename.",
            manifest.snapshot_replacements.len()
        )
    };
    let gaps = if manifest.missing_snapshot_slots.is_empty() {
        String::new()
    } else {
        format!(
            " Unavailable discovery slots: {}. Each slot and its neighboring minutes returned HTTP 404. The manifest lists these source gaps; flights during these gaps may be absent.",
            manifest.missing_snapshot_slots.len()
        )
    };
    let offsets = if manifest.snapshot_clock_offsets.is_empty() {
        String::new()
    } else {
        let maximum = manifest
            .snapshot_clock_offsets
            .iter()
            .map(|offset| offset.seconds.abs())
            .fold(0., f64::max);
        format!(
            " Discovery snapshots with offsets over one minute: {} (maximum {maximum:.1} seconds). The manifest records these offsets. Flight-trace times are unchanged.",
            manifest.snapshot_clock_offsets.len()
        )
    };
    let carryover = if manifest.trace_carryover.is_empty() {
        String::new()
    } else {
        let preceding: usize = manifest
            .trace_carryover
            .iter()
            .map(|v| v.preceding_day_positions)
            .sum();
        let following: usize = manifest
            .trace_carryover
            .iter()
            .map(|v| v.following_day_positions)
            .sum();
        let mut spans = Vec::new();
        if preceding > 0 {
            spans.push(format!("{preceding} positions from the preceding UTC day"));
        }
        if following > 0 {
            spans.push(format!("{following} positions from the following UTC day"));
        }
        format!(
            " {} trace files also contain {}. Original files are preserved; analysis uses positions within the selected archive dates.",
            manifest.trace_carryover.len(),
            spans.join(" and ")
        )
    };
    let recovered = manifest
        .snapshot_recoveries
        .iter()
        .filter(|record| record.trace.is_some())
        .count();
    let recovery = if manifest.snapshot_recoveries.is_empty() {
        String::new()
    } else {
        format!(
            " Snapshot recovery produced {recovered} short traces for {} missing full-day traces. Each recovery covers up to five minutes before and after recorded airborne discovery positions. Source snapshots, attempted windows and gaps are retained in the manifest.",
            manifest.snapshot_recoveries.len()
        )
    };
    Ok((
        format!(
            "ADS-B Exchange free evaluation sample for {}: {} full-day traces, {} unavailable. Discovery inspected {} snapshots across 288 five-minute slots in the search area.{}{}{}{}{} Flights between discovery snapshots and flights outside receiver coverage can be absent. The sample is available only on the first day of each month.",
            manifest.date,
            manifest.traces.len(),
            manifest.missing_aircraft.len(),
            manifest.snapshots.len(),
            replacements,
            gaps,
            offsets,
            carryover,
            recovery
        ),
        manifest.traces.is_empty() && recovered == 0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    const DAY: &str = "2024-03-01";
    fn bounds() -> Bounds {
        Bounds {
            south: 10.,
            north: 11.,
            west: 20.,
            east: 21.,
        }
    }
    fn start() -> f64 {
        NaiveDate::parse_from_str(DAY, "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp() as f64
    }
    fn compressed(value: Value) -> Vec<u8> {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(&serde_json::to_vec(&value).unwrap())
            .unwrap();
        gzip.finish().unwrap()
    }
    struct Server {
        endpoint: String,
        stop: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Server {
        fn new(response: impl Fn(&str) -> (u16, Vec<u8>, u64) + Send + 'static) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let stop = Arc::new(AtomicBool::new(false));
            let calls = Arc::new(AtomicUsize::new(0));
            let stopped = stop.clone();
            let counter = calls.clone();
            let thread = std::thread::spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    let (mut connection, _) = match listener.accept() {
                        Ok(value) => value,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        Err(error) => panic!("{error}"),
                    };
                    connection.set_nonblocking(false).unwrap();
                    connection
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut byte = [0; 1];
                    while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                        if connection.read(&mut byte).unwrap_or(0) == 0 {
                            break;
                        }
                        request.push(byte[0]);
                    }
                    let request = String::from_utf8(request).unwrap();
                    assert!(
                        request
                            .to_lowercase()
                            .contains("authorization: aws4-hmac-sha256")
                    );
                    let key = request.split_whitespace().nth(1).unwrap();
                    assert!(key.starts_with("/adsbx-sample-data/"));
                    counter.fetch_add(1, Ordering::Relaxed);
                    let (status, body, delay) = response(key);
                    let header = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = connection.write_all(header.as_bytes());
                    std::thread::sleep(Duration::from_millis(delay));
                    let _ = connection.write_all(&body);
                }
            });
            Self {
                endpoint,
                stop,
                calls,
                thread: Some(thread),
            }
        }
        fn client(&self) -> Client {
            let mut client = Client::new().unwrap();
            client.endpoint = self.endpoint.clone();
            client
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let joined = self.thread.take().unwrap().join();
            if !std::thread::panicking() {
                joined.unwrap();
            }
        }
    }
    fn fixture(key: &str) -> (u16, Vec<u8>, u64) {
        let name = key.rsplit('/').next().unwrap();
        let value = if key.contains("/readsb-hist/") {
            let hour: u32 = name[..2].parse().unwrap();
            let minute: u32 = name[2..4].parse().unwrap();
            json!({"now": start() + (hour * 3600 + minute * 60) as f64 - 1., "aircraft": [
                {"hex":"ABCDEF", "lat":10.4,"lon":20.4}, {"hex":"~ABC123", "lat":10.5,"lon":20.5},
                {"hex":"c0ffee", "lat":13.,"lon":25.}, {"hex":"../../", "lat":10.4,"lon":20.4}
            ]})
        } else {
            let id = name
                .strip_prefix("trace_full_")
                .unwrap()
                .strip_suffix(".json")
                .unwrap();
            let points: Vec<_> = (0..10)
                .map(|i| {
                    json!([
                        10. + i as f64 * 3.,
                        10.4,
                        20.4 + i as f64 * 0.001,
                        1000,
                        100,
                        90,
                        0
                    ])
                })
                .collect();
            json!({"icao":id,"timestamp":start(),"trace":points})
        };
        (200, compressed(value), 0)
    }
    async fn download(
        client: &Client,
        date: &str,
        cache: &Path,
        bounds: Bounds,
        control: &Control,
        progress: &Progress<'_>,
    ) -> Result<PathBuf> {
        super::download(
            TraceResolution::Standard,
            client,
            date,
            cache,
            bounds,
            control,
            progress,
        )
        .await
    }

    fn run(client: &Client, cache: &Path) -> Result<PathBuf> {
        run_controlled(client, cache, &Control::default())
    }

    fn run_controlled(client: &Client, cache: &Path, control: &Control) -> Result<PathBuf> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(download(client, DAY, cache, bounds(), control, &|_, _| {}))
    }

    #[test]
    fn signature_matches_independent_botocore_s3_fixture() {
        // Generated with botocore S3SigV4Auth, dummy credentials and fixed UTC.
        let actual = authorization(
            "example.com",
            "/adsbx-sample-data/traces/2024/03/01/23/trace_full_~abc123.json",
            "",
            "20240302T123456Z",
            "TESTACCESS",
            "test-secret",
        );
        assert_eq!(
            actual,
            "AWS4-HMAC-SHA256 Credential=TESTACCESS/20240302/auto/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=32c649f57975f213601dfdcf921e66a0d9bef4f8503eb158894bd0157a86fca5"
        );
    }
    #[test]
    fn validates_free_dates_bounds_identity_and_source_dates() {
        assert!(validate(DAY, Some(bounds())).is_ok());
        assert!(validate("2024-03-02", Some(bounds())).is_err());
        assert!(validate("2099-03-01", Some(bounds())).is_err());
        assert!(validate(DAY, None).is_err());
        assert!(
            validate(
                DAY,
                Some(Bounds {
                    south: f64::NAN,
                    ..bounds()
                })
            )
            .is_err()
        );
        assert_eq!(ident("~ABC123").as_deref(), Some("~abc123"));
        for value in ["../../a", "~~abc123", "g12345", "abcde"] {
            assert!(ident(value).is_none());
        }
        assert!(
            snapshot(
                &json!({"now":start()+86400.,"aircraft":[]}),
                start(),
                bounds()
            )
            .is_err()
        );
        assert!(
            trace(
                &json!({"icao":"abcdef","timestamp":start(),"trace":[]}),
                start(),
                "~abc123"
            )
            .is_err()
        );
        assert!(
            trace(
                &json!({"icao":"abcdef","timestamp":start()+86400.,"trace":[]}),
                start(),
                "abcdef"
            )
            .is_err()
        );
    }
    #[test]
    fn historical_snapshot_clock_jitter_keeps_trace_times_unchanged() {
        for offset in [-299.9, -60., -1., 30.2, 31.9, 60., 69.2, 299.9] {
            let value = json!({"now":start()+offset,"aircraft":[
                {"hex":"abcdef","lat":10.5,"lon":20.5}
            ]});
            assert_eq!(
                snapshot(&value, start(), bounds()).unwrap(),
                BTreeSet::from(["abcdef".into()])
            );
            assert_eq!(value["now"].as_f64().unwrap(), start() + offset);
        }
        for offset in [-86400., -300., 300., 86400.] {
            assert!(
                snapshot(
                    &json!({"now":start()+offset,"aircraft":[]}),
                    start(),
                    bounds()
                )
                .is_err()
            );
        }
        assert!(
            trace(
                &json!({"icao":"abcdef","timestamp":start()+86400.+30.2,"trace":[]}),
                start(),
                "abcdef"
            )
            .is_err()
        );
    }
    #[test]
    fn high_resolution_download_keeps_separate_traces_and_reuses_discovery_offline() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(|key| {
            let (status, bytes, delay) = fixture(key);
            if key.contains("/hires-traces/") {
                let mut value = parse(&bytes).unwrap();
                value["trace"] = json!(
                    (0..61)
                        .map(|i| json!([
                            10. + i as f64 * 0.5,
                            10.4,
                            20.4 + i as f64 / 6000.,
                            1000,
                            100,
                            90,
                            0
                        ]))
                        .collect::<Vec<_>>()
                );
                return (status, compressed(value), delay);
            }
            (status, bytes, delay)
        });
        let client = server.client();
        let standard = run(&client, dir.path()).unwrap();
        let original = std::fs::read(standard.join("trace_full_abcdef.json.gz")).unwrap();
        assert_eq!(server.calls.load(Ordering::Relaxed), 290);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let high = runtime
            .block_on(super::download(
                TraceResolution::High,
                &client,
                DAY,
                dir.path(),
                bounds(),
                &Control::default(),
                &|_, _| {},
            ))
            .unwrap();
        assert_ne!(standard, high);
        assert_eq!(server.calls.load(Ordering::Relaxed), 292);
        assert_eq!(
            std::fs::read(standard.join("trace_full_abcdef.json.gz")).unwrap(),
            original
        );
        let dense = std::fs::read(high.join("trace_full_abcdef.json.gz")).unwrap();
        assert_eq!(
            parse(&dense).unwrap()["trace"].as_array().unwrap().len(),
            61
        );
        assert_ne!(dense, original);
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(high.parent().unwrap().join("sample-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.resolution, TraceResolution::High);
        let offline = Control::default().with_offline(true);
        let request = crate::archive::ArchiveRequest {
            date_from: String::new(),
            date_to: None,
            cache: dir.path().into(),
            provider: crate::archive::ArchiveProvider::AdsbExchangeSampleHires,
        };
        assert_eq!(request.dates().unwrap(), [DAY]);
        assert_eq!(
            request
                .ensure_for_bounds(Some(bounds()), &offline, &|_, _| {})
                .unwrap(),
            std::slice::from_ref(&high)
        );
        assert_eq!(server.calls.load(Ordering::Relaxed), 292);

        // A missing high-resolution object must not use the standard trace.
        std::fs::remove_file(high.join("trace_full_abcdef.json.gz")).unwrap();
        assert!(
            request
                .ensure_for_bounds(Some(bounds()), &offline, &|_, _| {})
                .is_err()
        );
        assert_eq!(server.calls.load(Ordering::Relaxed), 292);
        assert_eq!(
            runtime
                .block_on(super::download(
                    TraceResolution::High,
                    &client,
                    DAY,
                    dir.path(),
                    bounds(),
                    &Control::default(),
                    &|_, _| {},
                ))
                .unwrap(),
            high
        );
        assert_eq!(server.calls.load(Ordering::Relaxed), 293);
        assert_eq!(
            std::fs::read(high.join("trace_full_abcdef.json.gz")).unwrap(),
            dense
        );
        assert_eq!(
            std::fs::read(standard.join("trace_full_abcdef.json.gz")).unwrap(),
            original
        );
    }

    #[test]
    fn full_day_download_preserves_non_icao_ids_and_reuses_offline_cache() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(fixture);
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        assert_eq!(server.calls.load(Ordering::Relaxed), 290);
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(path.parent().unwrap().join("sample-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.aircraft, ["abcdef", "~abc123"]);
        assert_eq!(manifest.snapshots.len(), 288);
        assert!(manifest.missing_aircraft.is_empty());
        assert!(path.join("trace_full_~abc123.json.gz").is_file());
        assert_eq!(
            crate::data::load(&path, &Control::default())
                .unwrap()
                .tracks
                .len(),
            2
        );
        drop(server);
        assert_eq!(run(&client, dir.path()).unwrap(), path);
    }
    #[test]
    fn delayed_discovery_snapshots_record_offsets_and_replay_offline() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(|key| {
            let (status, bytes, delay) = fixture(key);
            if key.ends_with("/000500Z.json.gz") {
                let mut value = parse(&bytes).unwrap();
                value["now"] = json!(start() + 300. + 69.2);
                return (status, compressed(value), delay);
            }
            (status, bytes, delay)
        });
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        let mut manifest: Manifest = serde_json::from_slice(
            &std::fs::read(path.parent().unwrap().join("sample-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.snapshot_clock_offsets.len(), 1);
        assert_eq!(manifest.snapshot_clock_offsets[0].file, "000500Z.json.gz");
        assert!((manifest.snapshot_clock_offsets[0].seconds - 69.2).abs() < 1e-6);
        let warning = coverage_warning(&path).unwrap();
        assert!(warning.contains("Discovery snapshots with offsets over one minute: 1"));
        assert!(warning.contains("maximum 69.2 seconds"));
        let data = crate::data::load(&path, &Control::default()).unwrap();
        assert_eq!(data.tracks[0].samples[0].time, start() + 10.);
        assert_eq!(server.calls.load(Ordering::Relaxed), 290);
        let offline = Control {
            offline: true,
            ..Control::default()
        };
        let replay = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(download(
                &client,
                DAY,
                dir.path(),
                bounds(),
                &offline,
                &|_, _| {},
            ))
            .unwrap();
        assert_eq!(path, replay);
        assert_eq!(server.calls.load(Ordering::Relaxed), 290);
        let names: Vec<_> = (0..288).map(|i| snapshot_name(i * 5)).collect();
        manifest.snapshot_clock_offsets[0].seconds = SNAPSHOT_INTERVAL_S;
        assert!(!snapshot_plan_valid(&manifest, &names));
        manifest.snapshot_clock_offsets[0].seconds = 69.2;
        manifest.snapshot_clock_offsets[0].file = "missing.json.gz".into();
        assert!(!snapshot_plan_valid(&manifest, &names));
    }
    #[test]
    fn adjacent_day_carryover_requires_current_day_positions_and_stays_bounded() {
        let value = json!({"icao":"abcdef","timestamp":start()+2.608,
            "trace":[[0.], [86397.4]]});
        assert_eq!(trace(&value, start(), "abcdef").unwrap(), (0, 1));
        for rows in [
            json!([[86397.4]]),
            json!([[0.], [172798.4]]),
            json!([[0.], [null]]),
        ] {
            let mut invalid = value.clone();
            invalid["trace"] = rows;
            assert!(trace(&invalid, start(), "abcdef").is_err());
        }
        let legacy: TraceCarryover = serde_json::from_value(json!({
            "file":"trace_full_abcdef.json.gz", "preceding_day_positions":10
        }))
        .unwrap();
        assert_eq!(legacy.following_day_positions, 0);
    }

    #[test]
    fn fractional_midnight_position_is_preserved_and_filtered_for_analysis() {
        check_following_day_position(86400.008);
    }

    #[test]
    fn next_day_flight_continuation_is_preserved_and_filtered_for_analysis() {
        check_following_day_position(86930.384);
    }

    fn check_following_day_position(offset: f64) {
        let dir = tempfile::tempdir().unwrap();
        let with_boundary = move |key: &str| {
            let (status, bytes, delay) = fixture(key);
            if !key.contains("/traces/") {
                return (status, bytes, delay);
            }
            let mut value = parse(&bytes).unwrap();
            let mut last = value["trace"].as_array().unwrap().last().unwrap().clone();
            last[0] = json!(offset);
            value["trace"].as_array_mut().unwrap().push(last);
            (status, compressed(value), delay)
        };
        let expected = with_boundary("/traces/2024/03/01/ef/trace_full_abcdef.json").1;
        let server = Server::new(with_boundary);
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        let bytes = std::fs::read(path.join("trace_full_abcdef.json.gz")).unwrap();
        assert_eq!(bytes, expected);
        let value = parse(&bytes).unwrap();
        assert_eq!(value["trace"][10][0], json!(offset));
        let manifest_path = path.parent().unwrap().join("sample-manifest.json");
        let manifest_bytes = std::fs::read(&manifest_path).unwrap();
        let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
        let carryover = manifest["trace_carryover"].as_array().unwrap();
        assert_eq!(carryover.len(), 2);
        assert!(
            carryover.iter().all(|v| {
                v["preceding_day_positions"] == 0 && v["following_day_positions"] == 1
            })
        );
        assert!(
            coverage_warning(&path)
                .unwrap()
                .contains("2 positions from the following UTC day")
        );
        let job = crate::Job {
            tracks: path.clone(),
            archive: Some(crate::archive::ArchiveRequest {
                date_from: DAY.into(),
                date_to: None,
                cache: dir.path().to_owned(),
                provider: crate::archive::ArchiveProvider::AdsbExchangeSample,
            }),
            ..crate::Job::default()
        };
        let offline = Control {
            offline: true,
            ..Control::default()
        };
        for _ in 0..2 {
            let selected = crate::data::load_for_job(&job, 60., &offline).unwrap();
            assert_eq!(selected.tracks.len(), 2);
            assert!(selected.tracks.iter().all(|track| {
                track.samples.len() == 10
                    && track
                        .samples
                        .iter()
                        .all(|p| p.time >= start() && p.time < start() + 86400.)
            }));
        }
        let replay = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(download(
                &client,
                DAY,
                dir.path(),
                bounds(),
                &offline,
                &|_, _| {},
            ))
            .unwrap();
        assert_eq!(replay, path);
        assert_eq!(server.calls.load(Ordering::Relaxed), 290);
        assert_eq!(std::fs::read(manifest_path).unwrap(), manifest_bytes);
        assert_eq!(
            std::fs::read(path.join("trace_full_abcdef.json.gz")).unwrap(),
            expected
        );
    }

    #[test]
    fn leading_previous_day_positions_are_preserved_and_filtered_for_analysis() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(|key| {
            let (status, bytes, delay) = fixture(key);
            if !key.contains("/traces/") {
                return (status, bytes, delay);
            }
            let mut value = parse(&bytes).unwrap();
            value["timestamp"] = json!(start() - 3600.);
            let mut current = value["trace"].as_array().unwrap().clone();
            for row in &mut current {
                row[0] = json!(row[0].as_f64().unwrap() + 3600.);
            }
            value["trace"].as_array_mut().unwrap().extend(current);
            (status, compressed(value), delay)
        });
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        let manifest_bytes =
            std::fs::read(path.parent().unwrap().join("sample-manifest.json")).unwrap();
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(manifest.trace_carryover.len(), 2);
        assert!(
            manifest
                .trace_carryover
                .iter()
                .all(|v| v.preceding_day_positions == 10)
        );
        assert!(
            coverage_warning(&path)
                .unwrap()
                .contains("20 positions from the preceding UTC day")
        );
        let raw = crate::data::load(&path, &Control::default()).unwrap();
        assert_eq!(raw.tracks.len(), 4);
        assert!(
            raw.tracks
                .iter()
                .any(|t| t.samples[0].time == start() - 3590.)
        );
        let job = crate::Job {
            tracks: path.clone(),
            archive: Some(crate::archive::ArchiveRequest {
                date_from: DAY.into(),
                date_to: None,
                cache: dir.path().to_owned(),
                provider: crate::archive::ArchiveProvider::AdsbExchangeSample,
            }),
            ..crate::Job::default()
        };
        let offline = Control {
            offline: true,
            ..Control::default()
        };
        for _ in 0..2 {
            let selected = crate::data::load_for_job(&job, 60., &offline).unwrap();
            assert_eq!(selected.tracks.len(), 2);
            assert!(selected.tracks.iter().all(|t| {
                t.samples.len() == 10
                    && t.samples[0].time == start() + 10.
                    && t.samples[9].time == start() + 37.
            }));
        }
        let mut other_date = job.clone();
        other_date.archive.as_mut().unwrap().date_from = "2024-04-01".into();
        assert!(crate::data::load_for_job(&other_date, 60., &offline).is_err());
        assert_eq!(server.calls.load(Ordering::Relaxed), 290);
        for object in &manifest.traces {
            assert_eq!(
                hash(&std::fs::read(path.join(&object.file)).unwrap()),
                object.sha256
            );
        }
        assert_eq!(
            std::fs::read(path.parent().unwrap().join("sample-manifest.json")).unwrap(),
            manifest_bytes
        );
        assert!(
            trace(
                &json!({"hex":"abcdef","timestamp":start()-3600.,"trace":[[0.]]}),
                start(),
                "abcdef"
            )
            .is_err()
        );
    }
    #[test]
    fn cache_damage_is_repaired_and_original_bytes_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(fixture);
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        let trace = path.join("trace_full_abcdef.json.gz");
        std::fs::write(&trace, b"broken cache").unwrap();
        assert_eq!(run(&client, dir.path()).unwrap(), path);
        assert_eq!(server.calls.load(Ordering::Relaxed), 291);
        assert_eq!(
            parse(&std::fs::read(trace).unwrap()).unwrap()["icao"],
            "abcdef"
        );
        let files: Vec<_> = std::fs::read_dir(path.parent().unwrap().join("rejected"))
            .unwrap()
            .collect();
        assert_eq!(files.len(), 1);
        assert_eq!(
            std::fs::read(files[0].as_ref().unwrap().path()).unwrap(),
            b"broken cache"
        );
        assert_eq!(
            crate::data::load(&path, &Control::default())
                .unwrap()
                .tracks
                .len(),
            2
        );
    }
    #[test]
    fn legacy_trace_identity_and_base_preserve_absolute_observation_times() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(|key| {
            let (status, bytes, delay) = fixture(key);
            if !key.contains("/traces/") {
                return (status, bytes, delay);
            }
            let mut value = parse(&bytes).unwrap();
            value["hex"] = value.as_object_mut().unwrap().remove("icao").unwrap();
            value["timestamp"] = json!(start() + 3600.);
            (status, compressed(value), delay)
        });
        let path = run(&server.client(), dir.path()).unwrap();
        let dataset = crate::data::load(&path, &Control::default()).unwrap();
        assert_eq!(dataset.tracks.len(), 2);
        for track in &dataset.tracks {
            assert_eq!(track.samples[0].time, start() + 3610.);
        }
        let value = parse(&std::fs::read(path.join("trace_full_abcdef.json.gz")).unwrap()).unwrap();
        assert!(value.get("icao").is_none());
        assert_eq!(value["hex"], "abcdef");
        assert_eq!(value["timestamp"], start() + 3600.);
        for value in [
            json!({"icao":"000000","hex":"abcdef","timestamp":start(),"trace":[]}),
            json!({"hex":"abcdef","timestamp":start()+86400.,"trace":[]}),
            json!({"hex":"abcdef","timestamp":start()+3600.,"trace":[[-3601.]]}),
            json!({"hex":"abcdef","timestamp":start()+3600.,"trace":[[82800.]]}),
        ] {
            assert!(trace(&value, start(), "abcdef").is_err());
        }
    }

    #[test]
    fn offline_cache_rechecks_trace_dates_even_when_recorded_hashes_match() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(fixture);
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        let manifest_path = path.parent().unwrap().join("sample-manifest.json");
        let mut manifest: Manifest =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        // Earlier validators checked the base date but not observation offsets.
        let record = &mut manifest.traces[0];
        let trace_path = path.join(&record.file);
        let mut value = parse(&std::fs::read(&trace_path).unwrap()).unwrap();
        value["trace"][0][0] = json!(172800.);
        let bytes = compressed(value);
        record.bytes = bytes.len();
        record.sha256 = hash(&bytes);
        std::fs::write(&trace_path, &bytes).unwrap();
        std::fs::write(
            path.join(format!("receipt-{}.json", record.file)),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(valid_object(&trace_path, record, &Control::default()));
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let calls = server.calls.load(Ordering::Relaxed);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = runtime
            .block_on(download(
                &client,
                DAY,
                dir.path(),
                bounds(),
                &Control::default().with_offline(true),
                &|_, _| {},
            ))
            .unwrap_err();
        assert!(format!("{error:#}").contains("Offline mode"));
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
    }

    #[test]
    fn missing_snapshot_uses_recorded_neighbor_and_verifies_it_offline() {
        assert_eq!(snapshot_choices(0), [0, 1]);
        assert_eq!(snapshot_choices(287), [1435, 1434, 1436]);
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(|key| {
            if key.ends_with("/000500Z.json.gz") {
                (404, vec![], 0)
            } else {
                fixture(key)
            }
        });
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        let manifest_path = path.parent().unwrap().join("sample-manifest.json");
        let mut manifest: Manifest =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        assert_eq!(manifest.schema, 5);
        assert_eq!(manifest.snapshots.len(), 288);
        assert_eq!(manifest.snapshots[1].file, "000400Z.json.gz");
        assert_eq!(
            manifest.snapshot_replacements,
            [SnapshotReplacement {
                requested_file: "000500Z.json.gz".into(),
                source_file: "000400Z.json.gz".into(),
            }]
        );
        assert!(
            coverage_warning(&path)
                .unwrap()
                .contains("Missing snapshot slots filled from neighboring minutes: 1.")
        );
        let shared = dir.path().join(format!("adsbx-sample-{DAY}-snapshots"));
        assert!(!shared.join("000500Z.json.gz").exists());
        assert!(shared.join("000400Z.json.gz").exists());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let offline = Control::default().with_offline(true);
        let calls = server.calls.load(Ordering::Relaxed);
        let reuse = || {
            runtime.block_on(download(
                &client,
                DAY,
                dir.path(),
                bounds(),
                &offline,
                &|_, _| {},
            ))
        };
        assert_eq!(reuse().unwrap(), path);
        manifest.snapshot_replacements[0].source_file = "000600Z.json.gz".into();
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(format!("{:#}", reuse().unwrap_err()).contains("Offline mode"));
        manifest.snapshot_replacements[0].source_file = "000400Z.json.gz".into();
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        std::fs::write(shared.join("000400Z.json.gz"), b"damaged alternative").unwrap();
        assert!(format!("{:#}", reuse().unwrap_err()).contains("Offline mode"));
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
    }

    #[test]
    fn snapshot_source_gaps_replay_offline_and_refresh_on_request() {
        let missing = Arc::new(AtomicBool::new(true));
        let source_missing = missing.clone();
        let server = Server::new(move |key| {
            if source_missing.load(Ordering::Relaxed)
                && matches!(
                    key.rsplit('/').next().unwrap(),
                    "000400Z.json.gz" | "000500Z.json.gz" | "000600Z.json.gz"
                )
            {
                (404, vec![], 0)
            } else {
                fixture(key)
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let client = server.client();
        let path = run(&client, dir.path()).unwrap();
        let manifest_path = path.parent().unwrap().join("sample-manifest.json");
        let bytes = std::fs::read(&manifest_path).unwrap();
        let mut manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest.missing_snapshot_slots, ["000500Z.json.gz"]);
        assert_eq!(manifest.snapshots.len(), 287);
        assert!(
            coverage_warning(&path)
                .unwrap()
                .contains("Unavailable discovery slots: 1")
        );
        let calls = server.calls.load(Ordering::Relaxed);
        assert_eq!(calls, 292);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let offline = Control::default().with_offline(true);
        let reuse = || {
            runtime.block_on(download(
                &client,
                DAY,
                dir.path(),
                bounds(),
                &offline,
                &|_, _| {},
            ))
        };
        assert_eq!(reuse().unwrap(), path);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
        assert_eq!(std::fs::read(&manifest_path).unwrap(), bytes);
        let actual = dir
            .path()
            .join(format!("adsbx-sample-{DAY}-snapshots/000000Z.json.gz"));
        let actual_bytes = std::fs::read(&actual).unwrap();
        std::fs::remove_file(&actual).unwrap();
        assert!(format!("{:#}", reuse().unwrap_err()).contains("Offline mode"));
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
        std::fs::write(&actual, actual_bytes).unwrap();
        let names: Vec<_> = (0..288).map(|i| snapshot_name(i * 5)).collect();
        manifest
            .missing_snapshot_slots
            .push("000500Z.json.gz".into());
        assert!(!snapshot_plan_valid(&manifest, &names));
        manifest.missing_snapshot_slots = vec!["000400Z.json.gz".into()];
        assert!(!snapshot_plan_valid(&manifest, &names));
        missing.store(false, Ordering::Relaxed);
        assert_eq!(run(&client, dir.path()).unwrap(), path);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
        assert_eq!(std::fs::read(&manifest_path).unwrap(), bytes);
        assert_eq!(
            run_controlled(
                &client,
                dir.path(),
                &Control::default().with_source_gap_refresh()
            )
            .unwrap(),
            path
        );
        assert_eq!(server.calls.load(Ordering::Relaxed), calls + 1);
        let repaired: Manifest =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        assert!(repaired.missing_snapshot_slots.is_empty());
        assert_eq!(repaired.snapshots.len(), 288);
    }
    #[test]
    fn completed_missing_trace_cache_resumes_online_and_refreshes_on_request() {
        let available = Arc::new(AtomicBool::new(false));
        let state = available.clone();
        let server = Server::new(move |key| {
            if key.contains("trace_full_~abc123") && !state.load(Ordering::Relaxed) {
                (404, vec![], 0)
            } else {
                fixture(key)
            }
        });
        let directory = tempfile::tempdir().unwrap();
        let client = server.client();
        let path = run(&client, directory.path()).unwrap();
        let manifest_path = path.parent().unwrap().join("sample-manifest.json");
        let original_manifest = std::fs::read(&manifest_path).unwrap();
        let calls = server.calls.load(Ordering::Relaxed);
        assert_eq!(run(&client, directory.path()).unwrap(), path);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
        assert_eq!(std::fs::read(&manifest_path).unwrap(), original_manifest);

        let downloaded = path.join("trace_full_abcdef.json.gz");
        let original_trace = std::fs::read(&downloaded).unwrap();
        std::fs::remove_file(&downloaded).unwrap();
        assert_eq!(run(&client, directory.path()).unwrap(), path);
        assert_eq!(std::fs::read(&downloaded).unwrap(), original_trace);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls + 2);

        available.store(true, Ordering::Relaxed);
        assert_eq!(run(&client, directory.path()).unwrap(), path);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls + 2);
        assert_eq!(std::fs::read(&manifest_path).unwrap(), original_manifest);
        let refresh = Control::default().with_source_gap_refresh();
        let offline = refresh.with_offline(true);
        let error = run_controlled(&client, directory.path(), &offline).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot be refreshed in offline mode")
        );
        assert_eq!(server.calls.load(Ordering::Relaxed), calls + 2);
        assert_eq!(
            run_controlled(&client, directory.path(), &refresh).unwrap(),
            path
        );
        assert_eq!(server.calls.load(Ordering::Relaxed), calls + 3);
        let updated: Manifest =
            serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
        assert!(updated.missing_aircraft.is_empty());
        assert_eq!(updated.traces.len(), 2);
    }

    #[test]
    fn missing_traces_are_recorded_and_empty_snapshot_days_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(|key| {
            if key.contains("trace_full_~abc123") {
                (404, vec![], 0)
            } else {
                fixture(key)
            }
        });
        let path = run(&server.client(), dir.path()).unwrap();
        let warning = coverage_warning(&path).unwrap();
        assert!(warning.contains("1 full-day traces, 1 unavailable"));
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(path.parent().unwrap().join("sample-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.missing_aircraft, ["~abc123"]);
        let calls = server.calls.load(Ordering::Relaxed);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let offline = Control::default().with_offline(true);
        let reused = runtime
            .block_on(download(
                &server.client(),
                DAY,
                dir.path(),
                bounds(),
                &offline,
                &|_, _| {},
            ))
            .unwrap();
        assert_eq!(reused, path);
        assert_eq!(coverage_warning(&reused).unwrap(), warning);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
        // A lost downloaded object remains a cache error even when another trace was unavailable upstream.
        std::fs::remove_file(path.join(&manifest.traces[0].file)).unwrap();
        let error = runtime
            .block_on(download(
                &server.client(),
                DAY,
                dir.path(),
                bounds(),
                &offline,
                &|_, _| {},
            ))
            .unwrap_err();
        assert!(format!("{error:#}").contains("Offline mode"));
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);

        let absent = Server::new(|_| (404, vec![], 0));
        let other = tempfile::tempdir().unwrap();
        assert!(
            run(&absent.client(), other.path())
                .unwrap_err()
                .to_string()
                .contains("No discovery snapshots are available")
        );
        assert_eq!(absent.calls.load(Ordering::Relaxed), 863);
    }

    fn recovery_fixture(key: &str, full_available: bool) -> (u16, Vec<u8>, u64) {
        if key.contains("trace_full_~abc123") && !full_available {
            return (404, vec![], 0);
        }
        if !key.contains("/readsb-hist/") {
            return fixture(key);
        }
        let name = key.rsplit('/').next().unwrap();
        let second = name[..2].parse::<u32>().unwrap() * 3600
            + name[2..4].parse::<u32>().unwrap() * 60
            + name[4..6].parse::<u32>().unwrap();
        if second == 1195 {
            return (404, vec![], 0);
        }
        let mut aircraft = vec![json!({"hex":"abcdef","lat":10.4,"lon":20.4})];
        if (1100..=1300).contains(&second) && !(1205..=1230).contains(&second) {
            aircraft.push(json!({"hex":"~abc123","lat":10.5,
                "lon":20.5 + f64::from(second - 1100) * 0.0001,
                "alt_baro":900,"alt_geom":1000,"seen_pos":1.,"type":"tisb_trackfile"}));
        }
        (
            200,
            compressed(json!({"now": start() + f64::from(second) - 1., "aircraft":aircraft})),
            0,
        )
    }

    #[test]
    fn missing_airborne_trace_recovers_offline_with_source_and_cache_checks() {
        let directory = tempfile::tempdir().unwrap();
        let available = Arc::new(AtomicBool::new(false));
        let state = available.clone();
        let server = Server::new(move |key| recovery_fixture(key, state.load(Ordering::Relaxed)));
        let client = server.client();
        let path = run(&client, directory.path()).unwrap();
        let manifest_path = path.parent().unwrap().join("sample-manifest.json");
        let original_manifest = std::fs::read(&manifest_path).unwrap();
        let manifest: Manifest = serde_json::from_slice(&original_manifest).unwrap();
        assert_eq!(manifest.missing_aircraft, ["~abc123"]);
        assert_eq!(manifest.snapshot_recoveries.len(), 1);
        let recovered = &manifest.snapshot_recoveries[0];
        assert_eq!(recovered.missing_slots, [1195]);
        assert_eq!(
            recovered.snapshots.len() + recovered.missing_slots.len(),
            recovered.slots.len()
        );
        assert!(
            coverage_warning(&path)
                .unwrap()
                .contains("Snapshot recovery produced 1 short traces")
        );
        let derived_path = path
            .join("snapshot-recovery")
            .join(&recovered.trace.as_ref().unwrap().file);
        let original_trace = std::fs::read(&derived_path).unwrap();
        let data =
            crate::data::load(&derived_path, &Control::default().with_offline(true)).unwrap();
        assert_eq!(data.tracks.len(), 2);
        assert!(
            data.tracks
                .iter()
                .all(|track| track.samples.iter().all(|sample| sample.point.alt == 304.8))
        );
        assert!(data.tracks[0].samples.last().unwrap().time < start() + 1200.);
        assert!(data.tracks[1].samples.first().unwrap().time > start() + 1230.);
        let calls = server.calls.load(Ordering::Relaxed);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let offline = Control::default().with_offline(true);
        let reuse = || {
            runtime.block_on(download(
                &client,
                DAY,
                directory.path(),
                bounds(),
                &offline,
                &|_, _| {},
            ))
        };
        assert_eq!(reuse().unwrap(), path);
        assert_eq!(std::fs::read(&manifest_path).unwrap(), original_manifest);
        assert_eq!(std::fs::read(&derived_path).unwrap(), original_trace);
        let snapshot = directory
            .path()
            .join(format!("adsbx-sample-{DAY}-snapshots"))
            .join(&recovered.snapshots[0].file);
        let source_bytes = std::fs::read(&snapshot).unwrap();
        std::fs::write(&snapshot, b"damaged source").unwrap();
        assert!(reuse().is_err());
        std::fs::write(&snapshot, source_bytes).unwrap();
        std::fs::write(&derived_path, b"damaged derived trace").unwrap();
        assert!(reuse().is_err());
        std::fs::write(&derived_path, &original_trace).unwrap();
        let extra = derived_path.with_file_name("trace_full_c0ffee.json.gz");
        std::fs::write(&extra, &original_trace).unwrap();
        assert!(reuse().is_err());
        std::fs::remove_file(extra).unwrap();
        assert_eq!(reuse().unwrap(), path);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
        available.store(true, Ordering::Relaxed);
        assert_eq!(run(&client, directory.path()).unwrap(), path);
        assert_eq!(server.calls.load(Ordering::Relaxed), calls);
        assert_eq!(std::fs::read(&manifest_path).unwrap(), original_manifest);
        assert_eq!(
            run_controlled(
                &client,
                directory.path(),
                &Control::default().with_source_gap_refresh()
            )
            .unwrap(),
            path
        );
        let upgraded: Manifest =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        assert!(upgraded.missing_aircraft.is_empty());
        assert!(upgraded.snapshot_recoveries.is_empty());
        assert!(!path.join("snapshot-recovery").exists());
        let preserved: Vec<_> = walkdir::WalkDir::new(path.parent().unwrap().join("rejected"))
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name() == "trace_full_~abc123.json.gz")
            .collect();
        assert_eq!(preserved.len(), 1);
        assert_eq!(std::fs::read(preserved[0].path()).unwrap(), original_trace);
    }

    #[test]
    fn recovery_refusal_and_cancellation_do_not_publish_a_complete_manifest() {
        for refusal in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let server = Server::new(move |key| {
                if refusal && key.ends_with("/001455Z.json.gz") {
                    (403, vec![], 0)
                } else {
                    recovery_fixture(key, false)
                }
            });
            let client = server.client();
            let control = Control::default();
            let progress = |message: &str, _: f32| {
                if !refusal && message.starts_with("Free snapshot recovery:") {
                    control.stop();
                }
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(download(
                &client,
                DAY,
                directory.path(),
                bounds(),
                &control,
                &progress,
            ));
            assert!(result.is_err());
            if !refusal {
                assert!(format!("{:#}", result.unwrap_err()).contains("Cancelled"));
            }
            assert!(
                !walkdir::WalkDir::new(directory.path())
                    .into_iter()
                    .filter_map(|entry| entry.ok())
                    .any(|entry| entry.file_name() == "sample-manifest.json")
            );
        }
    }
    #[test]
    fn refused_public_access_does_not_retry_or_use_another_bucket() {
        let server = Server::new(|_| (403, vec![], 0));
        let dir = tempfile::tempdir().unwrap();
        let error = run(&server.client(), dir.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no paid bucket or alternate account")
        );
        assert_eq!(server.calls.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn cancellation_interrupts_a_stalled_sample_body() {
        let server = Server::new(|_| (200, vec![0; 100], 1500));
        let client = server.client();
        let control = Control::default();
        let stop = control.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            stop.stop();
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let start = std::time::Instant::now();
        let error = runtime
            .block_on(client.get("readsb-hist/2024/03/01/000000Z.json.gz", &control))
            .unwrap_err();
        assert!(error.to_string().contains("Cancelled"));
        assert!(start.elapsed() < Duration::from_millis(1000));
        worker.join().unwrap();
    }
}
