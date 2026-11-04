use sha2::{Digest, Sha256};
use std::process::Command;

#[test]
fn date_only_cli_uses_full_day_verified_archive_and_protects_all_parts() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = soundgeo::demo::create(dir.path()).unwrap();
    job.tracks = "missing-reference.csv".into();
    job.media = "synthetic-flyovers.wav".into();
    let date = "2026-09-08";
    let cache = dir.path().join("cache");
    let day = cache.join(date);
    std::fs::create_dir_all(&day).unwrap();
    let tag = "v2026.09.08-planes-readsb-prod-0";
    let name = format!("{tag}.tar.aa");
    let mut tar = tar::Builder::new(Vec::new());
    for track in soundgeo::demo::tracks() {
        let rows: Vec<_> = track
            .samples
            .iter()
            .map(|p| {
                serde_json::json!([
                    p.time,
                    p.point.lat,
                    p.point.lon,
                    p.point.alt / 0.3048,
                    150,
                    0,
                    0,
                    0,
                    null,
                    "adsb_icao",
                    p.point.alt / 0.3048
                ])
            })
            .collect();
        let trace = serde_json::json!({"icao":track.id,"timestamp":0,"trace":rows});
        let mut zipped = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        serde_json::to_writer(&mut zipped, &trace).unwrap();
        let bytes = zipped.finish().unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(
            &mut header,
            format!("trace_full_{}.json", track.id),
            bytes.as_slice(),
        )
        .unwrap();
    }
    let bytes = tar.into_inner().unwrap();
    std::fs::write(day.join(&name), &bytes).unwrap();
    let manifest = serde_json::json!({"tag_name":tag,"assets":[{"name":name,"size":bytes.len(),
        "digest":format!("sha256:{:x}",Sha256::digest(&bytes)),
        "browser_download_url":"https://github.com/adsblol/globe_history_2026/releases/download/fixture/nonexistent"}]});
    std::fs::write(
        day.join("archive-manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(
        day.join("aircraft-types.json"),
        r#"{"C172":["Cessna","L1P","L"]}"#,
    )
    .unwrap();
    let config = dir.path().join("job.json");
    std::fs::write(&config, serde_json::to_vec(&job).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .current_dir(dir.path())
        .args([
            "analyze",
            "--config",
            "job.json",
            "--date",
            date,
            "--cache",
            "cache",
            "--output",
            "-",
            "--quiet",
            "--require-match",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: soundgeo::Report = serde_json::from_slice(&result.stdout).unwrap();
    assert!(!report.candidates.is_empty());
    assert!(report.job.start_utc.is_none());
    assert_eq!(report.reference_sources.len(), 1);
    // An output must not destroy a cached manifest or a later split part.
    let mut absolute = report.clone();
    absolute.job.archive.as_mut().unwrap().cache = cache;
    assert!(soundgeo::save_report(&absolute, &day.join("archive-manifest.json")).is_err());
    absolute.job.archive = None;
    absolute.reference_sources = vec![day.join(&name)];
    let second = day.join(format!("{tag}.tar.ab"));
    std::fs::write(&second, "protected second part").unwrap();
    assert!(soundgeo::save_report(&absolute, &second).is_err());
    assert_eq!(
        std::fs::read_to_string(second).unwrap(),
        "protected second part"
    );
}
