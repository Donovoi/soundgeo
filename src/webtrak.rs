//! Public airport replay. Every UTC day has 2880 validated 30s chunks.
use crate::{
    Control, Progress,
    geo::{Point, geoid_undulation},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::{
    America::{Chicago, Los_Angeles},
    Tz,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Chicago,
    Lax,
}

impl Site {
    pub fn name(self) -> &'static str {
        match self {
            Self::Chicago => "Chicago",
            Self::Lax => "LAX",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Chicago => "cda",
            Self::Lax => "lax4",
        }
    }
    fn provider(self) -> &'static str {
        match self {
            Self::Chicago => "webtrak_chicago",
            Self::Lax => "webtrak_lax",
        }
    }
    fn timezone(self) -> Tz {
        match self {
            Self::Chicago => Chicago,
            Self::Lax => Los_Angeles,
        }
    }
    fn base(self) -> &'static str {
        match self {
            Self::Chicago => "https://webtrak-us02.emsbk.com/WebTrak/cda/data/",
            Self::Lax => "https://webtrak-us02.emsbk.com/WebTrak/lax4/data/",
        }
    }
}
const LIMIT: usize = 2_000_000;
const CHUNKS: usize = 2880;

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "@handle")]
    handle: i64,
    #[serde(rename = "@startTime")]
    start: String,
    #[serde(rename = "@endTime")]
    end: String,
    #[serde(rename = "@radarStatus")]
    radar_status: String,
    #[serde(rename = "$text", default)]
    body: String,
}
#[derive(Serialize)]
struct Position {
    aircraft: String,
    time: String,
    lat: f64,
    lon: f64,
    altitude_m: f64,
    type_code: String,
    callsign: String,
    registration: String,
    altitude_msl_m: f64,
}
struct Chunk {
    positions: Vec<Position>,
    online: bool,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn decode(site: Site, bytes: &[u8], handle: i64) -> Result<Chunk> {
    if bytes.len() > LIMIT {
        bail!("WebTrak chunk exceeds 2 MB");
    }
    let text = std::str::from_utf8(bytes).context("WebTrak replay is not UTF-8")?;
    let envelope: Envelope = quick_xml::de::from_str(text).context("Invalid WebTrak replay XML")?;
    let epoch = handle.checked_mul(30).context("Invalid replay handle")?;
    let utc = DateTime::<Utc>::from_timestamp(epoch, 0).context("Invalid replay timestamp")?;
    let local = utc.with_timezone(&site.timezone()).naive_local();
    // The UTC handle resolves repeated local times on the autumn DST boundary.
    if envelope.handle != handle
        || envelope.start != local.format("%Y-%m-%d %H:%M:%S").to_string()
        || envelope.end
            != (local + chrono::Duration::seconds(29))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
    {
        bail!("WebTrak replay handle/time mismatch");
    }
    let mut positions = vec![];
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(envelope.body.as_bytes());
    for record in reader.records() {
        let row = record.context("Invalid WebTrak radar CSV")?;
        if row.get(0) != Some("radar") {
            continue;
        }
        if row.len() < 19 || row[2].is_empty() {
            bail!("Truncated WebTrak radar position");
        }
        let when = NaiveDateTime::parse_from_str(&row[1], "%Y%m%d%H%M%S")?;
        let delta = (when - local).num_seconds();
        if !(0..30).contains(&delta) {
            bail!("WebTrak position is outside its 30-second chunk");
        }
        let lat: f64 = row[9].parse()?;
        let lon: f64 = row[8].parse()?;
        let height: f64 = row[18].parse()?;
        let point = Point {
            lat,
            lon,
            alt: height,
        };
        if !point.valid() {
            bail!("Invalid WebTrak position coordinate or altitude");
        }
        positions.push(Position {
            aircraft: format!("{}:{}", site.id(), &row[2]),
            time: (utc + chrono::Duration::seconds(delta)).to_rfc3339(),
            lat,
            lon,
            altitude_m: height + geoid_undulation(lat, lon),
            type_code: row[5].into(),
            callsign: row[6].into(),
            registration: row[12].into(),
            altitude_msl_m: height,
        });
    }
    Ok(Chunk {
        positions,
        online: envelope.radar_status == "online",
    })
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("Missing cache parent")?)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn limited_read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        bail!("Cached file exceeds size limit");
    }
    Ok(bytes)
}
fn cached_chunk(site: Site, folder: &Path, handle: i64) -> Option<Chunk> {
    let bytes = limited_read(&folder.join(format!("{handle}.xml")), LIMIT).ok()?;
    // Accept the qualification downloader's sidecars too; both use SHA-256.
    let meta: serde_json::Value = serde_json::from_slice(
        &limited_read(&folder.join(format!("{handle}.meta.json")), 65536).ok()?,
    )
    .ok()?;
    if meta["sha256"].as_str() != Some(&digest(&bytes)) {
        return None;
    }
    if meta["url"].as_str() != Some(&format!("{}{handle}.mobile", site.base())) {
        return None;
    }
    decode(site, &bytes, handle).ok()
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: u32,
    date: String,
    provider: String,
    complete: bool,
    chunks: usize,
    rows: usize,
    offline_chunks: usize,
    csv_sha256: String,
    source: String,
    altitude: String,
}
fn cached_day(site: Site, folder: &Path, date: &str) -> Option<PathBuf> {
    let manifest: Manifest =
        serde_json::from_slice(&limited_read(&folder.join("provider-manifest.json"), 65536).ok()?)
            .ok()?;
    if manifest.format != 1
        || manifest.date != date
        || manifest.provider != site.provider()
        || manifest.source != site.base()
        || manifest.offline_chunks > CHUNKS
        || !manifest.complete
        || manifest.chunks != CHUNKS
        || manifest.rows == 0
    {
        return None;
    }
    let path = folder.join("flights.csv");
    let mut file = fs::File::open(&path).ok()?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).ok()?;
    (format!("{:x}", hash.finalize()) == manifest.csv_sha256).then_some(path)
}

