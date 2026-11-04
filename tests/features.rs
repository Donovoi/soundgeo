use soundgeo::{
    Control, Job,
    aircraft::{AircraftFilter, AircraftKind, Catalog, classify},
    audio,
    geo::{Point, Terrain},
    model::{Sample, Track},
    search::{SearchMode, SilenceMode},
};

fn events() -> (tempfile::TempDir, Job, Vec<audio::Event>) {
    let dir = tempfile::tempdir().unwrap();
    let job = soundgeo::demo::create(dir.path()).unwrap();
    let samples = audio::decode(&job, &Control::default()).unwrap();
    let events = audio::detect(&samples, &job, &Control::default(), &|_, _| {}).unwrap();
    assert_eq!(events.len(), 2);
    (dir, job, events)
}
#[test]
fn aircraft_classification_and_catalog_filtering() {
    assert_eq!(classify("L2T"), AircraftKind::Propeller);
    assert_eq!(classify("H1T"), AircraftKind::Rotorcraft);
    assert_eq!(classify("L2J"), AircraftKind::Jet);
    assert_eq!(classify("CESSNA"), AircraftKind::Unknown);
    let dir = tempfile::tempdir().unwrap();
    let catalog = dir.path().join("catalog.json");
    std::fs::write(&catalog,r#"{"C172":["Cessna","L1P","L"],"A320":["Airbus","L2J","M"],"H60":["Helicopter","H2T","M"]}"#).unwrap();
    let loaded = Catalog::read(&catalog).unwrap();
    assert_eq!(loaded.kind("c172", ""), AircraftKind::Propeller);
    let path = dir.path().join("flights.csv");
    let mut csv = String::from("aircraft,time,lat,lon,altitude_m,type_code\n");
    for (id, code) in [
        ("prop", "C172"),
        ("jet", "A320"),
        ("heli", "H60"),
        ("unknown", ""),
    ] {
        for i in 0..12 {
            // Metadata may appear only once, after initial unclassified samples.
            let code = if i == 2 { code } else { "" };
            csv.push_str(&format!(
                "{id},{},35,{},1000,{code}\n",
                1788861600 + i * 2,
                -110. + i as f64 * 0.0005
            ));
        }
    }
    std::fs::write(&path, csv).unwrap();
    let mut job = Job {
        tracks: path,
        type_catalog: Some(catalog),
        aircraft_filter: AircraftFilter::PropellerOrUnknown,
        ..Default::default()
    };
    let data = soundgeo::data::load_for_job(&job, 230., &Control::default()).unwrap();
    assert_eq!(data.tracks.len(), 2);
    job.aircraft_filter = AircraftFilter::PropellerOnly;
    let data = soundgeo::data::load_for_job(&job, 230., &Control::default()).unwrap();
    assert_eq!(data.tracks.len(), 1);
    assert!(data.tracks[0].id.starts_with("prop/"));
    assert_eq!(
        data.tracks[0].samples.len(),
        12,
        "Late type metadata must preserve earlier positions"
    );
    assert_eq!(data.tracks[0].samples[0].time, 1788861600.);
    job.aircraft_filter = AircraftFilter::All;
    assert_eq!(
        soundgeo::data::load_for_job(&job, 230., &Control::default())
            .unwrap()
            .tracks
            .len(),
        4
    );
}

#[test]
fn indexed_country_extent_search_recovers_unknown_time() {
    let (_dir, mut job, events) = events();
    job.bounds = None;
    job.search_mode = SearchMode::Auto;
    let mut tracks = soundgeo::demo::tracks();
    for i in 0..64 {
        let lat = 25. + (i % 8) as f64 * 3.3;
        let lon = -124. + (i / 8) as f64 * 7.5;
        tracks.push(Track {
            id: format!("distractor-{i}"),
            kind: AircraftKind::Propeller,
            samples: (0..30)
                .map(|j| {
                    Sample::new(
                        soundgeo::demo::EPOCH + (i as f64 + 1.) * 600. + j as f64 * 4.,
                        Point {
                            lat,
                            lon: lon + j as f64 * 0.003,
                            alt: 1400.,
                        },
                    )
                })
                .collect(),
        });
    }
    let mut terrain = Terrain::new(None, job.ground_m);
    let result = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert_eq!(result.method, "indexed_flight_corridors");
    assert!(
        result
            .candidates
            .iter()
            .all(|c| c.recording_clock_rate == 1.),
        "Two flyovers must not fit an underconstrained clock rate"
    );
    assert_eq!(result.coarse_grid_m, job.grid_m);
    let best = result
        .candidates
        .first()
        .expect("national extent must retain the true local basin");
    let error = Point {
        lat: best.latitude,
        lon: best.longitude,
        alt: best.ground_m,
    }
    .distance(soundgeo::demo::TRUTH);
    eprintln!(
        "Indexed extent: {} hypotheses, best error {error:.2} m",
        result.hypotheses
    );
    assert!(error < 200.);
    assert!((best.recording_start_unix - soundgeo::demo::EPOCH).abs() < 3.);
    job.max_hypotheses = 100;
    let result = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    );
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("no partial location")
    );
    let cancelled = Control::default();
    cancelled.stop();
    assert!(soundgeo::index::FlightIndex::new(&tracks, &cancelled).is_err());
}

