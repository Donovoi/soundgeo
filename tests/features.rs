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
    assert_eq!(conflicts.len(), 1);
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
