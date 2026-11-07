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
// Public demonstration values, not a user's account credentials.
const ACCESS: &str = "7b8d31d9d2f2d73ffb2208614db599fa";
const SECRET: &str = "8bd955340e355418a5204e52a055005b6ca761f896abe0cccdd741638e790a76";
const MAX_COMPRESSED: usize = 20_000_000;
const MAX_JSON: u64 = 128_000_000;

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
    // Historical snapshots can be recorded about 30 seconds after the
    // filename time, with small jitter. These snapshots select aircraft IDs;
    // the full traces retain their own timestamps for location analysis.
    ensure!(
        (now - expected).abs() < 60.,
        "Sample snapshot timestamp {now} is outside the one-minute window around {expected}"
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
fn trace(value: &Value, expected: f64, id: &str) -> Result<()> {
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
        (expected..expected + 86400.).contains(&timestamp),
        "Sample trace has the wrong date"
    );
    let positions = value["trace"]
        .as_array()
        .context("Sample trace lacks positions")?;
    // Older traces use the first observation as their base time. Newer
    // traces use midnight. In both formats, the importer adds each offset
    // to the original base; no timestamp is rewritten here.
    ensure!(
        positions
            .iter()
            .all(|row| row[0].as_f64().is_some_and(|offset| {
                (expected..expected + 86400.).contains(&(timestamp + offset))
            })),
        "Sample trace has an invalid observation time or a position outside the requested UTC day"
    );
    Ok(())
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
#[derive(Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    date: String,
    bounds: Bounds,
    source: String,
    snapshots: Vec<Object>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    snapshot_replacements: Vec<SnapshotReplacement>,
    aircraft: Vec<String>,
    traces: Vec<Object>,
    missing_aircraft: Vec<String>,
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
    if ![1, 2].contains(&manifest.schema) || manifest.snapshots.len() != names.len() {
        return false;
    }
    let mut replacements = Vec::new();
    for (index, (object, requested)) in manifest.snapshots.iter().zip(names).enumerate() {
        if object.file == *requested {
            continue;
        }
        if manifest.schema != 2
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
    replacements == manifest.snapshot_replacements
}
fn cached_aircraft_set_valid(manifest: &Manifest, allow_missing: bool) -> bool {
    if !allow_missing && !manifest.missing_aircraft.is_empty() {
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
    check(&value)?;
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
    control.check()?;
    let bounds = validate(date, bounds)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(download(
        &Client::new()?,
        date,
        cache,
        bounds,
        control,
        progress,
    ))
}
async fn download(
    client: &Client,
    date: &str,
    cache: &Path,
    bounds: Bounds,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    let tag = hash(&serde_json::to_vec(&bounds)?);
    let root = cache.join(format!("adsbx-sample-{date}-{}", &tag[..16]));
    let shared = cache.join(format!("adsbx-sample-{date}-snapshots"));
    let traces = root.join("traces");
    let manifest_path = root.join("sample-manifest.json");
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64;
    let names: Vec<_> = (0..288).map(|i| snapshot_name(i * 5)).collect();
    if let Ok(bytes) = std::fs::read(&manifest_path)
        && let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes)
        && manifest.date == date
        && serde_json::to_vec(&manifest.bounds)? == serde_json::to_vec(&bounds)?
        && snapshot_plan_valid(&manifest, &names)
        && cached_aircraft_set_valid(&manifest, control.offline)
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
        schema: 2,
        date: date.into(),
        bounds,
        source: "https://www.adsbexchange.com/data-products/sample-data/".into(),
        snapshots: vec![],
        snapshot_replacements: vec![],
        aircraft: vec![],
        traces: vec![],
        missing_aircraft: vec![],
    };
    let mut ids = BTreeSet::new();
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
        let (record, value, expected) = selected.with_context(|| {
            format!(
                "Missing free sample snapshot {name} and neighboring minutes for {date}; no snapshot was silently omitted"
            )
        })?;
        ids.extend(snapshot(&value, expected, bounds)?);
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
        manifest.snapshots.push(record);
        progress(
            &format!(
                "Full-day free sample: {}/288 snapshots, {} aircraft",
                index + 1,
                ids.len()
            ),
            0.7 * (index + 1) as f32 / 288.,
        );
    }
    std::fs::create_dir_all(&traces)?;
    manifest.aircraft = ids.iter().cloned().collect();
    for (index, id) in ids.iter().enumerate() {
        control.check()?;
        let file = format!("trace_full_{id}.json.gz");
        let key = format!(
            "traces/{prefix}/{}/trace_full_{id}.json",
            &id[id.len() - 2..]
        );
        if let Some((record, _)) = object(
            client,
            &key,
            &traces.join(file),
            &root.join("rejected"),
            control,
            |v| trace(v, start, id),
        )
        .await?
        {
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
            0.7 + 0.3 * (index + 1) as f32 / ids.len().max(1) as f32,
        );
    }
    control.check()?;
    atomic_write(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
    atomic_write(&root.join("SOURCE.txt"), b"ADS-B Exchange free first-of-month testing/evaluation sample. https://www.adsbexchange.com/data-products/sample-data/ . All 288 discovery slots were checked. A missing snapshot can use an available neighboring minute from the same day. Actual source files and replacement mappings are recorded in sample-manifest.json. Full-day traces were downloaded for regional identities. Flights entirely between discovery snapshots can be absent. Missing traces are listed in the manifest. Reference data is not bundled or redistributed by SoundGeo.\n")?;
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
    Ok((
        format!(
            "ADS-B Exchange free evaluation sample for {}: {} full-day traces, {} unavailable. Discovery inspected 288 snapshots near five-minute intervals in the search area.{} Flights between discovery snapshots and flights outside receiver coverage can be absent. The sample is available only on the first day of each month.",
            manifest.date,
            manifest.traces.len(),
            manifest.missing_aircraft.len(),
            replacements
        ),
        manifest.traces.is_empty(),
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
    fn run(client: &Client, cache: &Path) -> Result<PathBuf> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(download(
                client,
                DAY,
                cache,
                bounds(),
                &Control::default(),
                &|_, _| {},
            ))
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
        for offset in [-59.9, -1., 30.2, 31.9, 59.9] {
            let value = json!({"now":start()+offset,"aircraft":[
                {"hex":"abcdef","lat":10.5,"lon":20.5}
            ]});
            assert_eq!(
                snapshot(&value, start(), bounds()).unwrap(),
                BTreeSet::from(["abcdef".into()])
            );
            assert_eq!(value["now"].as_f64().unwrap(), start() + offset);
        }
        for offset in [-300., -60., 60., 300., 86400.] {
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
        value["trace"][0][0] = json!(86400.);
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
        assert_eq!(manifest.schema, 2);
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
    fn missing_traces_are_recorded_but_missing_snapshots_stop_the_download() {
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
                .contains("no snapshot was silently omitted")
        );
        assert_eq!(absent.calls.load(Ordering::Relaxed), 2);
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
