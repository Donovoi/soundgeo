//! Free-provider date indexes. An advertised date is checked again at download.
use crate::{Control, Progress, archive::ArchiveProvider, network::cancelable};
use anyhow::{Context, Result, ensure};
use chrono::{Datelike, Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_RESPONSE: usize = 8_000_000;
const MAX_CATALOG: u64 = 4_000_000;
const CACHE_SECONDS: i64 = 6 * 3600;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    pub sha256: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body_sha256: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub provider: ArchiveProvider,
    pub retrieved_utc: String,
    pub dates: Vec<String>,
    pub basis: String,
    pub sources: Vec<Source>,
    /// Last lightweight check for newly published daily archives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_checked_utc: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Stored {
    format: u32,
    payload: Catalog,
    sha256: String,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn cache_path(cache: &Path, provider: ArchiveProvider) -> PathBuf {
    let name = serde_json::to_value(provider).unwrap();
    cache.join(format!("available-dates-{}.json", name.as_str().unwrap()))
}
fn valid_date(value: &str, today: NaiveDate) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .is_ok_and(|date| date.to_string() == value && date.year() >= 1970 && date < today)
}
fn validate(catalog: &Catalog, provider: ArchiveProvider, today: NaiveDate) -> Result<()> {
    ensure!(
        catalog.provider == provider,
        "Date catalog belongs to a different provider"
    );
    ensure!(
        !catalog.dates.is_empty() && catalog.dates.iter().all(|d| valid_date(d, today)),
        "Date catalog has invalid or empty coverage"
    );
    ensure!(
        catalog.dates.windows(2).all(|p| p[0] < p[1]),
        "Date catalog must be ordered with no duplicates"
    );
    if provider.sample_resolution().is_some() {
        ensure!(
            catalog.dates.iter().all(|d| d.ends_with("-01")),
            "The free sample index contains a non-sample date"
        );
    }
    let stamp = chrono::DateTime::parse_from_rfc3339(&catalog.retrieved_utc)?;
    ensure!(
        stamp.timestamp() <= Utc::now().timestamp() + 300,
        "Date catalog timestamp is in the future"
    );
    if let Some(recent) = &catalog.recent_checked_utc {
        let checked = chrono::DateTime::parse_from_rfc3339(recent)?;
        ensure!(
            checked >= stamp && checked.timestamp() <= Utc::now().timestamp() + 300,
            "Recent date check timestamp is outside the catalog lifetime"
        );
    }
    ensure!(
        !catalog.sources.is_empty(),
        "Date catalog has no source records"
    );
    Ok(())
}
pub fn load(cache: &Path, provider: ArchiveProvider) -> Result<Catalog> {
    let path = cache_path(cache, provider);
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .with_context(|| {
            format!(
                "No saved date index at {}. Discover dates online first.",
                path.display()
            )
        })?
        .take(MAX_CATALOG + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_CATALOG,
        "Date catalog exceeds 4 MB"
    );
    let stored: Stored = serde_json::from_slice(&bytes).context("Invalid saved date index")?;
    ensure!(
        stored.format == 1 && stored.sha256 == hash(&serde_json::to_vec(&stored.payload)?),
        "Date catalog integrity check failed"
    );
    validate(&stored.payload, provider, Utc::now().date_naive())?;
    Ok(stored.payload)
}
pub(crate) fn save(cache: &Path, catalog: &Catalog) -> Result<()> {
    validate(catalog, catalog.provider, Utc::now().date_naive())?;
    std::fs::create_dir_all(cache)?;
    let path = cache_path(cache, catalog.provider);
    let stored = Stored {
        format: 1,
        sha256: hash(&serde_json::to_vec(catalog)?),
        payload: catalog.clone(),
    };
    let (mut file, destination) = crate::atomic_file::prepare(&path)?;
    serde_json::to_writer_pretty(file.as_file_mut(), &stored)?;
    file.flush()?;
    file.as_file().sync_all()?;
    if path.exists() && load(cache, catalog.provider).is_err() {
        let suffix = Utc::now()
            .timestamp_nanos_opt()
            .context("Invalid system clock")?;
        std::fs::rename(
            &path,
            path.with_extension(format!("rejected-{suffix}.json")),
        )?;
    }
    file.persist(destination)?;
    Ok(())
}

