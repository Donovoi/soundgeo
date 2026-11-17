use soundgeo::{
    Job, Report,
    audio::{Event, RidgePoint},
    search::{Candidate, Match},
};

#[test]
fn saved_report_preserves_ieee_observations_and_modeled_curves() {
    // Observed saved-report counterexamples: default JSON parsing moved each
    // value by one ULP before any replay search was performed.
    const MODELED_HZ: u64 = 0x4078_a644_d14d_0e39;
    const RIDGE_HZ: u64 = 0x406f_3710_cc0b_3000;
    const CONTRAST_DB: u64 = 0x4033_8e37_c000_0000;
    let mut event = Event {
        start_s: 10.,
        end_s: 20.,
        cpa_s: 15.,
        base_hz: f64::from_bits(RIDGE_HZ),
        speed_mps: 40.,
        slant_m: 100.,
        fit_rmse_hz: 0.5,
        contrast_db: f64::from_bits(CONTRAST_DB),
        ridge: vec![RidgePoint {
            time_s: 15.,
            hz: f64::from_bits(RIDGE_HZ),
            contrast_db: f64::from_bits(CONTRAST_DB),
        }],
        activity_span_s: Some([10., 20.]),
        alternatives: vec![],
    };
    event.alternatives.push(event.clone());
    let report = Report {
        version: "roundtrip-regression".into(),
        status: "ambiguous".into(),
        media: "unopened-fixture.wav".into(),
        job: Job::default(),
        duration_s: 900.,
        track_count: 1,
        events: vec![event],
        candidates: vec![Candidate {
            latitude: 45.,
            longitude: -90.,
            ground_m: 0.,
            recording_start_utc: "2026-01-01T00:00:00Z".into(),
            recording_start_unix: 1767225600.,
            recording_clock_rate: 1.,
            relative_error_percent: 1.,
            matches: vec![Match {
                event_index: Some(0),
                event_variant: 1,
                aircraft: "fixture".into(),
                rmse_hz: 0.5,
                fitted_tone_hz: f64::from_bits(MODELED_HZ),
                coverage: 1.,
                linear_baseline_rmse_hz: Some(1.),
                modeled_hz: vec![f64::from_bits(MODELED_HZ)],
            }],
            silence_conflicts: vec![],
        }],
        warnings: vec!["Fixture only; no analysis was run.".into()],
        search_grid_m: 200.,
        search_method: "fixture".into(),
        hypotheses: 1,
        created_utc: "2026-01-01T00:00:00Z".into(),
        reference_sources: vec![],
        terrain_sources: vec![],
        timing: None,
        date_search: None,
        doppler_partitions: vec![],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved/report.json");
    soundgeo::save_report(&report, &path).unwrap();
    let restored: Report = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        restored.candidates[0].matches[0].modeled_hz[0].to_bits(),
        MODELED_HZ
    );
    for trace in restored.events[0].variants() {
        assert_eq!(trace.ridge[0].hz.to_bits(), RIDGE_HZ);
        assert_eq!(trace.ridge[0].contrast_db.to_bits(), CONTRAST_DB);
    }
    assert_eq!(
        serde_json::to_vec(&restored).unwrap(),
        serde_json::to_vec(&report).unwrap(),
        "Saving and loading a report must preserve every field exactly"
    );
}