fn ensure_day(
    site: Site,
    date: &str,
    cache: &Path,
    control: &Control,
    progress: &Progress<'_>,
    fetch: &(impl Fn(i64) -> Result<Vec<u8>> + Sync),
) -> Result<PathBuf> {
    control.check()?;
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .context("Use a YYYY-MM-DD airport archive date")?;
    if day >= Utc::now().date_naive() || day < NaiveDate::from_ymd_opt(2023, 1, 1).unwrap() {
        bail!("Choose an airport archive date from 2023 through yesterday UTC");
    }
    let folder = cache.join(format!("webtrak-{}-{date}", site.id()));
    fs::create_dir_all(&folder)?;
    if let Some(path) = cached_day(site, &folder, date) {
        progress(
            &format!("Reusing verified {} full-day data", site.name()),
            1.,
        );
        return Ok(path);
    }
    control.require_network(&folder.display().to_string())?;
    let first = day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() / 30;
    let handles: Vec<_> = (first..first + CHUNKS as i64).collect();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build()?;
    let mut temporary = tempfile::NamedTempFile::new_in(&folder)?;
    let mut rows = 0;
    let mut offline = 0;
    {
        let mut writer = csv::Writer::from_writer(temporary.as_file_mut());
        for (batch_index, batch) in handles.chunks(60).enumerate() {
            control.check()?;
            progress(
                &format!("{} replay: {}/2880 chunks", site.name(), batch_index * 60),
                batch_index as f32 / 48.,
            );
            let chunks:Vec<Chunk>=pool.install(|| batch.par_iter().map(|&handle|->Result<Chunk>{
                control.check()?;
                if let Some(chunk)=cached_chunk(site,&folder,handle) {return Ok(chunk);}
                let bytes=fetch(handle).with_context(||format!("Could not obtain {} chunk {handle}; full day is incomplete",site.name()))?;
                let chunk=decode(site,&bytes,handle)?;
                atomic_write(&folder.join(format!("{handle}.xml")),&bytes)?;
                atomic_write(&folder.join(format!("{handle}.meta.json")),&serde_json::to_vec(&serde_json::json!({"url":format!("{}{handle}.mobile",site.base()),"sha256":digest(&bytes),"fetched_utc":Utc::now().to_rfc3339()}))?)?;
                Ok(chunk)
            }).collect::<Result<Vec<_>>>())?;
            // Indexed parallel collection preserves chronological chunk order.
            for chunk in chunks {
                offline += usize::from(!chunk.online);
                for position in chunk.positions {
                    writer.serialize(position)?;
                    rows += 1;
                }
            }
        }
        writer.flush()?;
    }
    if rows == 0 {
        bail!("WebTrak replay returned no positions; no completed archive was saved");
    }
    temporary.as_file().sync_all()?;
    let mut hash = Sha256::new();
    std::io::copy(&mut fs::File::open(temporary.path())?, &mut hash)?;
    let manifest = Manifest {
        format: 1,
        date: date.into(),
        provider: site.provider().into(),
        complete: true,
        chunks: CHUNKS,
        rows,
        offline_chunks: offline,
        csv_sha256: format!("{:x}", hash.finalize()),
        source: site.base().into(),
        altitude: "Field 18 MSL metres + EGM96 N; original sensor/pressure provenance unknown"
            .into(),
    };
    let path = folder.join("flights.csv");
    temporary.persist(&path).map_err(|e| e.error)?;
    atomic_write(
        &folder.join("provider-manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    progress(
        &format!(
            "{} full day saved: {rows} positions, {offline} chunks not marked online",
            site.name()
        ),
        1.,
    );
    Ok(path)
}

enum Reply {
    Data(Vec<u8>),
    RetryAfter(String),
}

fn request_chunk(
    client: &reqwest::Client,
    runtime: &tokio::runtime::Runtime,
    url: &str,
    control: &Control,
) -> Result<Reply> {
    runtime.block_on(async {
        let mut response = crate::network::cancelable(client.get(url).send(), control).await??;
        if matches!(response.status().as_u16(), 429 | 503) {
            return Ok(Reply::RetryAfter(
                response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .map(|v| v.to_str())
                    .transpose()?
                    .unwrap_or("10")
                    .to_owned(),
            ));
        }
        response.error_for_status_ref()?;
        if response.content_length().is_some_and(|n| n > LIMIT as u64) {
            bail!("WebTrak response exceeds 2 MB");
        }
        let mut bytes = vec![];
        while let Some(chunk) = crate::network::cancelable(response.chunk(), control).await?? {
            if bytes.len() + chunk.len() > LIMIT {
                bail!("WebTrak response exceeds 2 MB");
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Reply::Data(bytes))
    })
}

fn fetch_with_retry(
    control: &Control,
    next: &Mutex<Instant>,
    mut fetch: impl FnMut() -> Result<Reply>,
) -> Result<Vec<u8>> {
    for attempt in 0..3 {
        control.check()?;
        loop {
            control.check()?;
            let wait = {
                let mut n = next
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Request scheduler failed"))?;
                let now = Instant::now();
                if now >= *n {
                    *n = now + Duration::from_millis(500);
                    None
                } else {
                    Some(n.saturating_duration_since(now))
                }
            };
            match wait {
                None => break,
                Some(delay) => std::thread::sleep(delay.min(Duration::from_millis(50))),
            }
        }
        let pause = match fetch() {
            Ok(Reply::Data(bytes)) => return Ok(bytes),
            Ok(Reply::RetryAfter(pause)) => pause,
            Err(error) => {
                let transient = error.downcast_ref::<reqwest::Error>().is_some_and(|e| {
                    // reqwest marks a truncated streamed HTTP body as Decode.
                    // XML parsing happens after this helper, so malformed
                    // archive data is never retried here.
                    e.is_timeout()
                        || e.is_connect()
                        || e.is_request()
                        || e.is_body()
                        || e.is_decode()
                });
                if attempt == 2 || !transient {
                    return Err(error);
                }
                (1u64 << attempt).to_string()
            }
        };
        {
            if attempt == 2 {
                bail!("WebTrak replay retry limit reached");
            }
            let seconds = if let Ok(seconds) = pause.parse::<u64>() {
                seconds
            } else {
                (DateTime::parse_from_rfc2822(&pause)
                    .context("Unrecognized server retry delay; download stopped")?
                    .timestamp()
                    - Utc::now().timestamp())
                .max(1) as u64
            };
            if seconds > 60 {
                bail!(
                    "WebTrak server requests a longer pause; retry this resumable download later"
                );
            }
            let mut n = next
                .lock()
                .map_err(|_| anyhow::anyhow!("Request scheduler failed"))?;
            *n = (*n).max(Instant::now() + Duration::from_secs(seconds.max(1)));
            continue;
        }
    }
    bail!("WebTrak replay retry limit reached")
}

pub fn download_day(
    site: Site,
    date: &str,
    cache: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .user_agent("SoundGeo/0.3 public-airport-replay")
        .build()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let next = Mutex::new(Instant::now());
    let path = ensure_day(site, date, cache, control, progress, &|handle| {
        let day = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
        if (Utc::now().date_naive() - day).num_days() > 90 {
            bail!(
                "WebTrak public replay keeps about 90 days. Use a complete local cache or another free source for older dates."
            );
        }
        fetch_with_retry(control, &next, || {
            request_chunk(
                &client,
                &runtime,
                &format!("{}{handle}.mobile", site.base()),
                control,
            )
        })
    })?;
    control.check()?;
    let catalog = path
        .parent()
        .context("Missing WebTrak cache folder")?
        .join("aircraft-types.json");
    if !catalog.is_file() {
        control.require_network(&catalog.display().to_string())?;
        crate::aircraft::download(&catalog).context("WebTrak flight data is saved, but the free aircraft-type catalog could not be downloaded")?;
    }
    Ok(path)
}

pub fn coverage_warning(site: Site, path: &Path) -> Result<Option<String>> {
    let file = path
        .parent()
        .context("Missing WebTrak cache folder")?
        .join("provider-manifest.json");
    let manifest: Manifest = serde_json::from_slice(&limited_read(&file, 65536)?)?;
    Ok((manifest.offline_chunks>0).then(||format!("{} radar was not marked online for {} of 2880 intervals on {}. Those intervals can have missing flight observations.",site.name(),manifest.offline_chunks,manifest.date)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_truncated_response_but_does_not_retry_forbidden_status() {
        for forbidden in [false, true] {
            let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = server.local_addr().unwrap();
            let thread = std::thread::spawn(move || {
                for attempt in 0..if forbidden { 1 } else { 2 } {
                    let (mut socket, _) = server.accept().unwrap();
                    let mut request = [0; 2048];
                    let _ = socket.read(&mut request);
                    if forbidden {
                        let _=socket.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    } else {
                        let _ = socket.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
                        );
                        let _ = socket.write_all(if attempt == 0 { b"da" } else { b"data" });
                    }
                }
            });
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap();
            let control = Control::default();
            let next = Mutex::new(Instant::now());
            let mut calls = 0;
            let result = fetch_with_retry(&control, &next, || {
                calls += 1;
                request_chunk(
                    &client,
                    &runtime,
                    &format!("http://{address}/chunk"),
                    &control,
                )
            });
            if forbidden {
                assert!(
                    result
                        .unwrap_err()
                        .downcast_ref::<reqwest::Error>()
                        .is_some_and(|e| e.status() == Some(reqwest::StatusCode::FORBIDDEN))
                );
                assert_eq!(calls, 1);
            } else {
                assert_eq!(result.unwrap(), b"data");
                assert_eq!(calls, 2);
            }
            thread.join().unwrap();
        }
    }
    #[test]
    fn cancellation_interrupts_stalled_replay_and_oversized_headers_are_rejected() {
        for oversized in [false, true] {
            let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = server.local_addr().unwrap();
            let thread = std::thread::spawn(move || {
                let (mut socket, _) = server.accept().unwrap();
                let mut request = [0; 2048];
                let _ = socket.read(&mut request);
                let count = if oversized { LIMIT + 1 } else { 4 };
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {count}\r\nConnection: close\r\n\r\n"
                );
                std::thread::sleep(Duration::from_millis(700));
                let _ = socket.write_all(b"data");
            });
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();
            let control = Control::default();
            let other = control.clone();
            let cancellation = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                if !oversized {
                    other.stop();
                }
            });
            let begin = Instant::now();
            let error = match request_chunk(
                &client,
                &runtime,
                &format!("http://{address}/chunk"),
                &control,
            ) {
                Err(e) => e,
                Ok(_) => panic!("Invalid request succeeded"),
            };
            assert!(
                error.to_string().contains(if oversized {
                    "exceeds 2 MB"
                } else {
                    "Cancelled"
                }),
                "{error:#}"
            );
            assert!(begin.elapsed() < Duration::from_secs(2));
            cancellation.join().unwrap();
            thread.join().unwrap();
        }
    }
    fn xml(site: Site, handle: i64, body: &str) -> Vec<u8> {
        let local = DateTime::<Utc>::from_timestamp(handle * 30, 0)
            .unwrap()
            .with_timezone(&site.timezone())
            .naive_local();
        format!("<data handle=\"{handle}\" startTime=\"{local}\" endTime=\"{}\" radarStatus=\"online\">{body}</data>",local+chrono::Duration::seconds(29)).into_bytes()
    }
    fn row(site: Site, handle: i64) -> String {
        let t = DateTime::<Utc>::from_timestamp(handle * 30, 0)
            .unwrap()
            .with_timezone(&site.timezone());
        format!(
            "radar,{},track1,D,MDW,B738,CALL,J,-87.74,41.78,50,100,REG,1200,MDW,,,tag,280,op,route",
            t.format("%Y%m%d%H%M%S")
        )
    }
    #[test]
    fn repeated_local_hour_resolves_to_distinct_utc_and_uses_altitude_not_height() {
        let first = DateTime::parse_from_rfc3339("2026-11-01T06:30:00Z")
            .unwrap()
            .timestamp()
            / 30;
        let second = first + 120;
        let a = decode(
            Site::Chicago,
            &xml(Site::Chicago, first, &row(Site::Chicago, first)),
            first,
        )
        .unwrap();
        let b = decode(
            Site::Chicago,
            &xml(Site::Chicago, second, &row(Site::Chicago, second)),
            second,
        )
        .unwrap();
        assert_eq!(a.positions[0].time, "2026-11-01T06:30:00+00:00");
        assert_eq!(b.positions[0].time, "2026-11-01T07:30:00+00:00");
        assert!(
            (a.positions[0].altitude_m - (280. + geoid_undulation(41.78, -87.74))).abs() < 1e-9
        );
    }
    #[test]
    fn rejects_wrong_handle_time_invalid_coordinates_and_truncated_rows() {
        let h = 59785200;
        let valid = xml(Site::Chicago, h, &row(Site::Chicago, h));
        assert!(decode(Site::Chicago, &valid, h + 1).is_err());
        for bad in [
            row(Site::Chicago, h).replace("20261101120000", "20261101120100"),
            row(Site::Chicago, h).replace("41.78", "91.78"),
            "radar,20261101120000,id".into(),
        ] {
            assert!(decode(Site::Chicago, &xml(Site::Chicago, h, &bad), h).is_err());
        }
        assert!(decode(Site::Chicago, &vec![b'x'; LIMIT + 1], h).is_err());
    }
    #[test]
    fn lax_repeated_hour_and_cache_identity_cannot_use_chicago_data() {
        let first = DateTime::parse_from_rfc3339("2026-11-01T08:30:00Z")
            .unwrap()
            .timestamp()
            / 30;
        for handle in [first, first + 120] {
            let bytes = xml(Site::Lax, handle, &row(Site::Lax, handle));
            let chunk = decode(Site::Lax, &bytes, handle).unwrap();
            assert_eq!(
                chunk.positions[0].time,
                DateTime::<Utc>::from_timestamp(handle * 30, 0)
                    .unwrap()
                    .to_rfc3339()
            );
            assert_eq!(chunk.positions[0].aircraft, "lax4:track1");
            assert!(decode(Site::Chicago, &bytes, handle).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let bytes = xml(Site::Lax, first, &row(Site::Lax, first));
        fs::write(dir.path().join(format!("{first}.xml")), &bytes).unwrap();
        for site in [Site::Chicago, Site::Lax] {
            fs::write(dir.path().join(format!("{first}.meta.json")),serde_json::to_vec(&serde_json::json!({"sha256":digest(&bytes),"url":format!("{}{first}.mobile",site.base())})).unwrap()).unwrap();
            assert_eq!(
                cached_chunk(Site::Lax, dir.path(), first).is_some(),
                site == Site::Lax
            );
        }
        let csv = dir.path().join("flights.csv");
        fs::write(&csv, b"fixture").unwrap();
        let mut manifest = Manifest {
            format: 1,
            date: "2026-11-01".into(),
            provider: Site::Lax.provider().into(),
            complete: true,
            chunks: CHUNKS,
            rows: 1,
            offline_chunks: 0,
            csv_sha256: digest(b"fixture"),
            source: Site::Lax.base().into(),
            altitude: "fixture".into(),
        };
        let write = |m: &Manifest| {
            fs::write(
                dir.path().join("provider-manifest.json"),
                serde_json::to_vec(m).unwrap(),
            )
            .unwrap()
        };
        write(&manifest);
        assert_eq!(cached_day(Site::Lax, dir.path(), "2026-11-01"), Some(csv));
        assert!(cached_day(Site::Chicago, dir.path(), "2026-11-01").is_none());
        manifest.source = Site::Chicago.base().into();
        write(&manifest);
        assert!(cached_day(Site::Lax, dir.path(), "2026-11-01").is_none());
    }
    #[test]
    fn complete_day_cache_is_verified_and_corrupt_csv_is_rebuilt_without_network() {
        for site in [Site::Chicago, Site::Lax] {
            let dir = tempfile::tempdir().unwrap();
            let date = "2024-11-03";
            let control = Control::default();
            let path = ensure_day(site, date, dir.path(), &control, &|_, _| {}, &|h| {
                Ok(xml(site, h, &row(site, h)))
            })
            .unwrap();
            let original = fs::read(&path).unwrap();
            let no_network = |_| -> Result<Vec<u8>> { bail!("Unexpected network request") };
            assert_eq!(
                path,
                ensure_day(site, date, dir.path(), &control, &|_, _| {}, &no_network).unwrap()
            );
            fs::write(&path, b"corrupt").unwrap();
            ensure_day(site, date, dir.path(), &control, &|_, _| {}, &no_network).unwrap();
            assert_eq!(fs::read(path).unwrap(), original);
        }
    }
    #[test]
    fn missing_chunk_and_cancellation_never_publish_a_complete_day() {
        let dir = tempfile::tempdir().unwrap();
        let date = "2024-11-03";
        let first = 1730592000 / 30;
        assert!(
            ensure_day(
                Site::Chicago,
                date,
                dir.path(),
                &Control::default(),
                &|_, _| {},
                &|h| {
                    if h == first + 5 {
                        bail!("Missing fixture chunk");
                    }
                    Ok(xml(Site::Chicago, h, &row(Site::Chicago, h)))
                }
            )
            .is_err()
        );
        assert!(
            !dir.path()
                .join(format!("webtrak-cda-{date}/flights.csv"))
                .exists()
        );
        assert!(
            !dir.path()
                .join(format!("webtrak-cda-{date}/provider-manifest.json"))
                .exists()
        );
        let control = Control::default();
        control.stop();
        assert!(
            ensure_day(
                Site::Chicago,
                date,
                dir.path(),
                &control,
                &|_, _| {},
                &|_| unreachable!()
            )
            .is_err()
        );
    }
}
