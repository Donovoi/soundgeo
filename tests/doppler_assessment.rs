use soundgeo::{
    Report,
    doppler_assessment::{Competition, assess},
    search::Candidate,
};

fn report(detections: usize, matches: usize) -> Report {
    let event = serde_json::json!({"start_s":0.,"end_s":10.,"cpa_s":5.,
        "base_hz":1000.,"speed_mps":70.,"slant_m":1000.,"fit_rmse_hz":2.,
        "contrast_db":20.,"ridge":[]});
    let candidate = serde_json::json!({"latitude":0.,"longitude":0.,"ground_m":0.,
        "recording_start_utc":"1970-01-01T00:00:00Z","recording_start_unix":0.,
        "relative_error_percent":0.2,"matches":(0..matches).map(|i| serde_json::json!({
            "event_index":i,"aircraft":format!("flight-{i}"),"rmse_hz":2.,
            "fitted_tone_hz":1000.,"coverage":1.,"linear_baseline_rmse_hz":10.,"modeled_hz":[]
        })).collect::<Vec<_>>()});
    serde_json::from_value(serde_json::json!({
        "version":"test","status":"ambiguous","media":"unused.wav","job":soundgeo::Job::default(),
        "duration_s":1800.,"track_count":matches,"events":vec![event;detections],
        "candidates":[candidate],"warnings":[],"search_grid_m":200.,"created_utc":"test"
    }))
    .unwrap()
}

fn at_distance(mut candidate: Candidate, meters: f64) -> Candidate {
    candidate.longitude = (2. * (meters / (2. * 6_378_137.)).asin()).to_degrees();
    candidate
}

#[test]
fn moving_best_point_cannot_hide_a_previously_nearby_competitor() {
    let mut r = report(11, 10);
    let old = r.candidates[0].clone();
    let left = at_distance(old.clone(), -900.);
    let mut evidence = Competition::from_candidates(&[old, left]);
    let better = at_distance(report(11, 11).candidates.remove(0), 900.);
    evidence.merge(Competition::from_candidates(std::slice::from_ref(&better)));
    r.candidates = vec![better];
    r.doppler_competition = evidence;
    assert_eq!(assess(&r).unwrap().distant_competitors.len(), 1);
}

#[test]
fn measured_shuffled_false_match_still_fails_consensus() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/doppler-ranking-counterexample.json")).unwrap();
    // The original source report had 16 detections (corridor-worker-checks.json).
    // Candidate curves are measured; placeholder event bodies are unused by the screen.
    for key in ["baseline", "control"] {
        let mut r = report(16, 0);
        r.candidates = serde_json::from_value(fixture[key].clone()).unwrap();
        r.doppler_competition = Competition::from_candidates(&r.candidates);
        let result = assess(&r).unwrap();
        assert!(!result.passes);
        assert!(result.reasons.contains(&"insufficient_consensus".into()));
    }
}
