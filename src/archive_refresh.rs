//! Upgrade cached fallback releases without replacing a usable archive in place.
use crate::{Control, Progress};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const INSTANCES: [&str; 3] = ["prod-0", "prod-1", "staging-0"];
const MAX_MANIFEST: u64 = 4_000_000;

fn read_json(path: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_MANIFEST,
        "Archive metadata exceeds 4 MB"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_json(path: &Path, value: &Value) -> Result<()> {
    let (mut file, destination) = crate::atomic_file::prepare(path)?;
    serde_json::to_writer_pretty(file.as_file_mut(), value)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(destination)?;
    Ok(())
}

fn rank(release: &Value, prefix: &str) -> Result<usize> {
    INSTANCES
        .iter()
        .position(|instance| release["tag_name"] == format!("{prefix}{instance}"))
        .context("Unrecognized daily archive release")
}

// Use only immutable download fields. Download counts and release text can change.
fn release_key(release: &Value) -> Result<String> {
    let tag = release["tag_name"]
        .as_str()
        .context("Missing release tag")?;
    ensure!(
        release["draft"] != true && release["prerelease"] != true,
        "Archive release is not published"
    );
    let mut assets: Vec<Value> = release["assets"].as_array().context("Archive has no assets")?.iter()
        .filter(|a| a["name"].as_str().is_some_and(|n| n.ends_with(".tar") || n.contains(".tar.")))
        .map(|a| json!({"name":a["name"],"size":a["size"],"digest":a["digest"],"url":a["browser_download_url"]})).collect();
    assets.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    ensure!(
        !assets.is_empty() && assets.len() <= 676,
        "Invalid archive part count"
    );
    let unsplit = assets.len() == 1 && assets[0]["name"] == format!("{tag}.tar");
    for (i, a) in assets.iter().enumerate() {
        let expected = format!(
            "{tag}.tar.{}{}",
            (b'a' + (i / 26) as u8) as char,
            (b'a' + (i % 26) as u8) as char
        );
        ensure!(
            unsplit || a["name"] == expected,
            "Archive parts are not a complete ordered sequence"
        );
        ensure!(
            a["size"].as_u64().is_some_and(|size| size > 0),
            "Invalid archive part size"
        );
        let digest = a["digest"]
            .as_str()
            .and_then(|s| s.strip_prefix("sha256:"))
            .context("Preferred archive needs published SHA-256 checksums")?;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid archive checksum"
        );
        ensure!(
            a["url"]
                .as_str()
                .is_some_and(|url| url.starts_with("https://github.com/adsblol/")),
            "Unexpected archive host"
        );
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(tag, assets))?)
    ))
}

fn selected_cache(cache: &Path, date: &str, prefix: &str) -> Result<PathBuf> {
    let marker = cache.join(date).join("preferred-release.json");
    if !marker.is_file() {
        return Ok(cache.into());
    }
    let selected = read_json(&marker)?;
    let key = selected["release_key"]
        .as_str()
        .context("Missing preferred release key")?;
    ensure!(
        selected["format"] == 1 && key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid preferred release marker"
    );
    let root = cache.join(date).join(".releases").join(key);
    let release = read_json(&root.join(date).join("archive-manifest.json"))?;
    ensure!(
        release_key(&release)? == key && rank(&release, prefix)? <= 2,
        "Preferred archive metadata changed"
    );
    Ok(root)
}

pub(crate) fn download_day(
    date: &str,
    cache: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<PathBuf> {
    let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    let prepare = |root: &Path| crate::data::download_day(date, root, control, progress);
    ensure_with(
        date,
        cache,
        control,
        progress,
        chrono::Utc::now().timestamp(),
        |tag| {
            control.require_network("preferred daily archive")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let client = reqwest::Client::builder()
                    .user_agent("soundgeo/0.3 (historical flight research)")
                    .connect_timeout(Duration::from_secs(20))
                    .timeout(Duration::from_secs(30))
                    .build()?;
                let url = format!(
                    "https://api.github.com/repos/adsblol/globe_history_{}/releases/tags/{tag}",
                    day.format("%Y")
                );
                let response =
                    crate::network::cancelable(client.get(url).send(), control).await??;
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                let mut response = response.error_for_status()?;
                let mut bytes = Vec::new();
                while let Some(chunk) =
                    crate::network::cancelable(response.chunk(), control).await??
                {
                    ensure!(
                        bytes.len() + chunk.len() <= MAX_MANIFEST as usize,
                        "Archive metadata exceeds 4 MB"
                    );
                    bytes.extend_from_slice(&chunk);
                }
                Ok(Some(serde_json::from_slice(&bytes)?))
            })
        },
        prepare,
    )
}

