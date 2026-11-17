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
    // A simultaneous dense group cannot be divided below the minimum flight
    // timestamp interval. This failure must finish before any curve matching.
    for i in 0..16 {
        let mut coincident = tracks[0].clone();
        coincident.id = format!("coincident-{i}");
        tracks.push(coincident);
    }
    let progress = std::sync::Mutex::new(Vec::new());
    let missing_before = terrain.missing;
    let result = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|message, _| progress.lock().unwrap().push(message.to_owned()),
    );
    let error = result.err().unwrap().to_string();
    assert!(error.contains("no partial location"));
    assert!(error.contains("before curve matching"));
    assert_eq!(terrain.missing, missing_before);
    let messages = progress.into_inner().unwrap();
    assert!(
        messages
            .iter()
            .any(|s| s.starts_with("Checking search budget:"))
    );
    assert!(
        !messages
            .iter()
            .any(|s| s.starts_with("Indexed flight corridors:"))
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
    let mut extended_activity = events.clone();
    extended_activity[0].activity_span_s = Some([events[0].start_s, 120.]);
    assert!(
        soundgeo::search::silence_conflicts(
            &extended_activity,
            &tracks,
            soundgeo::demo::TRUTH,
            soundgeo::demo::EPOCH,
            job.sound_speed()
        )
        .is_empty(),
        "An unmodeled continuation of a tone must not be treated as a quiet gap"
    );
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
    assert!(
        !soft.candidates[0].silence_conflicts.is_empty(),
        "Candidates: {:?}; observations: {:?}",
        soft.candidates
            .iter()
            .map(|c| (
                c.latitude,
                c.longitude,
                c.recording_start_unix - soundgeo::demo::EPOCH,
                c.relative_error_percent,
                c.matches
                    .iter()
                    .map(|m| (m.event_index, m.event_variant, &m.aircraft))
                    .collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>(),
        events
            .iter()
            .map(|e| e
                .variants()
                .map(|v| (v.start_s, v.end_s, v.cpa_s, v.base_hz, v.slant_m))
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
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
fn shared_tiled_corridors_match_legacy_serial_candidates_and_counts() {
    let (dir, mut job, events) = events();
    job.search_mode = SearchMode::FlightCorridors;
    job.search_threads = Some(2);
    let root = dir.path().join("tiles");
    std::fs::create_dir(&root).unwrap();
    let bytes: Vec<_> = (0..1201 * 1201)
        .flat_map(|i| {
            let value = if i % 19 == 0 {
                i16::MIN
            } else {
                (140 + (i % 11)) as i16
            };
            value.to_be_bytes()
        })
        .collect();
    for name in ["N34W110.hgt", "N34W111.hgt", "N35W110.hgt", "N35W111.hgt"] {
        std::fs::write(root.join(name), &bytes).unwrap();
    }
    job.terrain = Some(root.clone());
    let tracks = soundgeo::demo::tracks();
    // An unsupported tile entry far outside all queries forces the original
    // serial loader without changing the actual sampled files or fallback.
    let unsupported = root.join("N00E000.hgt");
    std::fs::create_dir(&unsupported).unwrap();
    let mut terrain = Terrain::new(Some(root.clone()), job.ground_m + 7.5);
    let legacy = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    let expected_missing = terrain.missing;
    assert!(!legacy.candidates.is_empty());
    assert!(expected_missing > 0);
    std::fs::remove_dir(unsupported).unwrap();
    for workers in [1, 2] {
        job.search_threads = Some(workers);
        let mut terrain = Terrain::new(Some(root.clone()), job.ground_m + 7.5);
        let messages = std::sync::Mutex::new(Vec::new());
        let result = soundgeo::search::locate(
            &job,
            &events,
            &tracks,
            &mut terrain,
            &Control::default(),
            &|message, _| messages.lock().unwrap().push(message.to_owned()),
        )
        .unwrap();
        let used = workers.min(std::thread::available_parallelism().map_or(1, |n| n.get()));
        assert!(
            messages
                .lock()
                .unwrap()
                .contains(&format!("Flight-corridor workers: {used}"))
        );
        assert_eq!(result.hypotheses, legacy.hypotheses);
        assert_eq!(
            serde_json::to_vec(&result.candidates).unwrap(),
            serde_json::to_vec(&legacy.candidates).unwrap()
        );
        assert_eq!(result.warnings, legacy.warnings);
        assert_eq!(result.method, legacy.method);
        assert_eq!(
            result.coarse_grid_m.to_bits(),
            legacy.coarse_grid_m.to_bits()
        );
        assert_eq!(
            serde_json::to_vec(&result.partition).unwrap(),
            serde_json::to_vec(&legacy.partition).unwrap()
        );
        assert_eq!(terrain.missing, expected_missing);
    }
    for stage in [
        "Ranking complete Doppler curves",
        "Refining location and shared clock",
    ] {
        let changed = std::sync::atomic::AtomicBool::new(false);
        let mut terrain = Terrain::new(Some(root.clone()), job.ground_m + 7.5);
        let result = soundgeo::search::locate(
            &job,
            &events,
            &tracks,
            &mut terrain,
            &Control::default(),
            &|message, _| {
                if message.starts_with(stage)
                    && !changed.swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    use std::io::Write;
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(root.join("N35W110.hgt"))
                        .unwrap()
                        .write_all(&300_i16.to_be_bytes())
                        .unwrap();
                }
            },
        );
        assert!(
            changed.load(std::sync::atomic::Ordering::Relaxed),
            "{stage} not reached"
        );
        let error = result
            .err()
            .expect("changed terrain must not return a location");
        assert!(error.to_string().contains("Terrain"), "{error:#}");
        std::fs::write(root.join("N35W110.hgt"), &bytes).unwrap();
    }
    let control = Control::default();
    let mut terrain = Terrain::new(Some(root), job.ground_m + 7.5);
    assert!(
        soundgeo::search::locate(
            &job,
            &events,
            &tracks,
            &mut terrain,
            &control,
            &|message, _| {
                if message.starts_with("Ranking complete Doppler curves") {
                    control.stop();
                }
            }
        )
        .is_err()
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
    job.search_threads = Some(8);
    job.max_hypotheses = parallel.hypotheses;
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
    let expected_candidates = serde_json::to_value(&parallel.candidates).unwrap();
    assert_eq!(
        expected_candidates,
        serde_json::to_value(serial.candidates).unwrap()
    );
    for workers in [1, 8] {
        job.search_threads = Some(workers);
        job.terrain = None;
        let mut worker_terrain = Terrain::new(None, supplied_height);
        let messages = std::sync::Mutex::new(Vec::new());
        let result = soundgeo::search::locate(
            &job,
            &events,
            &tracks,
            &mut worker_terrain,
            &Control::default(),
            &|message, _| messages.lock().unwrap().push(message.to_owned()),
        )
        .unwrap();
        let used = workers.min(std::thread::available_parallelism().map_or(1, |n| n.get()));
        assert!(
            messages
                .lock()
                .unwrap()
                .contains(&format!("Flight-corridor workers: {used}"))
        );
        assert_eq!(result.hypotheses, parallel.hypotheses);
        assert_eq!(
            serde_json::to_value(result.candidates).unwrap(),
            expected_candidates,
            "worker limit {workers} changed the search"
        );
    }
    job.terrain = Some(dir.path().join("missing-tiles"));
    // One fewer proposal now triggers a complete automatic partition, including
    // on the serial terrain path. Both flyovers must remain available to a fit
    // even when their anchor proposals belong to different timestamp batches.
    job.max_hypotheses -= 1;
    let job_before = serde_json::to_value(&job).unwrap();
    let messages = std::sync::Mutex::new(Vec::new());
    let partitioned = soundgeo::search::locate(
        &job,
        &events,
        &tracks,
        &mut terrain,
        &Control::default(),
        &|message, _| messages.lock().unwrap().push(message.to_owned()),
    )
    .unwrap();
    let partition = partitioned.partition.as_ref().unwrap();
    assert!(partition.complete && partition.batches.len() >= 2);
    assert!(
        partition
            .batches
            .iter()
            .all(|b| b.hypotheses <= job.max_hypotheses)
    );
    assert_eq!(partitioned.hypotheses, parallel.hypotheses);
    assert_eq!(partitioned.candidates[0].matches.len(), events.len());
    let best = &partitioned.candidates[0];
    assert!(
        Point {
            lat: best.latitude,
            lon: best.longitude,
            alt: best.ground_m
        }
        .distance(soundgeo::demo::TRUTH)
            < 250.
    );
    assert!(job.start_utc.is_none());
    assert_eq!(serde_json::to_value(&job).unwrap(), job_before);
    assert!(
        messages
            .into_inner()
            .unwrap()
            .iter()
            .any(|s| s.contains("Indexed flight corridors:"))
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
    assert!(
        best.clock_fit_is_provisional(),
        "Recovering a rate from the minimum three sounds is not independent corroboration"
    );
    // A fourth independently timed pass supplies evidence beyond that minimum.
    // Keep this separate from the three-flight fixed-rate/discontinuity controls.
    let mut four_events = detected.clone();
    let mut fourth = four_events[0].clone();
    fn shift_event(event: &mut soundgeo::audio::Event, seconds: f64) {
        event.start_s += seconds;
        event.end_s += seconds;
        event.cpa_s += seconds;
        for point in &mut event.ridge {
            point.time_s += seconds;
        }
        for alternate in &mut event.alternatives {
            shift_event(alternate, seconds);
        }
    }
    shift_event(&mut fourth, 2500. / rate);
    four_events.push(fourth);
    let mut four_tracks = tracks.clone();
    let mut fourth_flight = four_tracks[0].clone();
    fourth_flight.id = "fourth-independent-flight".into();
    for sample in &mut fourth_flight.samples {
        sample.time += 2500.;
    }
    four_tracks.push(fourth_flight);
    let corroborated = soundgeo::search::locate(
        &job,
        &four_events,
        &four_tracks,
        &mut Terrain::new(None, job.ground_m),
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    let corroborated = corroborated.candidates.first().unwrap();
    assert_eq!(corroborated.matches.len(), 4);
    assert!(!corroborated.clock_fit_is_provisional());
    assert!((corroborated.recording_clock_rate - rate).abs() < 0.0004);
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

#[test]
fn worker_limits_validate_and_preserve_older_job_json() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = Job {
        media: dir.path().join("audio.wav"),
        tracks: dir.path().join("flights.csv"),
        ..Job::default()
    };
    std::fs::write(&job.media, []).unwrap();
    std::fs::write(&job.tracks, []).unwrap();
    job.search_threads = Some(0);
    assert!(job.validate().unwrap_err().to_string().contains("workers"));
    for valid in [None, Some(1), Some(4), Some(8), Some(usize::MAX)] {
        job.search_threads = valid;
        job.validate().unwrap();
        let saved = serde_json::to_value(&job).unwrap();
        assert_eq!(saved.get("search_threads").is_some(), valid.is_some());
        let loaded: Job = serde_json::from_value(saved).unwrap();
        assert_eq!(loaded.search_threads, valid);
    }
    let old: Job = serde_json::from_str("{}").unwrap();
    assert_eq!(old.search_threads, None);
}
