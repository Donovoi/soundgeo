use sha2::{Digest, Sha256};
use soundgeo::{
    Report,
    terrain_archive::{TileRecord, tiles},
};
use std::process::Command;

#[test]
fn cli_uses_verified_offline_terrain_and_preserves_manual_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let demo = dir.path().join("demo");
    let mut job = soundgeo::demo::create(&demo).unwrap();
    let bounds = job.bounds.unwrap();
    let cache = dir.path().join("terrain");
    std::fs::create_dir(&cache).unwrap();
    // Already downloaded fixture: tests must not need the public server.
    let bytes = [0, 100].repeat(1201 * 1201);
    for tile in tiles(bounds).unwrap() {
        let record = TileRecord {
            source_url: format!(
                "https://elevation-tiles-prod.s3.amazonaws.com/skadi/{}/{tile}.hgt.gz",
                &tile[..3]
            ),
            tile: tile.clone(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            compressed_sha256: "fixture".into(),
            size_bytes: bytes.len(),
            downloaded_utc: "2026-01-01T00:00:00Z".into(),
        };
        std::fs::write(cache.join(format!("{tile}.hgt")), &bytes).unwrap();
        std::fs::write(
            cache.join(format!("{tile}.terrain.json")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .args([
            "terrain",
            "--south",
            &bounds.south.to_string(),
            "--north",
            &bounds.north.to_string(),
            "--west",
            &bounds.west.to_string(),
            "--east",
            &bounds.east.to_string(),
            "--output",
        ])
        .arg(&cache)
        .arg("--quiet")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let config = demo.join("job.json");
    let downloaded_report = dir.path().join("downloaded.json");
    // Explicit CLI cache paths are relative to the caller, not the config.
    let output = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .current_dir(dir.path())
        .args([
            "analyze",
            "--config",
            "demo/job.json",
            "--terrain-cache",
            "terrain",
            "--output",
        ])
        .arg(&downloaded_report)
        .arg("--quiet")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let auto: Report = serde_json::from_slice(&std::fs::read(downloaded_report).unwrap()).unwrap();
    assert!(auto.job.download_terrain);
    assert!(auto.job.start_utc.is_none());
    assert_eq!(auto.terrain_sources.len(), tiles(bounds).unwrap().len());
    job.terrain = Some(cache);
    std::fs::write(&config, serde_json::to_vec(&job).unwrap()).unwrap();
    let manual_path = dir.path().join("manual.json");
    let output = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .args(["analyze", "--config"])
        .arg(&config)
        .arg("--output")
        .arg(&manual_path)
        .arg("--quiet")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manual: Report = serde_json::from_slice(&std::fs::read(manual_path).unwrap()).unwrap();
    // Saving a report must never corrupt its terrain input or cache metadata.
    let tile = tiles(bounds).unwrap().remove(0);
    let cache = manual.job.terrain.as_ref().unwrap();
    for name in [
        format!("{tile}.hgt"),
        format!("{tile}.terrain.json"),
        "TERRAIN-ATTRIBUTION.txt".into(),
    ] {
        let path = cache.join(name);
        let before = std::fs::read(&path).unwrap();
        assert!(
            soundgeo::save_report(&manual, &path)
                .unwrap_err()
                .to_string()
                .contains("terrain")
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    assert!(!auto.candidates.is_empty());
    assert_eq!(auto.status, manual.status);
    assert_eq!(
        serde_json::to_value(auto.candidates).unwrap(),
        serde_json::to_value(manual.candidates).unwrap()
    );
    assert_eq!(auto.hypotheses, manual.hypotheses);
    assert!(manual.terrain_sources.is_empty());
}
