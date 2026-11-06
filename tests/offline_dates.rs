use chrono::{Days, NaiveDate};
use sha2::{Digest, Sha256};
use soundgeo::{Control, Job, Report, demo, geo::Point};
use std::{path::Path, process::Command};

fn cache_day(root: &Path, date: &str, csv: &[u8]) {
    let folder = root.join(format!("webtrak-cda-{date}"));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("flights.csv"), csv).unwrap();
    let manifest = serde_json::json!({"format":1,"date":date,"provider":"webtrak_chicago","complete":true,
        "chunks":2880,"rows":csv.split(|b|*b==b'\n').count()-1,"offline_chunks":0,
        "csv_sha256":format!("{:x}",Sha256::digest(csv)),
        "source":"https://webtrak-us02.emsbk.com/WebTrak/cda/data/","altitude":"Synthetic WGS84"});
    std::fs::write(
        folder.join("provider-manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(
        folder.join("aircraft-types.json"),
        r#"{"C172":["Cessna","L1P","L"]}"#,
    )
    .unwrap();
}

fn shifted_csv(offset: f64, only_day: Option<&str>) -> Vec<u8> {
    let mut csv = String::from("aircraft,time,lat,lon,altitude_m\n");
    for track in demo::tracks() {
        for sample in track.samples {
            let time = sample.time + offset;
            if only_day.is_some_and(|day| {
                chrono::DateTime::from_timestamp(time as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d")
                    .to_string()
                    != day
            }) {
                continue;
            }
            csv.push_str(&format!(
                "{},{},{},{},{}\n",
                track.id, time, sample.point.lat, sample.point.lon, sample.point.alt
            ));
        }
    }
    csv.into_bytes()
}

fn analyze(root: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .current_dir(root)
        .args([
            "analyze",
            "--config",
            "job.json",
            "--cache",
            "cache",
            "--provider",
            "webtrak-chicago",
            "--offline",
            "--output",
            "-",
            "--quiet",
        ])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn prepare_and_search_33_cached_days_without_a_recording_date() {
    let dir = tempfile::tempdir().unwrap();
    demo::create(dir.path()).unwrap();
    let first = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
    for i in 0..33 {
        let day = first.checked_add_days(Days::new(i)).unwrap().to_string();
        let csv = if day == "2026-09-08" {
            shifted_csv(0., None)
        } else {
            format!(
                "aircraft,time,lat,lon,altitude_m\nunrelated,{},0,0,1000\n",
                first
                    .checked_add_days(Days::new(i))
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
            )
            .into_bytes()
        };
        cache_day(&dir.path().join("cache"), &day, &csv);
    }
    let ready = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .current_dir(dir.path())
        .args([
            "download",
            "--date",
            "2026-08-07",
            "--date-to",
            "2026-09-08",
            "--provider",
            "webtrak-chicago",
            "--output",
            "cache",
            "--offline",
            "--quiet",
        ])
        .output()
        .unwrap();
    assert!(
        ready.status.success(),
        "{}",
        String::from_utf8_lossy(&ready.stderr)
    );
    let result = analyze(dir.path(), &["--require-match"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Report = serde_json::from_slice(&result.stdout).unwrap();
    let coverage = report.date_search.as_ref().unwrap();
    assert_eq!(coverage.dates.len(), 33);
    assert_eq!(coverage.completed_batches, 32);
    assert!(coverage.complete && report.job.offline && report.job.start_utc.is_none());
    assert!(report.job.archive.as_ref().unwrap().date_from.is_empty());
    let candidate = &report.candidates[0];
    assert!((candidate.recording_start_unix - demo::EPOCH).abs() < 30.);
    assert!(
        demo::TRUTH.distance(Point {
            lat: candidate.latitude,
            lon: candidate.longitude,
            alt: candidate.ground_m
        }) < 500.
    );
    assert_eq!(report.reference_sources.len(), 33);
}

#[test]
fn offline_date_batches_recover_a_recording_across_midnight() {
    let dir = tempfile::tempdir().unwrap();
    demo::create(dir.path()).unwrap();
    let start = NaiveDate::from_ymd_opt(2026, 9, 7)
        .unwrap()
        .and_hms_opt(23, 59, 0)
        .unwrap()
        .and_utc()
        .timestamp() as f64;
    for day in ["2026-09-07", "2026-09-08"] {
        cache_day(
            &dir.path().join("cache"),
            day,
            &shifted_csv(start - demo::EPOCH, Some(day)),
        );
    }
    let result = analyze(dir.path(), &["--require-match"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Report = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report.date_search.unwrap().completed_batches, 1);
    assert!((report.candidates[0].recording_start_unix - start).abs() < 30.);
    assert!(report.candidates[0].matches.len() >= 2);
}

#[test]
fn different_matching_dates_remain_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    demo::create(dir.path()).unwrap();
    cache_day(
        &dir.path().join("cache"),
        "2026-09-08",
        &shifted_csv(0., None),
    );
    cache_day(
        &dir.path().join("cache"),
        "2026-10-08",
        &shifted_csv(30. * 86400., None),
    );
    let result = analyze(dir.path(), &["--require-unambiguous"]);
    assert_eq!(
        result.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Report = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report.status, "ambiguous");
    assert!(
        report
            .candidates
            .iter()
            .any(|c| (c.recording_start_unix - demo::EPOCH).abs() < 30.)
    );
    assert!(
        report
            .candidates
            .iter()
            .any(|c| (c.recording_start_unix - demo::EPOCH - 30. * 86400.).abs() < 30.)
    );
}

#[test]
fn missing_offline_data_is_reported_without_contacting_a_proxy() {
    let dir = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    for provider in [
        "adsb-lol",
        "webtrak-chicago",
        "webtrak-lax",
        "adsb-exchange-sample",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
            .current_dir(dir.path())
            .env("HTTP_PROXY", &proxy)
            .env("HTTPS_PROXY", &proxy)
            .env("ALL_PROXY", &proxy)
            .env("NO_PROXY", "")
            .args([
                "download",
                "--date",
                "2026-09-01",
                "--provider",
                provider,
                "--offline",
                "--output",
                "cache",
                "--south",
                "41.7",
                "--north",
                "41.8",
                "--west",
                "-87.8",
                "--east",
                "-87.7",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Offline mode"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn offline_media_decode_blocks_external_playlist_requests() {
    let dir = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let media = dir.path().join("playlist.m3u8");
    std::fs::write(
        &media,
        format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:30\n#EXTINF:30,\nhttp://{}/clip.ts\n#EXT-X-ENDLIST\n",
            listener.local_addr().unwrap()
        ),
    )
    .unwrap();
    let job = Job {
        media,
        offline: true,
        ..Default::default()
    };
    assert!(soundgeo::audio::decode(&job, &Control::default()).is_err());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