fn ensure_with(
    date: &str,
    cache: &Path,
    control: &Control,
    progress: &Progress<'_>,
    now: i64,
    mut lookup: impl FnMut(&str) -> Result<Option<Value>>,
    mut prepare: impl FnMut(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    control.check()?;
    let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    let prefix = format!("v{}-planes-readsb-", day.format("%Y.%m.%d"));
    let selected = selected_cache(cache, date, &prefix)?;
    let existing = prepare(&selected)?;
    let release = read_json(&selected.join(date).join("archive-manifest.json"))?;
    let priority = rank(&release, &prefix)?;
    let completeness = verify_complete_archive(&selected.join(date), &release, control);
    control.check()?;
    if control.offline {
        completeness.context("Cached flight archive is incomplete; prepare it online")?;
        return Ok(existing);
    }
    if priority == 0 && completeness.is_ok() {
        return Ok(existing);
    }
    let check_path = cache.join(date).join("preferred-check.json");
    let recent = read_json(&check_path).ok().is_some_and(|v| {
        v["tag_name"] == release["tag_name"]
            && v["checked_unix"]
                .as_i64()
                .is_some_and(|stamp| (0..900).contains(&now.saturating_sub(stamp)))
    });
    if recent {
        completeness.context(
            "Cached flight archive is incomplete; the next release check is due within 15 minutes",
        )?;
        return Ok(existing);
    }
    progress("Checking for a preferred daily archive", 0.);
    let attempt = (|| -> Result<Option<PathBuf>> {
        // A release can be visible before its final asset finishes uploading.
        // Re-read the same release when the saved parts do not form a complete tar.
        let count = priority + usize::from(completeness.is_err());
        for instance in &INSTANCES[..count] {
            control.check()?;
            let tag = format!("{prefix}{instance}");
            let Some(candidate) = lookup(&tag)? else {
                continue;
            };
            ensure!(
                candidate["tag_name"] == tag,
                "Preferred release returned the wrong tag"
            );
            let key = release_key(&candidate)?;
            let root = cache.join(date).join(".releases").join(&key);
            let folder = root.join(date);
            std::fs::create_dir_all(&folder)?;
            save_json(&folder.join("archive-manifest.json"), &candidate)?;
            // Keep the same type catalog when comparing releases for this date.
            std::fs::copy(
                selected.join(date).join("aircraft-types.json"),
                folder.join("aircraft-types.json"),
            )?;
            copy_verified_parts(&selected.join(date), &release, &folder, &candidate, control)?;
            let path = prepare(&root)?;
            verify_complete_archive(&folder, &candidate, control)?;
            control.check()?;
            save_json(
                &cache.join(date).join("preferred-release.json"),
                &json!({"format":1,"release_key":key}),
            )?;
            progress(
                "Selected the verified preferred daily archive; previous files retained",
                1.,
            );
            return Ok(Some(path));
        }
        Ok(None)
    })();
    control.check()?;
    // Failed checks are bounded too; the saved archive remains available offline.
    save_json(
        &check_path,
        &json!({"tag_name":release["tag_name"],"checked_unix":now}),
    )?;
    match attempt {
        Ok(Some(path)) => Ok(path),
        Ok(None) => {
            completeness.context(
                "Cached flight archive is incomplete and no complete release is available",
            )?;
            Ok(existing)
        }
        Err(error) => {
            if let Err(incomplete) = completeness {
                return Err(incomplete).context(format!(
                    "Could not complete the saved flight archive: {error:#}"
                ));
            }
            progress(
                &format!(
                    "Preferred archive unavailable ({error:#}); using the verified saved release"
                ),
                1.,
            );
            Ok(existing)
        }
    }
}

fn copy_verified_parts(
    source: &Path,
    saved: &Value,
    destination: &Path,
    candidate: &Value,
    control: &Control,
) -> Result<()> {
    let previous = saved["assets"]
        .as_array()
        .context("Missing saved archive assets")?;
    for asset in candidate["assets"]
        .as_array()
        .context("Missing candidate assets")?
    {
        control.check()?;
        let Some(name) = asset["name"]
            .as_str()
            .filter(|n| n.ends_with(".tar") || n.contains(".tar."))
        else {
            continue;
        };
        let target = destination.join(name);
        if !target.exists()
            && previous.iter().any(|old| {
                ["name", "size", "digest", "browser_download_url"]
                    .iter()
                    .all(|field| old[field] == asset[field])
            })
        {
            // The caller has already verified the saved parts. The downloader
            // verifies the copied bytes again before accepting the replacement.
            std::fs::copy(source.join(name), target)?;
        }
    }
    control.check()
}

fn verify_complete_archive(folder: &Path, release: &Value, control: &Control) -> Result<()> {
    // Part hashes are checked by download_day on every call. This receipt binds
    // the structural check to that exact saved manifest, including all part hashes.
    let key = format!("{:x}", Sha256::digest(serde_json::to_vec(release)?));
    let path = folder.join("archive-complete.json");
    if read_json(&path)
        .ok()
        .is_some_and(|v| v["format"] == 1 && v["manifest_sha256"] == key)
    {
        return Ok(());
    }
    verify_tar(folder, release, control)?;
    control.check()?;
    save_json(&path, &json!({"format":1,"manifest_sha256":key}))
}

fn verify_tar(folder: &Path, release: &Value, control: &Control) -> Result<()> {
    let mut names: Vec<_> = release["assets"]
        .as_array()
        .context("Missing archive assets")?
        .iter()
        .filter_map(|a| a["name"].as_str())
        .filter(|n| n.ends_with(".tar") || n.contains(".tar."))
        .collect();
    names.sort();
    let mut reader: Box<dyn Read> = Box::new(std::io::empty());
    for name in names {
        reader = Box::new(reader.chain(File::open(folder.join(name))?));
    }
    struct Checked<'a> {
        reader: Box<dyn Read>,
        control: &'a Control,
    }
    impl Read for Checked<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.control.check().map_err(std::io::Error::other)?;
            self.reader.read(bytes)
        }
    }
    let mut archive = tar::Archive::new(Checked { reader, control });
    let mut entries = 0;
    for entry in archive.entries()? {
        std::io::copy(&mut entry?, &mut std::io::sink())?;
        entries += 1;
    }
    ensure!(entries > 0, "Flight archive is empty");
    // tar's iterator also stops at physical EOF. Require the second end block
    // after the first zero header so a split at a member boundary cannot pass.
    let mut end_block = [1u8; 512];
    archive
        .into_inner()
        .read_exact(&mut end_block)
        .context("Flight archive has no complete end marker")?;
    ensure!(
        end_block.iter().all(|b| *b == 0),
        "Invalid flight archive end marker"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const DATE: &str = "2026-11-09";

    fn fixture(instance: &str, bytes: &[u8]) -> Value {
        let tag = format!("v2026.11.09-planes-readsb-{instance}");
        let pieces = bytes.chunks(bytes.len().div_ceil(2));
        let assets: Vec<_> = pieces.enumerate().map(|(i, piece)| {
            let name = format!("{tag}.tar.a{}", (b'a' + i as u8) as char);
            json!({"name":name,"size":piece.len(),"digest":format!("sha256:{:x}",Sha256::digest(piece)),
                "browser_download_url":format!("https://github.com/adsblol/globe_history_2026/releases/download/{tag}/{name}")})
        }).collect();
        json!({"tag_name":tag,"assets":assets})
    }

    fn archive_bytes() -> Vec<u8> {
        let body = b"reference fixture";
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "trace.json", &body[..])
            .unwrap();
        builder.into_inner().unwrap()
    }

    fn seed(root: &Path, release: &Value, bytes: &[u8], parts: usize) {
        let dir = root.join(DATE);
        std::fs::create_dir_all(&dir).unwrap();
        save_json(&dir.join("archive-manifest.json"), release).unwrap();
        std::fs::write(dir.join("aircraft-types.json"), b"{}").unwrap();
        for (asset, piece) in release["assets"]
            .as_array()
            .unwrap()
            .iter()
            .zip(bytes.chunks(bytes.len().div_ceil(2)))
            .take(parts)
        {
            std::fs::write(dir.join(asset["name"].as_str().unwrap()), piece).unwrap();
        }
    }

    fn prepare(root: &Path) -> Result<PathBuf> {
        crate::data::download_day(
            DATE,
            root,
            &Control::default().with_offline(true),
            &|_, _| {},
        )
    }

    #[test]
    fn preferred_release_promotes_only_verified_parts_and_reuses_them_offline() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        let staging = fixture("staging-0", &bytes);
        let production = fixture("prod-0", &bytes);
        seed(temp.path(), &staging, &bytes, 2);
        let original = std::fs::read(temp.path().join(DATE).join("archive-manifest.json")).unwrap();
        let mut requests = vec![];
        let path = ensure_with(
            DATE,
            temp.path(),
            &Control::default(),
            &|_, _| {},
            1000,
            |tag| {
                requests.push(tag.to_owned());
                Ok(Some(production.clone()))
            },
            |root| {
                if root != temp.path() {
                    seed(root, &production, &bytes, 2);
                }
                prepare(root)
            },
        )
        .unwrap();
        assert_eq!(requests, [production["tag_name"].as_str().unwrap()]);
        assert!(path.starts_with(temp.path().join(DATE).join(".releases")));
        assert_eq!(
            std::fs::read(temp.path().join(DATE).join("archive-manifest.json")).unwrap(),
            original
        );
        assert!(prepare(temp.path()).unwrap().is_file());
        for offline in [true, false] {
            let reused = ensure_with(
                DATE,
                temp.path(),
                &Control::default().with_offline(offline),
                &|_, _| {},
                2000,
                |_| panic!("preferred production archive must be reused"),
                prepare,
            )
            .unwrap();
            assert_eq!(reused, path);
        }
    }

    #[test]
    fn fallback_checks_are_bounded_and_future_timestamps_do_not_suppress_them() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        let staging = fixture("staging-0", &bytes);
        seed(temp.path(), &staging, &bytes, 2);
        let mut calls = 0;
        for now in [1000, 1001, 1899, 1900, 500] {
            ensure_with(
                DATE,
                temp.path(),
                &Control::default(),
                &|_, _| {},
                now,
                |_| {
                    calls += 1;
                    Ok(None)
                },
                prepare,
            )
            .unwrap();
        }
        assert_eq!(calls, 6);
        ensure_with(
            DATE,
            temp.path(),
            &Control::default().with_offline(true),
            &|_, _| {},
            5000,
            |_| panic!("offline must not query releases"),
            prepare,
        )
        .unwrap();
    }

    #[test]
    fn partial_upgrade_retains_old_archive_and_resumes_after_backoff() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        let staging = fixture("staging-0", &bytes);
        let production = fixture("prod-0", &bytes);
        seed(temp.path(), &staging, &bytes, 2);
        let old = prepare(temp.path()).unwrap();
        let pending = temp
            .path()
            .join(DATE)
            .join(".releases")
            .join(release_key(&production).unwrap());
        seed(&pending, &production, &bytes, 1);
        let got = ensure_with(
            DATE,
            temp.path(),
            &Control::default(),
            &|_, _| {},
            1000,
            |_| Ok(Some(production.clone())),
            prepare,
        )
        .unwrap();
        assert_eq!(got, old);
        assert!(
            !temp
                .path()
                .join(DATE)
                .join("preferred-release.json")
                .exists()
        );
        assert_eq!(prepare(temp.path()).unwrap(), old);
        let first = pending
            .join(DATE)
            .join(production["assets"][0]["name"].as_str().unwrap());
        let before = std::fs::read(&first).unwrap();
        let second = pending
            .join(DATE)
            .join(production["assets"][1]["name"].as_str().unwrap());
        std::fs::write(second, &bytes[bytes.len().div_ceil(2)..]).unwrap();
        let got = ensure_with(
            DATE,
            temp.path(),
            &Control::default(),
            &|_, _| {},
            1900,
            |_| Ok(Some(production.clone())),
            prepare,
        )
        .unwrap();
        assert_eq!(got, first);
        assert_eq!(std::fs::read(first).unwrap(), before);
    }

    #[test]
    fn malformed_or_hash_damaged_replacement_is_not_selected() {
        for malformed_tar in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let bytes = archive_bytes();
            seed(temp.path(), &fixture("staging-0", &bytes), &bytes, 2);
            let old = prepare(temp.path()).unwrap();
            let mut candidate_bytes = if malformed_tar {
                vec![b'x'; 2048]
            } else {
                bytes.clone()
            };
            let production = fixture("prod-0", &candidate_bytes);
            if !malformed_tar {
                candidate_bytes[0] ^= 1;
            }
            let got = ensure_with(
                DATE,
                temp.path(),
                &Control::default(),
                &|_, _| {},
                1000,
                |_| Ok(Some(production.clone())),
                |root| {
                    if root != temp.path() {
                        seed(root, &production, &candidate_bytes, 2);
                    }
                    prepare(root)
                },
            )
            .unwrap();
            assert_eq!(got, old);
            assert!(
                !temp
                    .path()
                    .join(DATE)
                    .join("preferred-release.json")
                    .exists()
            );
        }
    }

    #[test]
    fn lookup_failure_uses_cache_but_cancellation_propagates() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        seed(temp.path(), &fixture("staging-0", &bytes), &bytes, 2);
        let old = prepare(temp.path()).unwrap();
        assert_eq!(
            ensure_with(
                DATE,
                temp.path(),
                &Control::default(),
                &|_, _| {},
                1000,
                |_| anyhow::bail!("rate limited"),
                prepare
            )
            .unwrap(),
            old
        );
        let control = Control::default();
        let production = fixture("prod-0", &bytes);
        let error = ensure_with(
            DATE,
            temp.path(),
            &control,
            &|_, _| {},
            1900,
            |_| Ok(Some(production.clone())),
            |root| {
                if root != temp.path() {
                    control.stop();
                    anyhow::bail!("Cancelled");
                }
                prepare(root)
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("Cancelled"));
        assert!(
            !temp
                .path()
                .join(DATE)
                .join("preferred-release.json")
                .exists()
        );
        assert_eq!(prepare(temp.path()).unwrap(), old);
    }

    #[test]
    fn production_one_is_upgraded_when_production_zero_appears() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        seed(temp.path(), &fixture("prod-1", &bytes), &bytes, 2);
        let production = fixture("prod-0", &bytes);
        ensure_with(
            DATE,
            temp.path(),
            &Control::default(),
            &|_, _| {},
            1000,
            |tag| {
                assert_eq!(tag, production["tag_name"]);
                Ok(Some(production.clone()))
            },
            |root| {
                if root != temp.path() {
                    seed(root, &production, &bytes, 2);
                }
                prepare(root)
            },
        )
        .unwrap();
        assert!(
            temp.path()
                .join(DATE)
                .join("preferred-release.json")
                .is_file()
        );
    }

    #[test]
    fn changed_or_unsafe_release_metadata_is_rejected() {
        let bytes = archive_bytes();
        let release = fixture("prod-0", &bytes);
        for field in ["name", "digest", "size", "browser_download_url"] {
            let mut bad = release.clone();
            bad["assets"][0][field] = "../bad".into();
            assert!(release_key(&bad).is_err(), "{field}");
        }
        let temp = tempfile::tempdir().unwrap();
        seed(temp.path(), &fixture("staging-0", &bytes), &bytes, 2);
        save_json(
            &temp.path().join(DATE).join("preferred-release.json"),
            &json!({"format":1,"release_key":"../outside"}),
        )
        .unwrap();
        assert!(
            ensure_with(
                DATE,
                temp.path(),
                &Control::default().with_offline(true),
                &|_, _| {},
                1000,
                |_| panic!("invalid marker must fail before lookup"),
                prepare
            )
            .is_err()
        );
    }

    #[test]
    fn partial_current_release_fetches_new_manifest_and_reuses_existing_part() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        let full = fixture("prod-0", &bytes);
        let mut partial = full.clone();
        partial["assets"].as_array_mut().unwrap().truncate(1);
        seed(temp.path(), &partial, &bytes, 1);
        assert!(
            ensure_with(
                DATE,
                temp.path(),
                &Control::default().with_offline(true),
                &|_, _| {},
                1000,
                |_| panic!("offline must not fetch metadata"),
                prepare
            )
            .is_err()
        );
        let original = std::fs::read(temp.path().join(DATE).join("archive-manifest.json")).unwrap();
        let result = ensure_with(
            DATE,
            temp.path(),
            &Control::default(),
            &|_, _| {},
            1000,
            |tag| {
                assert_eq!(tag, full["tag_name"]);
                Ok(Some(full.clone()))
            },
            |root| {
                if root != temp.path() {
                    let folder = root.join(DATE);
                    let first = folder.join(full["assets"][0]["name"].as_str().unwrap());
                    assert_eq!(
                        std::fs::read(first).unwrap(),
                        bytes[..bytes.len().div_ceil(2)]
                    );
                    let second = folder.join(full["assets"][1]["name"].as_str().unwrap());
                    assert!(!second.exists());
                    std::fs::write(second, &bytes[bytes.len().div_ceil(2)..]).unwrap();
                }
                prepare(root)
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read(temp.path().join(DATE).join("archive-manifest.json")).unwrap(),
            original
        );
        assert!(
            result
                .parent()
                .unwrap()
                .join("archive-complete.json")
                .exists()
        );
        assert_eq!(
            ensure_with(
                DATE,
                temp.path(),
                &Control::default().with_offline(true),
                &|_, _| {},
                2000,
                |_| panic!("offline must reuse the completed release"),
                prepare
            )
            .unwrap(),
            result
        );
    }

    #[test]
    fn incomplete_current_release_never_falls_back_to_success() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        let mut partial = fixture("prod-0", &bytes);
        partial["assets"].as_array_mut().unwrap().truncate(1);
        seed(temp.path(), &partial, &bytes, 1);
        let error = ensure_with(
            DATE,
            temp.path(),
            &Control::default(),
            &|_, _| {},
            1000,
            |_| anyhow::bail!("provider unavailable"),
            prepare,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Could not complete"));
        assert!(
            ensure_with(
                DATE,
                temp.path(),
                &Control::default(),
                &|_, _| {},
                1001,
                |_| panic!("honor the failed lookup backoff"),
                prepare
            )
            .is_err()
        );
        assert!(
            !temp
                .path()
                .join(DATE)
                .join("archive-complete.json")
                .exists()
        );
        assert!(
            !temp
                .path()
                .join(DATE)
                .join("preferred-release.json")
                .exists()
        );
    }

    #[test]
    fn incomplete_staging_release_can_refresh_its_own_parts() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        let full = fixture("staging-0", &bytes);
        let mut partial = full.clone();
        partial["assets"].as_array_mut().unwrap().truncate(1);
        seed(temp.path(), &partial, &bytes, 1);
        let mut requests = vec![];
        ensure_with(
            DATE,
            temp.path(),
            &Control::default(),
            &|_, _| {},
            1000,
            |tag| {
                requests.push(tag.to_owned());
                Ok((tag == full["tag_name"]).then(|| full.clone()))
            },
            |root| {
                if root != temp.path() {
                    std::fs::write(
                        root.join(DATE)
                            .join(full["assets"][1]["name"].as_str().unwrap()),
                        &bytes[bytes.len().div_ceil(2)..],
                    )
                    .unwrap();
                }
                prepare(root)
            },
        )
        .unwrap();
        assert_eq!(requests.len(), 3);
        ensure_with(
            DATE,
            temp.path(),
            &Control::default().with_offline(true),
            &|_, _| {},
            2000,
            |_| panic!("offline"),
            prepare,
        )
        .unwrap();
    }

    #[test]
    fn tar_completion_requires_both_zero_end_blocks() {
        for removed in [512, 1024] {
            let temp = tempfile::tempdir().unwrap();
            let mut bytes = archive_bytes();
            bytes.truncate(bytes.len() - removed);
            let release = fixture("prod-0", &bytes);
            seed(temp.path(), &release, &bytes, 2);
            assert!(
                verify_complete_archive(&temp.path().join(DATE), &release, &Control::default())
                    .is_err()
            );
        }
    }

    #[test]
    fn completion_receipt_does_not_bypass_part_integrity_checks() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = archive_bytes();
        let release = fixture("prod-0", &bytes);
        seed(temp.path(), &release, &bytes, 2);
        let control = Control::default().with_offline(true);
        ensure_with(
            DATE,
            temp.path(),
            &control,
            &|_, _| {},
            1000,
            |_| panic!("offline"),
            prepare,
        )
        .unwrap();
        assert!(
            temp.path()
                .join(DATE)
                .join("archive-complete.json")
                .is_file()
        );
        let part = temp
            .path()
            .join(DATE)
            .join(release["assets"][0]["name"].as_str().unwrap());
        let mut damaged = std::fs::read(&part).unwrap();
        damaged[0] ^= 1;
        std::fs::write(part, damaged).unwrap();
        assert!(
            ensure_with(
                DATE,
                temp.path(),
                &control,
                &|_, _| {},
                1001,
                |_| panic!("offline"),
                prepare
            )
            .is_err()
        );
    }
}
