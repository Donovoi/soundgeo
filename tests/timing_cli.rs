use soundgeo::{AnalysisMethod, Job, geo::Bounds};
use std::process::Command;

#[test]
fn timing_cli_reports_insufficient_audio_without_downloading_or_claiming_a_match() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("quiet.wav");
    let mut writer = hound::WavWriter::create(
        &media,
        hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..8000 * 480 {
        writer.write_sample(0i16).unwrap();
    }
    writer.finalize().unwrap();
    let cache = dir.path().join("must-not-download");
    let job = Job {
        media,
        archive: Some(soundgeo::archive::ArchiveRequest {
            date_from: "2026-09-08".into(),
            date_to: None,
            cache: cache.clone(),
            provider: soundgeo::archive::ArchiveProvider::WebtrakChicago,
        }),
        bounds: Some(Bounds {
            south: 41.65,
            north: 42.01,
            west: -87.98,
            east: -87.5,
        }),
        ..Default::default()
    };
    let config = dir.path().join("job.json");
    std::fs::write(&config, serde_json::to_vec(&job).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .args([
            "analyze",
            "--method",
            "passage-timing",
            "--output",
            "-",
            "--quiet",
            "--require-match",
            "--config",
        ])
        .arg(config)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: soundgeo::Report = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report.job.analysis_method, AnalysisMethod::PassageTiming);
    assert!(report.job.start_utc.is_none());
    assert_eq!(report.status, "insufficient_evidence");
    assert_eq!(report.candidate_count(), 0);
    assert!(report.timing.unwrap().peaks.is_empty());
    assert!(report.reference_sources.is_empty());
    assert!(!cache.exists());
}

#[test]
fn timing_requires_bounds_before_processing_and_old_jobs_keep_their_method() {
    let old: Job = serde_json::from_str(r#"{"start_utc":null}"#).unwrap();
    assert_eq!(old.analysis_method, AnalysisMethod::Doppler);
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("job.json");
    std::fs::write(&config, serde_json::to_vec(&old).unwrap()).unwrap();
    let output = dir.path().join("report.json");
    let result = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
        .args(["analyze", "--method", "passage-timing", "--config"])
        .arg(config)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("needs a search area"));
    assert!(!output.exists());
}