pub fn write_report(catalog: &Catalog, output: &Path, cache: &Path) -> Result<()> {
    if output == Path::new("-") {
        let mut stream = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stream, catalog)?;
        writeln!(stream)?;
        return Ok(());
    }
    ensure!(
        !output
            .canonicalize()
            .ok()
            .is_some_and(|p| cache_path(cache, catalog.provider)
                .canonicalize()
                .is_ok_and(|c| c == p)),
        "The report cannot overwrite its date-index cache"
    );
    let (mut file, destination) = crate::atomic_file::prepare(output)?;
    serde_json::to_writer_pretty(file.as_file_mut(), catalog)?;
    file.persist(destination)?;
    Ok(())
}

pub fn discover(
    provider: ArchiveProvider,
    cache: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Catalog> {
    control.check()?;
    ensure!(
        !cache.as_os_str().is_empty() && (!cache.exists() || cache.is_dir()),
        "Choose a folder for the flight date cache"
    );
    if control.offline {
        return load(cache, provider);
    }
    if let Ok(catalog) = load(cache, provider) {
        let stamp = chrono::DateTime::parse_from_rfc3339(&catalog.retrieved_utc)?;
        if Utc::now().timestamp() - stamp.timestamp() < CACHE_SECONDS {
            return refresh_cached(catalog, cache, control, progress, Utc::now(), |catalog| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(refresh_recent(catalog, control, progress))
            });
        }
    }
    control.require_network("discover available flight dates")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let catalog = runtime.block_on(discover_online(provider, control, progress))?;
    save(cache, &catalog)?;
    progress(
        &format!(
            "Found {} available dates; flight files are prepared as each date is searched",
            catalog.dates.len()
        ),
        1.,
    );
    Ok(catalog)
}

fn recent_refresh_due(catalog: &Catalog, now: chrono::DateTime<Utc>) -> Result<bool> {
    if catalog.provider != ArchiveProvider::AdsbLol {
        return Ok(false);
    }
    let stamp = chrono::DateTime::parse_from_rfc3339(
        catalog
            .recent_checked_utc
            .as_deref()
            .unwrap_or(&catalog.retrieved_utc),
    )?;
    Ok(now.timestamp() - stamp.timestamp() >= 15 * 60)
}

fn recent_years(today: NaiveDate) -> BTreeSet<i32> {
    [1, 2]
        .into_iter()
        .filter_map(|days| today.checked_sub_days(Days::new(days)))
        .map(|day| day.year())
        .filter(|&year| year >= 2023)
        .collect()
}

fn refresh_cached(
    catalog: Catalog,
    cache: &Path,
    control: &Control,
    progress: &Progress<'_>,
    now: chrono::DateTime<Utc>,
    refresh: impl FnOnce(Catalog) -> Result<Catalog>,
) -> Result<Catalog> {
    control.check()?;
    if !control.offline && recent_refresh_due(&catalog, now)? {
        // Keep the verified index intact if the provider is temporarily down.
        // A cancellation must still stop the operation.
        match refresh(catalog.clone()) {
            Ok(updated) => {
                control.check()?;
                save(cache, &updated)?;
                return Ok(updated);
            }
            Err(error) => {
                control.check()?;
                progress(
                    &format!(
                        "Could not check recent archive publications: {error:#}. Using saved dates."
                    ),
                    1.,
                );
                let mut fallback = catalog;
                fallback.basis.push_str(
                    " The recent publication check failed; newly published dates may be missing.",
                );
                return Ok(fallback);
            }
        }
    }
    progress("Using the verified provider date index", 1.);
    Ok(catalog)
}

fn merge_recent_releases(
    catalog: &mut Catalog,
    releases: &[serde_json::Value],
    year: i32,
    today: NaiveDate,
) {
    let mut dates: BTreeSet<_> = catalog.dates.iter().cloned().collect();
    dates.extend(
        releases
            .iter()
            .filter_map(|release| release_date(release, year, today)),
    );
    catalog.dates = dates.into_iter().collect();
}

