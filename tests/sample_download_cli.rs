use std::process::Command;

#[test]
fn sample_cli_rejects_unavailable_dates_and_missing_bounds_before_network() {
    let directory = tempfile::tempdir().unwrap();
    for (index, arguments, expected) in [
        (
            0,
            vec![
                "--date",
                "2024-03-02",
                "--south",
                "10",
                "--north",
                "11",
                "--west",
                "20",
                "--east",
                "21",
            ],
            "first day",
        ),
        (1, vec!["--date", "2024-03-01"], "bounds"),
        (2, vec!["--date", "2024-03-01", "--south", "10"], "--north"),
    ] {
        let cache = directory.path().join(index.to_string());
        let result = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
            .args(["download", "--provider", "adsb-exchange-sample"])
            .args(arguments)
            .arg("--output")
            .arg(&cache)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!cache.exists());
    }
}
