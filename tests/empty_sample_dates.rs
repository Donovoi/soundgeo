use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soundgeo::{
    Control, Report, demo,
    geo::{Bounds, Point},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

fn object(folder: &Path, name: &str, value: Value) -> Value {
    let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
    gzip.write_all(&serde_json::to_vec(&value).unwrap())
        .unwrap();
    let bytes = gzip.finish().unwrap();
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join(name), &bytes).unwrap();
    json!({"file":name,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))})
}

fn sample_day(cache: &Path, date: &str, bounds: Bounds, populated: bool) -> PathBuf {
    let tag = format!("{:x}", Sha256::digest(serde_json::to_vec(&bounds).unwrap()));
    let root = cache.join(format!("adsbx-sample-{date}-{}", &tag[..16]));
    let shared = cache.join(format!("adsbx-sample-{date}-snapshots"));
    let traces = root.join("traces");
    std::fs::create_dir_all(&traces).unwrap();
    let start = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp() as f64;
    let aircraft = if populated {
        vec!["000001", "000002"]
    } else {
        vec![]
    };
    let snapshots: Vec<_> = (0..288).map(|i| {
        let name = format!("{:02}{:02}00Z.json.gz", i / 12, i % 12 * 5);
        object(&shared, &name, json!({"now":start+i as f64*300.,"aircraft":
            aircraft.iter().map(|id|json!({"hex":id,"lat":demo::TRUTH.lat,"lon":demo::TRUTH.lon})).collect::<Vec<_>>()
        }))
    }).collect();
    let mut records = Vec::new();
    if populated {
        for (id, track) in aircraft.iter().zip(demo::tracks()) {
            let rows: Vec<_> = track
                .samples
                .iter()
                .map(|s| {
                    json!([
                        36000. + s.time - demo::EPOCH,
                        s.point.lat,
                        s.point.lon,
                        s.point.alt / 0.3048,
                        100,
                        90,
                        0,
                        0,
                        null,
                        "adsb_icao",
                        s.point.alt / 0.3048
                    ])
                })
                .collect();
            records.push(object(
                &traces,
                &format!("trace_full_{id}.json.gz"),
                json!({"icao":id,"timestamp":start,"trace":rows,"desc":"L1P"}),
            ));
        }
    }
    std::fs::write(
        root.join("sample-manifest.json"),
        serde_json::to_vec(&json!({
            "schema":1,"date":date,"bounds":bounds,"source":"https://example.com/synthetic-sample",
            "snapshots":snapshots,"aircraft":aircraft,"traces":records,"missing_aircraft":[]
        }))
        .unwrap(),
    )
    .unwrap();
    traces
}

fn analyze(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .current_dir(root)
        .args([
            "analyze",
            "--config",
            "job.json",
            "--cache",
            "cache",
            "--provider",
            "adsb-exchange-sample",
            "--offline",
            "--quiet",
            "--output",
            "-",
        ])
        .output()
        .unwrap()
}

#[test]
fn empty_sample_dates_keep_later_matches_and_damaged_cache_still_fails() {
    let dir = tempfile::tempdir().unwrap();
    let job = demo::create(dir.path()).unwrap();
    let cache = dir.path().join("cache");
    let empty = sample_day(&cache, "2026-07-01", job.bounds.unwrap(), false);
    assert!(soundgeo::data::load(&empty, &Control::default()).is_err());
    let single = analyze(dir.path());
    assert!(
        single.status.success(),
        "{}",
        String::from_utf8_lossy(&single.stderr)
    );
    let report: Report = serde_json::from_slice(&single.stdout).unwrap();
    assert_eq!(report.status, "insufficient_evidence");
    assert!(report.candidates.is_empty());
    assert_eq!(
        report
            .reference_sources
            .iter()
            .map(|p| dir.path().join(p))
            .collect::<Vec<_>>(),
        [empty]
    );

    sample_day(&cache, "2026-08-01", job.bounds.unwrap(), true);
    sample_day(&cache, "2026-09-01", job.bounds.unwrap(), false);
    let mixed = analyze(dir.path());
    assert!(
        mixed.status.success(),
        "{}",
        String::from_utf8_lossy(&mixed.stderr)
    );
    let report: Report = serde_json::from_slice(&mixed.stdout).unwrap();
    let coverage = report.date_search.as_ref().unwrap();
    assert_eq!(coverage.dates, ["2026-07-01", "2026-08-01", "2026-09-01"]);
    assert!(coverage.complete);
    assert_eq!(coverage.completed_batches, 3);
    assert_eq!(report.reference_sources.len(), 3);
    assert!(
        report.job.start_utc.is_none() && report.job.archive.as_ref().unwrap().date_from.is_empty()
    );
    assert_ne!(report.search_method, "date_batches");
    let candidate = &report.candidates[0];
    assert!(
        demo::TRUTH.distance(Point {
            lat: candidate.latitude,
            lon: candidate.longitude,
            alt: candidate.ground_m
        }) < 500.
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("2026-07-01: 0 full-day traces"))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("2026-09-01: 0 full-day traces"))
    );

    std::fs::remove_file(cache.join("adsbx-sample-2026-09-01-snapshots/000000Z.json.gz")).unwrap();
    let damaged = analyze(dir.path());
    assert!(!damaged.status.success());
    assert!(String::from_utf8_lossy(&damaged.stderr).contains("Offline mode"));
    assert!(damaged.stdout.is_empty());
}
