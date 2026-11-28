use soundgeo::{AnalysisMethod, Control, audio_check};

#[test]
fn checked_audio_reuses_selected_settings_without_changing_the_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = soundgeo::demo::create(dir.path()).unwrap();
    job.min_snr_db = 9.;
    let control = Control::default();
    let info = audio_check::probe(&job, &control).unwrap();
    assert!(info.has_audio && info.duration_s.unwrap() >= 20.);
    let prepared = audio_check::inspect_job(&job, &control, &|_, _| {}).unwrap();
    assert_eq!(prepared.report().detector_settings.min_snr_db, 9.);
    assert!(prepared.report().doppler.enough_observations_to_search);
    assert!(
        !prepared
            .report()
            .passage_timing
            .enough_observations_to_search
    );
    assert!(
        !control.offline && !job.offline,
        "The local check must not make analysis offline"
    );
    let normal = soundgeo::analyze(&job, &control, &|_, _| {}).unwrap();
    let messages = std::sync::Mutex::new(Vec::new());
    let checked = soundgeo::analyze_checked(&job, &prepared, &control, &|m, _| {
        messages.lock().unwrap().push(m.to_string())
    })
    .unwrap();
    assert_eq!(normal.status, checked.status);
    assert_eq!(normal.hypotheses, checked.hypotheses);
    assert_eq!(
        serde_json::to_value(normal.candidates).unwrap(),
        serde_json::to_value(checked.candidates).unwrap()
    );
    assert!(
        !messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("Extracting audio"))
    );
    job.min_snr_db = 11.;
    assert!(
        soundgeo::analyze_checked(&job, &prepared, &control, &|_, _| {})
            .unwrap_err()
            .to_string()
            .contains("settings changed")
    );
    job.min_snr_db = 9.;
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&job.media)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert!(
        soundgeo::analyze_checked(&job, &prepared, &control, &|_, _| {})
            .unwrap_err()
            .to_string()
            .contains("Recording changed")
    );
}

#[test]
fn below_minimum_skips_flights_but_eligible_input_reaches_missing_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = soundgeo::demo::create(dir.path()).unwrap();
    let prepared = audio_check::inspect_job(&job, &Control::default(), &|_, _| {}).unwrap();
    let cache = dir.path().join("empty-flight-cache");
    job.archive = Some(soundgeo::archive::ArchiveRequest {
        date_from: "2026-09-08".into(),
        date_to: None,
        cache: cache.clone(),
        provider: Default::default(),
    });
    job.tracks.clear();
    job.analysis_method = AnalysisMethod::PassageTiming;
    let result =
        soundgeo::analyze_checked(&job, &prepared, &Control::default(), &|_, _| {}).unwrap();
    assert_eq!(result.status, "insufficient_evidence");
    assert!(
        !cache.exists(),
        "An ineligible recording must not prepare flight data"
    );
    job.analysis_method = AnalysisMethod::Doppler;
    let error = soundgeo::analyze_checked(
        &job,
        &prepared,
        &Control::default().with_offline(true),
        &|_, _| {},
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("Offline"), "{error:#}");
}
