use std::process::Command;

#[test]
fn gap_refresh_rejects_offline_and_unsupported_providers_before_network() {
    let directory = tempfile::tempdir().unwrap();
    for (index, arguments, expected) in [
        (
            0,
            vec![
                "--offline",
                "download",
                "--refresh-gaps",
                "--provider",
                "adsb-exchange-sample",
            ],
            "cannot be used with",
        ),
        (
            1,
            vec!["download", "--refresh-gaps", "--provider", "adsb-lol"],
            "free sample providers",
        ),
    ] {
        let cache = directory.path().join(index.to_string());
        let result = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
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
        for provider in ["adsb-exchange-sample", "adsb-exchange-sample-hires"] {
            let cache = directory.path().join(format!("{provider}-{index}"));
            let result = Command::new(env!("CARGO_BIN_EXE_soundgeo-cli"))
                .args(["download", "--provider", provider])
                .args(&arguments)
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
}