#[test]
fn quiet_gap_constraint_uses_only_known_closer_propellers() {
    let (_dir, mut job, events) = events();
    let mut tracks = soundgeo::demo::tracks();
    tracks.push(Track {
        id: "unexpected".into(),
        kind: AircraftKind::Propeller,
        samples: (80..=125)
            .map(|t| {
                Sample::new(
                    soundgeo::demo::EPOCH + t as f64,
                    Point {
                        lat: 35.,
                        lon: -110. + (t as f64 - 100.) * 70. / (111320. * 35f64.to_radians().cos()),
                        alt: 500.,
                    },
                )
            })
            .collect(),
    });
    let conflicts = soundgeo::search::silence_conflicts(
        &events,
        &tracks,
        soundgeo::demo::TRUTH,
        soundgeo::demo::EPOCH,
        job.sound_speed(),
    );
    assert_eq!(
        conflicts.len(),
        1,
        "Detected intervals (start, end, CPA, slant): {:?}",
        events
            .iter()
            .map(|e| (e.start_s, e.end_s, e.cpa_s, e.slant_m))
            .collect::<Vec<_>>()
    );
    assert_eq!(conflicts[0].aircraft, "unexpected");
    tracks[2].kind = AircraftKind::Unknown;
    assert!(
        soundgeo::search::silence_conflicts(
            &events,
            &tracks,
            soundgeo::demo::TRUTH,
            soundgeo::demo::EPOCH,
            job.sound_speed()
        )
        .is_empty()
    );
    tracks[2].kind = AircraftKind::Propeller;
    let mut terrain = Terrain::new(None, job.ground_m);
    let soft = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert!(!soft.candidates.is_empty());
    assert!(!soft.candidates[0].silence_conflicts.is_empty());
    job.silence_mode = SilenceMode::Strict;
    let strict = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert!(strict.candidates.len() < soft.candidates.len());
    assert!(
        strict
            .candidates
            .iter()
            .all(|c| c.silence_conflicts.is_empty())
    );
}

#[test]
fn parallel_corridors_match_serial_fallback_terrain() {
    let (dir, mut job, events) = events();
    job.search_mode = SearchMode::FlightCorridors;
    let tracks = soundgeo::demo::tracks();
    // The caller-supplied Terrain is authoritative even if its fallback
    // differs from the Job; workers must not silently reconstruct it.
    let supplied_height = job.ground_m + 7.5;
    let mut terrain = Terrain::new(None, supplied_height);
    let parallel = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    // An empty tile folder exercises the serial terrain-cache path with
    // exactly the same fallback heights and search inputs.
    job.terrain = Some(dir.path().join("missing-tiles"));
    let mut terrain = Terrain::new(job.terrain.clone(), supplied_height);
    let serial = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert_eq!(parallel.hypotheses, serial.hypotheses);
    assert!(!parallel.candidates.is_empty());
    assert_eq!(
        serde_json::to_value(parallel.candidates).unwrap(),
        serde_json::to_value(serial.candidates).unwrap()
    );
}

