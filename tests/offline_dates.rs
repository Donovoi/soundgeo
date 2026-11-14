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

fn checkpoint_job(root: &Path) -> Job {
    let mut job = demo::create(root).unwrap();
    job.offline = true;
    job.search_threads = Some(1);
    job.archive = Some(soundgeo::archive::ArchiveRequest {
        date_from: String::new(),
        date_to: None,
        cache: root.join("cache"),
        provider: soundgeo::archive::ArchiveProvider::WebtrakChicago,
    });
    for (day, offset) in [("2026-09-08", 0.), ("2026-09-10", 2. * 86400.)] {
        cache_day(&root.join("cache"), day, &shifted_csv(offset, None));
    }
    job
}

fn without_creation_time(report: Report) -> serde_json::Value {
    let mut value = serde_json::to_value(report).unwrap();
    value.as_object_mut().unwrap().remove("created_utc");
    value
}

#[test]
fn control_identity_replays_overlapping_dates_partitions_and_competing_clocks() {
    use soundgeo::doppler_controls::{bind_search, replay_observations};
    let dir = tempfile::tempdir().unwrap();
    let mut job = checkpoint_job(dir.path());
    cache_day(
        &dir.path().join("cache"),
        "2026-09-09",
        &shifted_csv(86400., None),
    );
    job.search_mode = soundgeo::search::SearchMode::FlightCorridors;
    let probe = soundgeo::analyze(&job, &Control::default(), &|_, _| {}).unwrap();
    assert!(probe.doppler_partitions.is_empty());
    job.max_hypotheses = probe.hypotheses / 8;
    assert!(job.max_hypotheses >= 100);
    let baseline = soundgeo::analyze(&job, &Control::default(), &|_, _| {}).unwrap();
    let coverage = baseline.date_search.as_ref().unwrap();
    assert_eq!(coverage.completed_batches, 2);
    assert_eq!(coverage.dates, ["2026-09-08", "2026-09-09", "2026-09-10"]);
    assert_eq!(baseline.doppler_partitions.len(), 2);
    assert!(
        baseline
            .doppler_partitions
            .iter()
            .all(|p| p.complete && p.batches.len() > 1)
    );
    assert_eq!(baseline.status, "ambiguous");
    for offset in [0., 86400., 172800.] {
        assert!(
            baseline
                .candidates
                .iter()
                .any(|c| (c.recording_start_unix - demo::EPOCH - offset).abs() < 30.)
        );
    }
    let saved = dir.path().join("identity-baseline.json");
    soundgeo::save_report(&baseline, &saved).unwrap();
    let loaded: Report = serde_json::from_slice(&std::fs::read(saved).unwrap()).unwrap();
    let binding = bind_search(&loaded, &Control::default()).unwrap();
    let mut incomplete = loaded.clone();
    incomplete.doppler_partitions[0].complete = false;
    assert!(bind_search(&incomplete, &Control::default()).is_err());
    let binding = serde_json::from_slice(&serde_json::to_vec(&binding).unwrap()).unwrap();
    let replay = replay_observations(
        &loaded,
        &binding,
        loaded.events.clone(),
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert_eq!(
        without_creation_time(replay),
        without_creation_time(baseline),
        "Identity replay must preserve the full production report, including overlap, partitions, warnings, candidates and merge status"
    );
}

#[test]
fn control_bindings_reject_changed_dates_partial_inputs_and_incomplete_baselines() {
    use soundgeo::doppler_controls::{bind_search, replay_observations};
    let dir = tempfile::tempdir().unwrap();
    let job = checkpoint_job(dir.path());
    let baseline = soundgeo::analyze(&job, &Control::default(), &|_, _| {}).unwrap();
    let binding = bind_search(&baseline, &Control::default()).unwrap();
    for bad in [
        {
            let mut r = baseline.clone();
            r.date_search.as_mut().unwrap().complete = false;
            r
        },
        {
            let mut r = baseline.clone();
            r.date_search.as_mut().unwrap().completed_batches = 1;
            r
        },
        {
            let mut r = baseline.clone();
            r.job.start_utc = Some(demo::EPOCH);
            r
        },
        {
            let mut r = baseline.clone();
            r.job.archive.as_mut().unwrap().date_from = "2026-09-08".into();
            r
        },
    ] {
        assert!(bind_search(&bad, &Control::default()).is_err());
    }
    assert!(
        binding
            .check_output_path(&dir.path().join("cache/webtrak-cda-2026-09-08/control.json"))
            .is_err()
    );
    cache_day(
        &dir.path().join("cache"),
        "2026-09-09",
        &shifted_csv(86400., None),
    );
    assert!(
        replay_observations(
            &baseline,
            &binding,
            baseline.events.clone(),
            &Control::default(),
            &|_, _| {}
        )
        .unwrap_err()
        .to_string()
        .contains("date plan")
    );
    std::fs::remove_dir_all(dir.path().join("cache/webtrak-cda-2026-09-09")).unwrap();
    let file = dir.path().join("cache/webtrak-cda-2026-09-08/flights.csv");
    let bytes = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    assert!(
        replay_observations(
            &baseline,
            &binding,
            baseline.events.clone(),
            &Control::default(),
            &|_, _| {}
        )
        .is_err()
    );
    std::fs::write(&file, bytes).unwrap();
    let catalog = dir
        .path()
        .join("cache/webtrak-cda-2026-09-08/aircraft-types.json");
    let original = std::fs::read(&catalog).unwrap();
    std::fs::write(&catalog, br#"{"C172":["Different maker","L1P","L"]}"#).unwrap();
    assert!(
        replay_observations(
            &baseline,
            &binding,
            baseline.events.clone(),
            &Control::default(),
            &|_, _| {}
        )
        .unwrap_err()
        .to_string()
        .contains("inputs changed")
    );
    std::fs::write(&catalog, original).unwrap();
    let error = replay_observations(
        &baseline,
        &binding,
        baseline.events.clone(),
        &Control::default(),
        &|message, _| {
            if message.ends_with("Analysis complete") {
                std::fs::write(&catalog, br#"{"C172":["Changed mid-search","L1P","L"]}"#).unwrap();
            }
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("changed during search"),
        "{error:#}"
    );
}

#[test]
fn control_identity_preserves_single_date_and_imported_searches() {
    use soundgeo::doppler_controls::{bind_search, replay_observations};
    let dir = tempfile::tempdir().unwrap();
    let mut job = demo::create(dir.path()).unwrap();
    job.offline = true;
    for archive in [false, true] {
        if archive {
            cache_day(
                &dir.path().join("cache"),
                "2026-09-08",
                &shifted_csv(0., None),
            );
            job.archive = Some(soundgeo::archive::ArchiveRequest {
                date_from: String::new(),
                date_to: None,
                cache: dir.path().join("cache"),
                provider: soundgeo::archive::ArchiveProvider::WebtrakChicago,
            });
        }
        let baseline = soundgeo::analyze(&job, &Control::default(), &|_, _| {}).unwrap();
        assert!(baseline.date_search.is_none());
        let binding = bind_search(&baseline, &Control::default()).unwrap();
        let replay = replay_observations(
            &baseline,
            &binding,
            baseline.events.clone(),
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(
            without_creation_time(replay),
            without_creation_time(baseline)
        );
    }
}

#[test]
fn interrupted_date_search_resumes_complete_batches_with_exact_report_parity() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let job = checkpoint_job(dir.path());
    let checkpoint = dir.path().join("checkpoints");
    let control = Control::default().with_date_checkpoints(checkpoint.clone());
    let stopped = soundgeo::analyze(&job, &control, &|message, _| {
        if message.ends_with("Saved completed date batch") {
            control.stop();
        }
    });
    assert!(stopped.unwrap_err().to_string().contains("Cancelled"));
    let first = std::fs::read(checkpoint.join("batch-0000.json")).unwrap();
    assert!(
        !checkpoint.join("batch-0001.json").exists(),
        "Partial search must not publish a later batch"
    );
    let reused = AtomicUsize::new(0);
    let resumed = soundgeo::analyze(
        &job,
        &Control::default().with_date_checkpoints(checkpoint.clone()),
        &|message, _| {
            if message.ends_with("Reusing completed date batch") {
                reused.fetch_add(1, Ordering::Relaxed);
            }
        },
    )
    .unwrap();
    assert_eq!(reused.load(Ordering::Relaxed), 1);
    assert_eq!(
        std::fs::read(checkpoint.join("batch-0000.json")).unwrap(),
        first
    );
    assert_eq!(
        resumed.status, "ambiguous",
        "The other day's competing clock must survive recovery"
    );
    let coverage = resumed.date_search.as_ref().unwrap();
    assert!(coverage.complete);
    assert_eq!(coverage.completed_batches, 2);
    let uninterrupted = soundgeo::analyze(&job, &Control::default(), &|_, _| {}).unwrap();
    assert_eq!(
        without_creation_time(resumed),
        without_creation_time(uninterrupted),
        "Checkpoint JSON must preserve all numeric values, candidates, warnings and coverage exactly"
    );

    // A whole valid record copied to the wrong window must still be rejected.
    std::fs::write(checkpoint.join("batch-0001.json"), &first).unwrap();
    let error = soundgeo::analyze(
        &job,
        &Control::default().with_date_checkpoints(checkpoint),
        &|_, _| {},
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("checkpoint binding"),
        "{error:#}"
    );
}

#[test]
fn date_checkpoints_reject_changed_reference_metadata_and_corrupt_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = checkpoint_job(dir.path());
    let explicit_catalog = dir.path().join("explicit-aircraft-types.json");
    let catalog_bytes = br#"{"C172":["Cessna","L1P","L"]}"#;
    std::fs::write(&explicit_catalog, catalog_bytes).unwrap();
    job.type_catalog = Some(explicit_catalog.clone());
    let checkpoint = dir.path().join("checkpoints");
    // Change a shared input after the manifest is bound but before matching.
    // Restoring it after failure must not make an incorrectly bound result reusable.
    let error = soundgeo::analyze(
        &job,
        &Control::default().with_date_checkpoints(checkpoint.clone()),
        &|message, _| {
            if message.ends_with("Reading historical flight paths") {
                std::fs::write(
                    &explicit_catalog,
                    br#"{"C172":["Changed maker","L1P","L"]}"#,
                )
                .unwrap();
            }
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("Shared inputs changed"),
        "{error:#}"
    );
    assert!(!checkpoint.join("batch-0000.json").exists());
    std::fs::write(&explicit_catalog, catalog_bytes).unwrap();
    let control = Control::default().with_date_checkpoints(checkpoint.clone());
    soundgeo::analyze(&job, &control, &|message, _| {
        if message.ends_with("Saved completed date batch") {
            control.stop();
        }
    })
    .unwrap_err();
    let path = checkpoint.join("batch-0000.json");
    let original = std::fs::read(&path).unwrap();
    let run = || {
        soundgeo::analyze(
            &job,
            &Control::default().with_date_checkpoints(checkpoint.clone()),
            &|_, _| {},
        )
        .unwrap_err()
    };
    // Keep the provider's integrity checks valid while changing the actual CSV.
    let mut changed = shifted_csv(0., None);
    changed.push(b'\n');
    cache_day(&dir.path().join("cache"), "2026-09-08", &changed);
    let error = run();
    assert!(
        error.to_string().contains("checkpoint binding")
            || error.to_string().contains("reference inputs changed"),
        "{error:#}"
    );
    cache_day(
        &dir.path().join("cache"),
        "2026-09-08",
        &shifted_csv(0., None),
    );
    // The inferred aircraft catalog is also part of the reference inventory.
    let catalog = dir
        .path()
        .join("cache/webtrak-cda-2026-09-08/aircraft-types.json");
    let catalog_original = std::fs::read(&catalog).unwrap();
    std::fs::write(&catalog, b"{}").unwrap();
    let error = run();
    assert!(
        error.to_string().contains("checkpoint binding")
            || error.to_string().contains("reference inputs changed"),
        "{error:#}"
    );
    std::fs::write(catalog, catalog_original).unwrap();
    let mut stored: serde_json::Value = serde_json::from_slice(&original).unwrap();
    stored["report"] = serde_json::Value::String("{}".into());
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(run().to_string().contains("checksum failed"));
    std::fs::write(&path, b"{\"incomplete\":").unwrap();
    assert!(run().to_string().contains("Invalid date batch checkpoint"));
    std::fs::write(&path, &original).unwrap();
    let mut changed_job = job.clone();
    changed_job.temperature_c += 1.;
    let error = soundgeo::analyze(
        &changed_job,
        &Control::default().with_date_checkpoints(checkpoint),
        &|_, _| {},
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("executable, plan or inputs changed"),
        "{error:#}"
    );
    assert_eq!(
        std::fs::read(path).unwrap(),
        original,
        "Failures must preserve the earlier checkpoint"
    );
}

#[test]
fn date_checkpoint_cli_rejects_unsupported_inputs_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("unparsed.wav");
    let tracks = dir.path().join("unparsed.csv");
    std::fs::write(
        &media,
        b"Decoding this is an error; validation must reject the request first",
    )
    .unwrap();
    std::fs::write(&tracks, b"unused").unwrap();
    for (extra, expected) in [
        (
            vec![
                "--provider",
                "webtrak-chicago",
                "--date",
                "2026-09-08",
                "--offline",
            ],
            "at least two prepared dates",
        ),
        (
            vec![
                "--provider",
                "webtrak-chicago",
                "--date",
                "2026-09-08",
                "--date-to",
                "2026-09-09",
            ],
            "require offline analysis",
        ),
        (
            vec!["--tracks", "unparsed.csv", "--offline"],
            "require offline analysis",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
            .current_dir(dir.path())
            .args([
                "analyze",
                "--input",
                "unparsed.wav",
                "--output",
                "-",
                "--quiet",
                "--date-checkpoints",
                "checkpoints",
            ])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(!dir.path().join("checkpoints").exists());
    }
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
fn unknown_date_cli_partitions_large_search_and_preserves_competing_clocks() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = demo::create(dir.path()).unwrap();
    job.search_mode = soundgeo::search::SearchMode::FlightCorridors;
    job.search_threads = Some(1);
    job.max_hypotheses = 1_000_000;
    job.start_utc = None;
    let cache = dir.path().join("cache");
    let first = shifted_csv(0., None);
    let second = shifted_csv(86400., None);
    cache_day(&cache, "2026-09-08", &first);
    cache_day(&cache, "2026-09-09", &second);
    let job_path = dir.path().join("job.json");
    std::fs::write(&job_path, serde_json::to_vec(&job).unwrap()).unwrap();
    let full = analyze(dir.path(), &["--require-unambiguous"]);
    assert_eq!(
        full.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&full.stderr)
    );
    let full: Report = serde_json::from_slice(&full.stdout).unwrap();
    assert!(full.doppler_partitions.is_empty());
    job.max_hypotheses = full.hypotheses / 3;
    assert!(job.max_hypotheses >= 100);
    let bytes = serde_json::to_vec(&job).unwrap();
    std::fs::write(&job_path, &bytes).unwrap();
    let divided = analyze(dir.path(), &["--require-unambiguous"]);
    assert_eq!(
        divided.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&divided.stderr)
    );
    let report: Report = serde_json::from_slice(&divided.stdout).unwrap();
    assert_eq!(report.status, "ambiguous");
    assert!(report.job.start_utc.is_none() && report.job.offline);
    let archive = report.job.archive.as_ref().unwrap();
    assert!(archive.date_from.is_empty() && archive.date_to.is_none());
    let coverage = report.date_search.as_ref().unwrap();
    assert!(coverage.complete);
    assert_eq!(coverage.dates, ["2026-09-08", "2026-09-09"]);
    assert_eq!(coverage.completed_batches, 1);
    assert_eq!(report.hypotheses, full.hypotheses);
    assert_eq!(report.doppler_partitions.len(), 1);
    let partition = &report.doppler_partitions[0];
    assert!(partition.complete && partition.batches.len() > 1);
    assert_eq!(partition.max_hypotheses_per_batch, job.max_hypotheses);
    assert!(
        partition
            .batches
            .iter()
            .all(|b| b.hypotheses <= job.max_hypotheses)
    );
    assert_eq!(
        partition
            .batches
            .iter()
            .map(|b| b.hypotheses)
            .sum::<usize>(),
        full.hypotheses
    );
    assert!(
        partition
            .batches
            .windows(2)
            .all(|b| b[0].emission_to_unix == b[1].emission_from_unix)
    );
    for start in [demo::EPOCH, demo::EPOCH + 86400.] {
        assert!(
            report.candidates.iter().any(|candidate| {
                (candidate.recording_start_unix - start).abs() < 30.
                    && Point {
                        lat: candidate.latitude,
                        lon: candidate.longitude,
                        alt: candidate.ground_m,
                    }
                    .distance(demo::TRUTH)
                        < 250.
                    && candidate.matches.len() == report.events.len()
            }),
            "A competing clock or jointly supported location was lost"
        );
    }
    assert_eq!(std::fs::read(job_path).unwrap(), bytes);
    assert_eq!(
        std::fs::read(cache.join("webtrak-cda-2026-09-08/flights.csv")).unwrap(),
        first
    );
    assert_eq!(
        std::fs::read(cache.join("webtrak-cda-2026-09-09/flights.csv")).unwrap(),
        second
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

fn date_index(root: &Path, dates: &[&str]) {
    let catalog = soundgeo::availability::Catalog {
        provider: soundgeo::archive::ArchiveProvider::WebtrakChicago,
        retrieved_utc: chrono::Utc::now().to_rfc3339(),
        recent_checked_utc: None,
        dates: dates.iter().map(|s| (*s).to_owned()).collect(),
        basis: "Saved provider-index fixture; no recording date supplied".into(),
        sources: vec![soundgeo::availability::Source {
            url: "https://example.com/fixture-index".into(),
            sha256: format!("{:x}", Sha256::digest(b"fixture")),
            status: 200,
            request_body_sha256: None,
        }],
    };
    let hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&catalog).unwrap())
    );
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join("available-dates-webtrak_chicago.json"),
        serde_json::to_vec(&serde_json::json!({"format":1,"payload":catalog,"sha256":hash}))
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn unknown_date_online_search_uses_provider_index_and_keeps_older_cached_days() {
    let dir = tempfile::tempdir().unwrap();
    demo::create(dir.path()).unwrap();
    let cache = dir.path().join("cache");
    date_index(&cache, &["2026-09-08", "2026-09-09"]);
    cache_day(&cache, "2026-09-08", &shifted_csv(0., None));
    for date in ["2026-08-07", "2026-09-09"] {
        let time = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        cache_day(
            &cache,
            date,
            format!("aircraft,time,lat,lon,altitude_m\nunrelated,{time},0,0,1000\n").as_bytes(),
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .current_dir(dir.path())
        .args([
            "analyze",
            "--config",
            "job.json",
            "--cache",
            "cache",
            "--provider",
            "webtrak-chicago",
            "--output",
            "-",
            "--quiet",
            "--require-match",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Report = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.job.start_utc.is_none() && !report.job.offline);
    assert!(report.job.archive.as_ref().unwrap().date_from.is_empty());
    let coverage = report.date_search.unwrap();
    assert_eq!(coverage.dates, ["2026-08-07", "2026-09-08", "2026-09-09"]);
    assert!(coverage.complete);
    assert_eq!(
        coverage.provider_discovery.unwrap().dates,
        ["2026-09-08", "2026-09-09"]
    );
    let first = &report.candidates[0];
    assert!(
        demo::TRUTH.distance(Point {
            lat: first.latitude,
            lon: first.longitude,
            alt: first.ground_m
        }) < 500.
    );
}

#[test]
fn saved_date_index_does_not_count_as_downloaded_flight_data_offline() {
    let dir = tempfile::tempdir().unwrap();
    demo::create(dir.path()).unwrap();
    date_index(&dir.path().join("cache"), &["2026-09-08"]);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let command = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
            .current_dir(dir.path())
            .env("HTTP_PROXY", &proxy)
            .env("HTTPS_PROXY", &proxy)
            .env("ALL_PROXY", &proxy)
            .env("NO_PROXY", "")
            .args(args)
            .output()
            .unwrap()
    };
    let dates = command(&[
        "dates",
        "--provider",
        "webtrak-chicago",
        "--cache",
        "cache",
        "--offline",
        "--quiet",
    ]);
    assert!(
        dates.status.success(),
        "{}",
        String::from_utf8_lossy(&dates.stderr)
    );
    let catalog: soundgeo::availability::Catalog = serde_json::from_slice(&dates.stdout).unwrap();
    assert_eq!(catalog.dates, ["2026-09-08"]);
    let analysis = command(&[
        "analyze",
        "--config",
        "job.json",
        "--provider",
        "webtrak-chicago",
        "--cache",
        "cache",
        "--offline",
        "--output",
        "-",
        "--quiet",
    ]);
    assert!(!analysis.status.success());
    assert!(String::from_utf8_lossy(&analysis.stderr).contains("No cached dates"));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