async fn refresh_recent(
    mut catalog: Catalog,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Catalog> {
    control.require_network("check recent flight archive publications")?;
    let now = Utc::now();
    let mut fetch = Fetcher::new()?;
    for year in recent_years(now.date_naive()) {
        progress(
            &format!("Checking recent free archive publications: {year}"),
            0.,
        );
        let url = format!(
            "https://api.github.com/repos/adsblol/globe_history_{year}/releases?per_page=100&page=1"
        );
        if let Some(bytes) = fetch.get(&url, None, control).await? {
            let releases: Vec<serde_json::Value> = serde_json::from_slice(&bytes)?;
            merge_recent_releases(&mut catalog, &releases, year, now.date_naive());
        }
    }
    // Preserve the full-scan timestamp so frequent recent checks cannot postpone
    // discovery of older releases. Keep both sets of source receipts.
    catalog.recent_checked_utc = Some(now.to_rfc3339());
    catalog.sources.extend(fetch.sources);
    catalog.basis = "All release pages were read during full discovery. Recent publication checks add new dates and preserve the earlier list.".into();
    Ok(catalog)
}

struct Fetcher {
    client: reqwest::Client,
    sources: Vec<Source>,
}
impl Fetcher {
    fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent("soundgeo/0.3 (free flight date discovery)")
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(60))
                .build()?,
            sources: vec![],
        })
    }
    async fn get(
        &mut self,
        url: &str,
        form: Option<&str>,
        control: &Control,
    ) -> Result<Option<Vec<u8>>> {
        let request = if let Some(body) = form {
            self.client
                .post(url)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body.to_owned())
        } else {
            self.client.get(url)
        };
        let mut response = cancelable(request.send(), control).await??;
        let status = response.status();
        if status != reqwest::StatusCode::NOT_FOUND {
            response = response.error_for_status()?;
        }
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= MAX_RESPONSE as u64),
            "Provider date response is too large"
        );
        let mut bytes = Vec::new();
        while let Some(part) = cancelable(response.chunk(), control).await?? {
            ensure!(
                bytes.len().saturating_add(part.len()) <= MAX_RESPONSE,
                "Provider date response is too large"
            );
            bytes.extend_from_slice(&part);
        }
        self.sources.push(Source {
            url: url.into(),
            sha256: hash(&bytes),
            status: status.as_u16(),
            request_body_sha256: form.map(|s| hash(s.as_bytes())),
        });
        Ok((status != reqwest::StatusCode::NOT_FOUND).then_some(bytes))
    }
}
pub(crate) fn query(fields: &[(&str, &str)]) -> String {
    fn encode(value: &str) -> String {
        value
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    let mut fields: Vec<_> = fields.iter().map(|(k, v)| (encode(k), encode(v))).collect();
    fields.sort();
    fields
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn release_date(value: &serde_json::Value, year: i32, today: NaiveDate) -> Option<String> {
    if value["draft"].as_bool() == Some(true) || value["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let tag = value["tag_name"].as_str()?;
    let (date, instance) = tag.strip_prefix('v')?.split_once("-planes-readsb-")?;
    if !["prod-0", "prod-1", "staging-0"].contains(&instance) {
        return None;
    }
    let day = NaiveDate::parse_from_str(date, "%Y.%m.%d").ok()?;
    if day.year() != year || day >= today {
        return None;
    }
    let assets = value["assets"].as_array()?;
    if !assets.iter().any(|a| {
        a["name"]
            .as_str()
            .is_some_and(|n| n.ends_with(".tar") || n.ends_with(".tar.aa"))
            && a["size"].as_u64().is_some_and(|n| n > 0)
    }) {
        return None;
    }
    Some(day.to_string())
}

async fn discover_online(
    provider: ArchiveProvider,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Catalog> {
    let today = Utc::now().date_naive();
    let mut fetch = Fetcher::new()?;
    let mut dates = BTreeSet::new();
    let basis;
    match provider {
        ArchiveProvider::AdsbLol => {
            basis = "Published daily releases with a supported production or staging archive. All release pages were read.";
            for year in 2023..=today.year() {
                let mut seen = BTreeSet::new();
                for page in 1.. {
                    control.check()?;
                    progress(
                        &format!("Reading free daily archive dates: {year}, page {page}"),
                        0.,
                    );
                    let url = format!(
                        "https://api.github.com/repos/adsblol/globe_history_{year}/releases?per_page=100&page={page}"
                    );
                    let Some(bytes) = fetch.get(&url, None, control).await? else {
                        ensure!(
                            page == 1,
                            "An archive index page disappeared during discovery"
                        );
                        break;
                    };
                    let releases: Vec<serde_json::Value> = serde_json::from_slice(&bytes)?;
                    if releases.is_empty() {
                        break;
                    }
                    ensure!(
                        seen.insert(hash(&bytes)),
                        "Archive date pagination repeated a page"
                    );
                    for release in &releases {
                        if let Some(date) = release_date(release, year, today) {
                            dates.insert(date);
                        }
                    }
                    if releases.len() < 100 {
                        break;
                    }
                }
            }
        }
        ArchiveProvider::AdsbExchangeSample | ArchiveProvider::AdsbExchangeSampleHires => {
            basis = "First-of-month dates listed under both free sample snapshots and traces. Completeness is checked during download.";
            let client = crate::sample_archive::Client::new()?;
            let mut by_kind = vec![];
            for kind in [
                "readsb-hist",
                provider.sample_resolution().unwrap().object_prefix(),
            ] {
                let mut listed = BTreeSet::new();
                let root = format!("{kind}/");
                for year in prefixes(&client, &root, &mut fetch.sources, control).await? {
                    let Some(y) = year
                        .strip_prefix(&root)
                        .and_then(|s| s.strip_suffix('/'))
                        .and_then(|s| s.parse::<i32>().ok())
                    else {
                        continue;
                    };
                    if !(2016..=today.year()).contains(&y) {
                        continue;
                    }
                    progress(&format!("Reading free sample dates: {kind}, {y}"), 0.);
                    for month in prefixes(&client, &year, &mut fetch.sources, control).await? {
                        for day in prefixes(&client, &month, &mut fetch.sources, control).await? {
                            if let Some(date) =
                                day.strip_prefix(&root).and_then(|s| s.strip_suffix('/'))
                            {
                                let date = date.replace('/', "-");
                                if valid_date(&date, today) && date.ends_with("-01") {
                                    listed.insert(date);
                                }
                            }
                        }
                    }
                }
                by_kind.push(listed);
            }
            dates = by_kind[0].intersection(&by_kind[1]).cloned().collect();
        }
        ArchiveProvider::WebtrakChicago | ArchiveProvider::WebtrakLax => {
            basis = "Complete UTC days inside the public site's advertised replay period. Availability and every 30-second chunk are checked during download.";
            let site = if provider == ArchiveProvider::WebtrakChicago {
                "cda"
            } else {
                "lax4"
            };
            progress("Reading the airport's public replay period", 0.);
            let form = query(&[
                ("app", "focus"),
                ("product", "WebTrak"),
                ("sitename", site),
                ("action", "get_config"),
                ("send_all", "false"),
                ("exclude_layers", "true"),
            ]);
            let bytes = fetch
                .get(
                    "https://focus-apis.emsbk.com/productinfo",
                    Some(&form),
                    control,
                )
                .await?
                .context("Airport configuration is unavailable")?;
            dates = webtrak_dates(&serde_json::from_slice(&bytes)?, Utc::now())?
                .into_iter()
                .collect();
        }
    }
    let catalog = Catalog {
        provider,
        retrieved_utc: Utc::now().to_rfc3339(),
        recent_checked_utc: None,
        dates: dates.into_iter().collect(),
        basis: basis.into(),
        sources: fetch.sources,
    };
    validate(&catalog, provider, today)?;
    Ok(catalog)
}

fn webtrak_dates(value: &serde_json::Value, now: chrono::DateTime<Utc>) -> Result<Vec<String>> {
    ensure!(
        value.get("error").is_none(),
        "Airport date configuration returned an error"
    );
    let raw = &value["config"]["airport"]["data_period_days"];
    let days = raw
        .as_u64()
        .or_else(|| raw.as_str().and_then(|s| s.parse().ok()))
        .context("Airport did not advertise its replay period")?;
    ensure!((1..=36525).contains(&days), "Invalid airport replay period");
    let earliest = now
        .checked_sub_signed(chrono::Duration::days(days as i64))
        .context("Invalid airport replay date")?;
    // The first advertised day may already have expired in part. Only select
    // complete UTC days; the current UTC day is still being recorded.
    let first = earliest
        .date_naive()
        .checked_add_days(Days::new(1))
        .context("Invalid first airport day")?;
    Ok((0..(now.date_naive() - first).num_days())
        .map(|i| (first + Days::new(i as u64)).to_string())
        .collect())
}

#[derive(Deserialize)]
struct Prefix {
    #[serde(rename = "Prefix")]
    value: String,
}
#[derive(Deserialize)]
struct Listing {
    #[serde(rename = "IsTruncated")]
    truncated: bool,
    #[serde(rename = "NextContinuationToken")]
    next: Option<String>,
    #[serde(rename = "CommonPrefixes", default)]
    prefixes: Vec<Prefix>,
}
async fn prefixes(
    client: &crate::sample_archive::Client,
    prefix: &str,
    sources: &mut Vec<Source>,
    control: &Control,
) -> Result<Vec<String>> {
    let mut next = None;
    let mut seen = BTreeSet::new();
    let mut result = BTreeSet::new();
    loop {
        let (url, bytes) = client.listing(prefix, next.as_deref(), control).await?;
        let value: Listing = quick_xml::de::from_reader(bytes.as_slice())?;
        sources.push(Source {
            url,
            sha256: hash(&bytes),
            status: 200,
            request_body_sha256: None,
        });
        for child in value.prefixes {
            let rest = child
                .value
                .strip_prefix(prefix)
                .context("Sample index returned an unrelated prefix")?;
            ensure!(
                rest.ends_with('/') && rest.len() > 1 && !rest[..rest.len() - 1].contains('/'),
                "Sample index returned an invalid child prefix"
            );
            result.insert(child.value);
        }
        if !value.truncated {
            break;
        }
        let token = value
            .next
            .context("Truncated sample index has no next page")?;
        ensure!(
            !token.is_empty() && seen.insert(token.clone()),
            "Sample date pagination repeated a token"
        );
        next = Some(token);
    }
    Ok(result.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_catalog_is_verified_and_can_be_read_with_networking_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let provider = ArchiveProvider::WebtrakChicago;
        let catalog = Catalog {
            provider,
            retrieved_utc: Utc::now().to_rfc3339(),
            recent_checked_utc: None,
            dates: vec!["2024-02-29".into(), "2024-03-01".into()],
            basis: "fixture".into(),
            sources: vec![Source {
                url: "https://example.com/index".into(),
                sha256: hash(b"fixture"),
                status: 200,
                request_body_sha256: None,
            }],
        };
        save(dir.path(), &catalog).unwrap();
        let control = Control::default().with_offline(true);
        assert_eq!(
            discover(provider, dir.path(), &control, &|_, _| {})
                .unwrap()
                .dates,
            catalog.dates
        );
        let path = cache_path(dir.path(), provider);
        let body = std::fs::read_to_string(&path)
            .unwrap()
            .replace("2024-03-01", "2024-03-02");
        std::fs::write(&path, body).unwrap();
        assert!(discover(provider, dir.path(), &control, &|_, _| {}).is_err());
        save(dir.path(), &catalog).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        assert!(load(dir.path(), ArchiveProvider::WebtrakLax).is_err());
        control.stop();
        assert!(discover(provider, dir.path(), &control, &|_, _| {}).is_err());
    }
    #[test]
    fn high_resolution_date_catalog_preserves_provider_and_monthly_coverage() {
        let dir = tempfile::tempdir().unwrap();
        let provider = ArchiveProvider::AdsbExchangeSampleHires;
        let mut catalog = Catalog {
            provider,
            retrieved_utc: Utc::now().to_rfc3339(),
            recent_checked_utc: None,
            dates: vec!["2024-03-01".into()],
            basis: "fixture".into(),
            sources: vec![Source {
                url: "https://example.com/hires-traces/".into(),
                sha256: hash(b"fixture"),
                status: 200,
                request_body_sha256: None,
            }],
        };
        save(dir.path(), &catalog).unwrap();
        let offline = Control::default().with_offline(true);
        assert_eq!(
            discover(provider, dir.path(), &offline, &|_, _| {})
                .unwrap()
                .dates,
            catalog.dates
        );
        assert!(load(dir.path(), ArchiveProvider::AdsbExchangeSample).is_err());
        catalog.dates = vec!["2024-03-02".into()];
        assert!(save(dir.path(), &catalog).is_err());
        assert_eq!(load(dir.path(), provider).unwrap().dates, ["2024-03-01"]);
    }

    #[test]
    fn release_index_recognizes_supported_complete_day_archives() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 7).unwrap();
        let mut value = serde_json::json!({"tag_name":"v2026.11.06-planes-readsb-prod-0","assets":[{"name":"day.tar.aa","size":10}]});
        assert_eq!(
            release_date(&value, 2026, today).as_deref(),
            Some("2026-11-06")
        );
        value["assets"][0]["name"] = "day.tar".into();
        assert!(release_date(&value, 2026, today).is_some());
        value["tag_name"] = "v2026.11.06-planes-readsb-mlatonly-0".into();
        assert!(release_date(&value, 2026, today).is_none());
        value["tag_name"] = "v2026.11.07-planes-readsb-prod-0".into();
        assert!(release_date(&value, 2026, today).is_none());
        value["tag_name"] = "v2026.11.06-planes-readsb-prod-0".into();
        value["draft"] = true.into();
        assert!(release_date(&value, 2026, today).is_none());
    }
    #[test]
    fn airport_period_excludes_partial_days_and_handles_leap_years() {
        let value = serde_json::json!({"config":{"airport":{"data_period_days":"90"}}});
        let now = chrono::DateTime::parse_from_rfc3339("2024-03-02T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let dates = webtrak_dates(&value, now).unwrap();
        assert_eq!(dates.len(), 89);
        assert_eq!(dates.last().unwrap(), "2024-03-01");
        assert!(dates.contains(&"2024-02-29".into()));
        assert!(webtrak_dates(&serde_json::json!({"error":"unavailable"}), now).is_err());
        assert!(webtrak_dates(&serde_json::json!({}), now).is_err());
    }
    #[test]
    fn s3_query_encoding_sorts_fields_and_preserves_opaque_tokens() {
        assert_eq!(
            query(&[
                ("prefix", "traces/2026/"),
                ("continuation-token", "a+b /="),
                ("delimiter", "/")
            ]),
            "continuation-token=a%2Bb%20%2F%3D&delimiter=%2F&prefix=traces%2F2026%2F"
        );
    }
    fn daily_fixture(now: chrono::DateTime<Utc>) -> Catalog {
        Catalog {
            provider: ArchiveProvider::AdsbLol,
            retrieved_utc: (now - chrono::Duration::hours(1)).to_rfc3339(),
            recent_checked_utc: None,
            dates: vec!["2024-03-01".into()],
            basis: "fixture".into(),
            sources: vec![Source {
                url: "https://example.com/original".into(),
                sha256: hash(b"original"),
                status: 200,
                request_body_sha256: None,
            }],
        }
    }

    #[test]
    fn recent_dates_extend_legacy_index_without_postponing_full_discovery() {
        let now = Utc::now();
        let dir = tempfile::tempdir().unwrap();
        let catalog = daily_fixture(now);
        let legacy_bytes = serde_json::to_vec(&catalog).unwrap();
        assert!(!String::from_utf8_lossy(&legacy_bytes).contains("recent_checked_utc"));
        save(dir.path(), &catalog).unwrap();
        let old = load(dir.path(), catalog.provider).unwrap();
        assert_eq!(serde_json::to_vec(&old).unwrap(), legacy_bytes);
        let day = now.date_naive().pred_opt().unwrap();
        let added = serde_json::json!({"tag_name":format!("v{}-planes-readsb-staging-0", day.format("%Y.%m.%d")),
            "assets":[{"name":"day.tar","size":10}]});
        let updated = refresh_cached(
            old,
            dir.path(),
            &Control::default(),
            &|_, _| {},
            now,
            |mut value| {
                merge_recent_releases(
                    &mut value,
                    &[added.clone(), added],
                    day.year(),
                    now.date_naive(),
                );
                value.recent_checked_utc = Some(now.to_rfc3339());
                value.sources.push(Source {
                    url: "https://example.com/recent".into(),
                    sha256: hash(b"recent"),
                    status: 200,
                    request_body_sha256: None,
                });
                Ok(value)
            },
        )
        .unwrap();
        assert_eq!(updated.dates, ["2024-03-01".to_owned(), day.to_string()]);
        assert_eq!(updated.retrieved_utc, catalog.retrieved_utc);
        assert_eq!(updated.sources.len(), 2);
        assert_eq!(
            load(dir.path(), catalog.provider).unwrap().dates,
            updated.dates
        );
        assert!(!recent_refresh_due(&updated, now + chrono::Duration::seconds(899)).unwrap());
        assert!(recent_refresh_due(&updated, now + chrono::Duration::seconds(900)).unwrap());
        let cached = refresh_cached(
            updated,
            dir.path(),
            &Control::default(),
            &|_, _| {},
            now,
            |_| panic!("Fresh catalog must avoid another request"),
        )
        .unwrap();
        assert_eq!(cached.dates.len(), 2);
    }

    #[test]
    fn recent_date_failures_and_offline_reads_preserve_saved_bytes() {
        let now = Utc::now();
        let dir = tempfile::tempdir().unwrap();
        let catalog = daily_fixture(now);
        save(dir.path(), &catalog).unwrap();
        let path = cache_path(dir.path(), catalog.provider);
        let before = std::fs::read(&path).unwrap();
        let fallback = refresh_cached(
            catalog.clone(),
            dir.path(),
            &Control::default(),
            &|_, _| {},
            now,
            |_| anyhow::bail!("fixture outage"),
        )
        .unwrap();
        assert_eq!(fallback.dates, catalog.dates);
        assert!(fallback.basis.contains("publication check failed"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let offline = Control::default().with_offline(true);
        let saved = discover(catalog.provider, dir.path(), &offline, &|_, _| {}).unwrap();
        assert_eq!(saved.dates, catalog.dates);
        refresh_cached(
            catalog.clone(),
            dir.path(),
            &offline,
            &|_, _| {},
            now,
            |_| panic!("Offline must not probe publications"),
        )
        .unwrap();
        let cancel = Control::default();
        let result = refresh_cached(catalog, dir.path(), &cancel, &|_, _| {}, now, |_| {
            cancel.stop();
            anyhow::bail!("cancelled during request")
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn recent_checks_cover_year_boundary_and_validate_timestamps() {
        assert_eq!(
            recent_years(NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()),
            BTreeSet::from([2026])
        );
        assert_eq!(
            recent_years(NaiveDate::from_ymd_opt(2027, 1, 2).unwrap()),
            BTreeSet::from([2026, 2027])
        );
        assert_eq!(
            recent_years(NaiveDate::from_ymd_opt(2027, 1, 3).unwrap()),
            BTreeSet::from([2027])
        );
        let now = Utc::now();
        let mut catalog = daily_fixture(now);
        catalog.recent_checked_utc = Some((now + chrono::Duration::hours(1)).to_rfc3339());
        assert!(validate(&catalog, catalog.provider, now.date_naive()).is_err());
        catalog.recent_checked_utc = Some((now - chrono::Duration::hours(2)).to_rfc3339());
        assert!(validate(&catalog, catalog.provider, now.date_naive()).is_err());
        catalog.recent_checked_utc = Some(now.to_rfc3339());
        validate(&catalog, catalog.provider, now.date_naive()).unwrap();
        catalog.provider = ArchiveProvider::AdsbExchangeSampleHires;
        assert!(!recent_refresh_due(&catalog, now + chrono::Duration::hours(2)).unwrap());
    }
}