#[test]
fn unknown_clock_consensus_ignores_an_outlier_but_requires_three_flights() {
    let (_dir, mut job, mut detected) = events();
    let mut tracks = soundgeo::demo::tracks();
    let mut third = detected[1].clone();
    third.start_s += 250.;
    third.end_s += 250.;
    third.cpa_s += 250.;
    for p in &mut third.ridge {
        p.time_s += 250.;
    }
    detected.push(third);
    let mut third_track = tracks[1].clone();
    third_track.id = "THIRD".into();
    for p in &mut third_track.samples {
        p.time += 250.;
    }
    tracks.push(third_track);
    // A curved non-aircraft detection occurs before the real flyovers.
    let mut outlier = detected[0].clone();
    outlier.start_s -= 100.;
    outlier.end_s -= 100.;
    outlier.cpa_s -= 100.;
    for p in &mut outlier.ridge {
        p.time_s -= 100.;
    }
    detected.insert(0, outlier);
    for event in &mut detected {
        event.start_s += 100.;
        event.end_s += 100.;
        event.cpa_s += 100.;
        for p in &mut event.ridge {
            p.time_s += 100.;
        }
    }
    for mode in [SearchMode::RegionalGrid, SearchMode::FlightCorridors] {
        job.search_mode = mode;
        let mut terrain = Terrain::new(None, job.ground_m);
        let result = soundgeo::search::locate(
            &job,
            &detected,
            &tracks,
            &mut terrain,
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        let best = result
            .candidates
            .first()
            .expect("Three consistent flights must survive an unrelated detection");
        assert_eq!(best.matches.len(), 3);
        assert_eq!(
            best.matches
                .iter()
                .map(|m| m.event_index.unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(
            Point {
                lat: best.latitude,
                lon: best.longitude,
                alt: best.ground_m
            }
            .distance(soundgeo::demo::TRUTH)
                < 200.
        );
        assert!((best.recording_start_unix - (soundgeo::demo::EPOCH - 100.)).abs() < 3.);
        let mut wrong = tracks.clone();
        for p in &mut wrong[2].samples {
            p.time += 140.;
        }
        let negative = soundgeo::search::locate(
            &job,
            &detected,
            &wrong,
            &mut terrain,
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        assert!(
            negative.candidates.is_empty(),
            "Two flights alone must not corroborate a recording with four detections"
        );
    }
}

#[test]
fn downloaded_but_short_tracks_report_missing_reference_coverage() {
    let (_dir, mut job, detected) = events();
    job.search_mode = SearchMode::RegionalGrid;
    let mut tracks = soundgeo::demo::tracks();
    for track in &mut tracks {
        let closest = track
            .samples
            .iter()
            .min_by(|a, b| {
                a.point
                    .distance(soundgeo::demo::TRUTH)
                    .total_cmp(&b.point.distance(soundgeo::demo::TRUTH))
            })
            .unwrap()
            .time;
        track.samples.retain(|p| (p.time - closest).abs() <= 12.);
    }
    let result = soundgeo::search::locate(
        &job,
        &detected,
        &tracks,
        &mut Terrain::new(None, job.ground_m),
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert!(result.candidates.is_empty());
    assert!(result.hypotheses > 0);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("reference-position coverage")),
        "Downloaded flight files with incomplete temporal coverage must be distinguished from a failed curve fit: {:?}",
        result.warnings
    );
}

#[test]
fn unknown_media_clock_rate_is_recovered_across_three_flights() {
    let (_dir, mut job, mut detected) = events();
    job.search_mode = SearchMode::FlightCorridors;
    job.silence_mode = SilenceMode::Off;
    let mut tracks = soundgeo::demo::tracks();
    let mut third = detected[1].clone();
    third.start_s += 5000.;
    third.end_s += 5000.;
    third.cpa_s += 5000.;
    for p in &mut third.ridge {
        p.time_s += 5000.;
    }
    detected.push(third);
    let mut flight = tracks[1].clone();
    flight.id = "third-independent-flight".into();
    for sample in &mut flight.samples {
        sample.time += 5000.;
    }
    tracks.push(flight);
    // Simulate a recording clock running short. The independent emission-phase
    // audio fixture supplies the curves; no model prediction generates them.
    let rate = 1.004;
    for event in &mut detected {
        event.start_s /= rate;
        event.end_s /= rate;
        event.cpa_s /= rate;
        event.base_hz *= rate;
        for p in &mut event.ridge {
            p.time_s /= rate;
            p.hz *= rate;
        }
    }
    let result = soundgeo::search::locate(
        &job,
        &detected,
        &tracks,
        &mut Terrain::new(None, job.ground_m),
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    let best = result
        .candidates
        .first()
        .expect("A shared clock-rate error must not discard three valid flyovers");
    assert!(
        Point {
            lat: best.latitude,
            lon: best.longitude,
            alt: best.ground_m
        }
        .distance(soundgeo::demo::TRUTH)
            < 200.
    );
    assert!((best.recording_start_unix - soundgeo::demo::EPOCH).abs() < 3.);
    assert!((best.recording_clock_rate - rate).abs() < 0.0004);
    job.max_clock_drift_ppm = 0.;
    let fixed = soundgeo::search::locate(
        &job,
        &detected,
        &tracks,
        &mut Terrain::new(None, job.ground_m),
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert!(
        fixed.candidates.is_empty(),
        "A fixed clock cannot explain this long recording"
    );
    // A separate timing discontinuity cannot be absorbed by the shared rate.
    job.max_clock_drift_ppm = 5000.;
    for sample in &mut tracks[0].samples {
        sample.time += 100.;
    }
    let inconsistent = soundgeo::search::locate(
        &job,
        &detected,
        &tracks,
        &mut Terrain::new(None, job.ground_m),
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert!(inconsistent.candidates.is_empty());
}

#[test]
fn clock_configuration_limits_and_older_reports_remain_supported() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = soundgeo::demo::create(dir.path()).unwrap();
    for invalid in [-1., 5001., f64::NAN, f64::INFINITY] {
        job.max_clock_drift_ppm = invalid;
        assert!(
            job.validate()
                .unwrap_err()
                .to_string()
                .contains("clock drift")
        );
    }
    for valid in [0., 5000.] {
        job.max_clock_drift_ppm = valid;
        job.validate().unwrap();
    }
    let old_job: Job = serde_json::from_str("{}").unwrap();
    assert_eq!(old_job.max_clock_drift_ppm, 5000.);
    let old = serde_json::json!({"latitude": 35., "longitude": -110., "ground_m": 0.,
        "recording_start_utc": "2026-09-08T10:00:00Z", "recording_start_unix": 1788861600.,
        "relative_error_percent": 0.5, "matches": []});
    let candidate: soundgeo::search::Candidate = serde_json::from_value(old).unwrap();
    assert_eq!(candidate.recording_clock_rate, 1.);
}
