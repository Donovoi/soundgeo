use soundgeo::{AnalysisMethod, Control, Job, data};

fn job_for(path: std::path::PathBuf) -> Job {
    Job {
        tracks: path,
        analysis_method: AnalysisMethod::PassageTiming,
        smooth_tracks: false,
        ..Default::default()
    }
}

#[test]
fn timing_import_keeps_short_recorded_paths_without_relaxing_doppler() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("short.csv");
    let mut csv = "aircraft,time,lat,lon,altitude_m\n".to_owned();
    for (id, count) in [("three", 3), ("two", 2), ("eight", 8)] {
        for i in 0..count {
            csv.push_str(&format!(
                "{id},{},41,{},150\n",
                10000 + 5 * i,
                -87. + i as f64 * 0.003
            ));
        }
    }
    std::fs::write(&file, csv).unwrap();
    let job = job_for(file.clone());
    let control = Control::default();
    let timing = data::load_for_job(&job, 1200., &control).unwrap();
    assert_eq!(
        timing
            .tracks
            .iter()
            .map(|t| (t.id.as_str(), t.samples.len()))
            .collect::<Vec<_>>(),
        [("eight/0", 8), ("three/0", 3)]
    );
    assert_eq!(
        timing.tracks[1]
            .samples
            .iter()
            .map(|s| s.time)
            .collect::<Vec<_>>(),
        [10000., 10005., 10010.]
    );
    assert!(
        timing
            .warnings
            .iter()
            .any(|s| s.contains("1 short flight segments"))
    );
    let doppler = data::load_for_job(
        &Job {
            analysis_method: AnalysisMethod::Doppler,
            ..job
        },
        1200.,
        &control,
    )
    .unwrap();
    assert_eq!(doppler.tracks.len(), 1);
    assert_eq!(doppler.tracks[0].id, "eight/0");
    assert_eq!(data::load(&file, &control).unwrap().tracks.len(), 1);
    control.stop();
    assert!(data::load_for_job(&job_for(file), 1200., &control).is_err());
}

#[test]
fn short_trace_import_still_thins_duplicates_and_splits_gaps_and_bad_speeds() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("trace_full_test.json");
    let write = |rows: serde_json::Value| {
        std::fs::write(
            &file,
            serde_json::to_vec(&serde_json::json!({"icao":"test","timestamp":10000,"trace":rows}))
                .unwrap(),
        )
        .unwrap();
    };
    let row = |time: i32, lon: f64| {
        serde_json::json!([time, 41., lon, 500, 100, 90, 0, 0, null, null, 500])
    };
    write(serde_json::json!([
        row(0, -87.),
        row(1, -86.999),
        row(2, -86.998)
    ]));
    assert!(
        data::load_for_job(&job_for(file.clone()), 1200., &Control::default())
            .unwrap_err()
            .to_string()
            .contains("at least 3")
    );
    write(serde_json::json!([
        row(0, -87.),
        row(5, -86.997),
        row(10, -86.994),
        row(100, -87.),
        row(105, -86.997),
        row(110, -86.994)
    ]));
    let data = data::load_for_job(&job_for(file.clone()), 1200., &Control::default()).unwrap();
    assert_eq!(data.tracks.len(), 2);
    assert!(data.tracks.iter().all(|track| track.samples.len() == 3));
    assert_eq!(data.tracks[0].samples.last().unwrap().time, 10010.);
    assert_eq!(data.tracks[1].samples[0].time, 10100.);
    write(serde_json::json!([
        row(0, -87.),
        row(5, -86.997),
        row(10, -80.)
    ]));
    assert!(data::load_for_job(&job_for(file), 1200., &Control::default()).is_err());
}

#[test]
fn imported_three_position_flights_recover_one_unknown_clock() {
    use soundgeo::{geo::Bounds, timing};
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("passes.csv");
    let mut job = job_for(file.clone());
    job.bounds = Some(Bounds {
        south: 40.985,
        north: 41.015,
        west: -87.02,
        east: -86.98,
    });
    let emissions = [
        40., 99., 181., 262., 393., 471., 592., 611., 722., 831., 932., 1044.,
    ];
    let offset = 192800.;
    let mut csv = "aircraft,time,lat,lon,altitude_m\n".to_owned();
    for (i, time) in emissions.iter().enumerate() {
        let angle = i as f64 * 0.7;
        for relative in [-20., 0., 20.] {
            let north = relative * 50. * angle.sin();
            let east = relative * 50. * angle.cos();
            csv.push_str(&format!(
                "flight-{i},{},{},{},150\n",
                offset + time + relative,
                41. + north / 111000.,
                -87. + east / (111000. * 41_f64.to_radians().cos())
            ));
        }
    }
    std::fs::write(file, csv).unwrap();
    let control = Control::default();
    let data = data::load_for_job(&job, 1200., &control).unwrap();
    assert_eq!(data.tracks.len(), 12);
    assert!(data.tracks.iter().all(|track| track.samples.len() == 3));
    assert!(job.start_utc.is_none());
    let peaks = emissions
        .iter()
        .map(|time| timing::Peak {
            time_s: time + 150. / job.sound_speed(),
            excess_db: 12.,
            width_s: 2.,
        })
        .collect();
    let assessment = timing::assess(
        &job,
        peaks,
        1200.,
        &data.tracks,
        &job.timing_options,
        &control,
        &|_, _| {},
    )
    .unwrap();
    let best = &assessment.search.as_ref().unwrap().candidates[0];
    assert!((best.offset_utc - offset).abs() < 0.1);
    assert_eq!(best.pairs.len(), 12);
    assert_eq!(assessment.control_maximum_scores.len(), 19);
    assert_eq!(assessment.screen, timing::Screen::TimingCandidate);
    assert!((best.latitude - 41.).abs() < 0.002);
    assert!((best.longitude + 87.).abs() < 0.002);
}
