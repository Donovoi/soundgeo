use std::process::Command;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"));
    command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    command
}

#[test]
fn optional_video_commands_keep_offline_and_argument_guards() {
    let dir = tempfile::tempdir().unwrap();
    let result = cli()
        .args(["video-data", "--country", "US", "--offline", "--output"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("Offline mode"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let result = cli()
        .args([
            "video-clues",
            "--input",
            "https://example.com/video.mp4",
            "--gazetteer",
            "places.txt",
            "--output",
            "-",
            "--offline",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("local video"));
    let result = cli()
        .args([
            "video-clues",
            "--input",
            "video.mp4",
            "--gazetteer",
            "places.txt",
            "--output",
            "-",
            "--south",
            "50",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--north"));
}

#[test]
fn headless_cli_generates_demo_and_streams_a_complete_report() {
    let dir = tempfile::tempdir().unwrap();
    let demo = dir.path().join("demo with spaces");
    let generated = cli()
        .args(["--quiet", "demo", "--output"])
        .arg(&demo)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(generated.stdout.is_empty());
    let config = demo.join("job.json");
    let mut job: soundgeo::Job = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    job.media = "synthetic-flyovers.wav".into();
    job.tracks = "flights.csv".into();
    std::fs::write(&config, serde_json::to_vec(&job).unwrap()).unwrap();
    let result = cli()
        .current_dir(dir.path())
        .args(["analyze", "--config"])
        .arg(&config)
        .args([
            "--output",
            "-",
            "--require-match",
            "--require-unambiguous",
            "--max-hypotheses",
            "50000000",
            "--search-threads",
            "8",
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    // No progress or human-readable summary may contaminate stdout.
    let report: soundgeo::Report = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report.events.len(), 2);
    assert!(!report.candidates.is_empty());
    assert_eq!(report.status, "candidates_found");
    assert!(report.job.start_utc.is_none());
    assert_eq!(report.job.max_hypotheses, 50_000_000);
    assert_eq!(report.job.search_threads, Some(8));
    let saved: soundgeo::Job = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(saved.max_hypotheses, job.max_hypotheses);
    assert_eq!(saved.search_threads, None);
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
    assert!(!result.stderr.is_empty());
}

#[test]
fn headless_cli_direct_paths_quiet_mode_and_no_match_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("silence.wav");
    let mut writer = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 4000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..4000 * 25 {
        writer.write_sample(0i16).unwrap();
    }
    writer.finalize().unwrap();
    let tracks = dir.path().join("flights.csv");
    std::fs::write(&tracks, b"aircraft,time,lat,lon,altitude_m\n").unwrap();
    let output = dir.path().join("reports/result.json");
    let result = cli()
        .args(["analyze", "--input"])
        .arg(&wav)
        .arg("--tracks")
        .arg(&tracks)
        .arg("--output")
        .arg(&output)
        .args(["--quiet", "--require-match"])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    let report: soundgeo::Report =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(report.status, "insufficient_evidence");
    assert!(report.candidates.is_empty());
    let result = cli()
        .args(["--quiet", "analyze", "--input"])
        .arg(&wav)
        .arg("--tracks")
        .arg(&tracks)
        .args(["--output", "-"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert!(serde_json::from_slice::<soundgeo::Report>(&result.stdout).is_ok());
    let result = cli()
        .args(["--quiet", "analyze", "--input"])
        .arg(&wav)
        .arg("--tracks")
        .arg(&tracks)
        .args(["--output", "-", "--require-unambiguous"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3));
    let report: soundgeo::Report = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report.status, "insufficient_evidence");
    assert!(report.candidates.is_empty());
}

#[test]
fn headless_cli_can_reject_ambiguity_after_saving_the_report() {
    let dir = tempfile::tempdir().unwrap();
    soundgeo::demo::create(dir.path()).unwrap();
    let config = dir.path().join("job.json");
    let mut job: soundgeo::Job = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    // Keep only the first flyover. Its unknown clock must remain provisional.
    let mut reader = hound::WavReader::open(&job.media).unwrap();
    let spec = reader.spec();
    let short = dir.path().join("one-flyover.wav");
    let mut writer = hound::WavWriter::create(&short, spec).unwrap();
    for sample in reader
        .samples::<i16>()
        .take(spec.sample_rate as usize * 100)
    {
        writer.write_sample(sample.unwrap()).unwrap();
    }
    writer.finalize().unwrap();
    job.media = short;
    std::fs::write(&config, serde_json::to_vec(&job).unwrap()).unwrap();
    let output = dir.path().join("reports/ambiguous.json");
    let result = cli()
        .args(["--quiet", "analyze", "--config"])
        .arg(&config)
        .arg("--output")
        .arg(&output)
        .arg("--require-unambiguous")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    let report: soundgeo::Report =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(report.status, "ambiguous");
    assert_eq!(report.events.len(), 1);
    assert!(!report.candidates.is_empty());
    assert!(report.job.start_utc.is_none());

    // Existing --require-match callers keep their candidate-only behavior.
    let result = cli()
        .args(["--quiet", "analyze", "--config"])
        .arg(&config)
        .args(["--output", "-", "--require-match"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    let compatible: soundgeo::Report = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(compatible.status, report.status);
    let mut expected = serde_json::to_value(&report).unwrap();
    let mut actual = serde_json::to_value(&compatible).unwrap();
    // The two invocations necessarily have different report creation times.
    expected.as_object_mut().unwrap().remove("created_utc");
    actual.as_object_mut().unwrap().remove("created_utc");
    assert!(actual == expected, "The CLI policy changed report evidence");
}

#[test]
fn headless_cli_help_and_errors_have_documented_exit_codes() {
    let no_args = cli().output().unwrap();
    assert!(no_args.status.success());
    assert!(String::from_utf8_lossy(&no_args.stdout).contains("Usage:"));
    let help = cli().args(["analyze", "--help"]).output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--require-match"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--require-unambiguous"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--max-hypotheses"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--search-threads"));
    for budget in ["0", "99", "100000001", "-1", "many"] {
        let invalid = cli()
            .args([
                "analyze",
                "--config",
                "unused.json",
                "--output",
                "-",
                "--max-hypotheses",
                budget,
            ])
            .output()
            .unwrap();
        assert_eq!(invalid.status.code(), Some(2), "invalid budget: {budget}");
        assert!(invalid.stdout.is_empty());
    }
    for workers in ["0", "-1", "many", "18446744073709551616"] {
        let invalid = cli()
            .args([
                "analyze",
                "--config",
                "unused.json",
                "--output",
                "-",
                "--search-threads",
                workers,
            ])
            .output()
            .unwrap();
        assert_eq!(invalid.status.code(), Some(2), "invalid workers: {workers}");
        assert!(invalid.stdout.is_empty());
    }
    let invalid = cli().arg("analyze").output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("broken.json");
    std::fs::write(&config, "not JSON").unwrap();
    let invalid = cli()
        .args(["analyze", "--config"])
        .arg(config)
        .args(["--output", "-"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Invalid job JSON"));
}
