use soundgeo::{
    Control, Job, audio, data,
    geo::{Bounds, Point},
    model::{Sample, Track},
};

#[test]
fn propagation_delay_and_doppler_have_correct_sign() {
    let observer = Point {
        lat: 0.,
        lon: 0.,
        alt: 0.,
    };
    let track = Track {
        id: "test".into(),
        kind: Default::default(),
        samples: (-20..=20)
            .map(|i| {
                Sample::new(
                    1000. + i as f64,
                    Point {
                        lat: 0.,
                        lon: i as f64 * 70. / 111320.,
                        alt: 1000.,
                    },
                )
            })
            .collect(),
    };
    let early = track.factor(observer.ecef(), 990., 340.).unwrap();
    let late = track.factor(observer.ecef(), 1015., 340.).unwrap();
    assert!(early > 1. && late < 1.);
    let (arrival, distance, _) = track.cpa(observer.ecef(), 340.).unwrap();
    assert!((distance - 1000.).abs() < 0.1);
    assert!((arrival - 1000. - 1000. / 340.).abs() < 0.01);
    assert!(track.factor(observer.ecef(), 900., 340.).is_none());
}

#[test]
fn missing_track_gaps_are_not_interpolated() {
    let p = Point {
        lat: 35.,
        lon: -110.,
        alt: 1000.,
    };
    let track = Track {
        id: "gap".into(),
        kind: Default::default(),
        samples: vec![Sample::new(1000., p), Sample::new(1100., p)],
    };
    assert!(
        track
            .factor(Point { alt: 0., ..p }.ecef(), 1050., 340.)
            .is_none()
    );
}

#[test]
fn import_rejects_invalid_positions_and_retains_absolute_time() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("flights.csv");
    std::fs::write(
        &file,
        "aircraft,time,lat,lon,altitude_m\na,1000,NaN,0,1000\n",
    )
    .unwrap();
    assert!(
        data::load(&file, &Control::default())
            .unwrap_err()
            .to_string()
            .contains("Invalid position")
    );
    assert_eq!(
        data::parse_time("2026-09-08T10:00:00Z").unwrap(),
        1788861600.
    );
    assert!(data::parse_time("NaN").is_err());
    assert!(data::parse_time("2026-09-08 10:00:00").is_err());
}

#[test]
fn noise_and_stationary_hum_do_not_generate_events() {
    let mut state = 41u64;
    let samples: Vec<f32> = (0..audio::SAMPLE_RATE * 45)
        .map(|i| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (state >> 32) as u32 as f32 / u32::MAX as f32 - 0.5;
            noise * 0.2
                + 0.1 * (std::f32::consts::TAU * 100. * i as f32 / audio::SAMPLE_RATE as f32).sin()
        })
        .collect();
    let events = audio::detect(&samples, &Job::default(), &Control::default(), &|_, _| {}).unwrap();
    assert!(events.is_empty(), "noise created {} events", events.len());
    let silent = vec![0.; audio::SAMPLE_RATE * 30];
    assert!(
        audio::detect(&silent, &Job::default(), &Control::default(), &|_, _| {})
            .unwrap()
            .is_empty()
    );
}

#[test]
fn grid_budget_and_input_bounds_are_enforced() {
    let b = Bounds {
        south: 20.,
        north: 50.,
        west: -130.,
        east: -70.,
    };
    let (points, spacing) = b.grid(200., 6000);
    assert!(points.len() <= 6000);
    assert!(spacing > 200.);
    assert!(
        Bounds {
            west: 170.,
            east: -170.,
            ..b
        }
        .validate()
        .is_err()
    );
    let c = Control::default();
    c.stop();
    assert!(c.check().is_err());
}

#[test]
fn readsb_trace_uses_geometric_altitude_and_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.json");
    let trace: Vec<_> = (0..24)
        .map(|i| {
            serde_json::json!([
                i,
                35.0,
                -110. + i as f64 * 0.0005,
                3000,
                100,
                90,
                0,
                0,
                null,
                "adsb_icao",
                4000
            ])
        })
        .collect();
    std::fs::write(
        &path,
        serde_json::to_vec(
            &serde_json::json!({"icao":"abc123","timestamp":1788861600.,"trace":trace}),
        )
        .unwrap(),
    )
    .unwrap();
    let data = data::load(&path, &Control::default()).unwrap();
    assert_eq!(data.tracks.len(), 1);
    assert_eq!(data.tracks[0].samples[0].time, 1788861600.);
    assert!((data.tracks[0].samples[0].point.alt - 1219.2).abs() < 1e-6);
}

#[test]
fn pressure_altitude_warning_counts_only_imported_positions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.json");
    let mut trace: Vec<_> = (0..24)
        .map(|i| {
            serde_json::json!([
                i,
                35.,
                -110. + i as f64 * 0.0005,
                3000,
                100,
                90,
                0,
                0,
                null,
                "adsb_icao",
                4000
            ])
        })
        .collect();
    // Pressure-only positions outside the chosen region must not warn about
    // the geometric-altitude track actually used by the solver.
    trace.push(serde_json::json!([30, 50., 0., 3000, 100, 90, 0, 0]));
    // Nor may a point removed by the two-second input sampling count.
    trace.insert(1, serde_json::json!([0.5, 35., -110., 3000, 100, 90, 0, 0]));
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "icao": "abc123", "timestamp": 1788861600., "trace": trace
        }))
        .unwrap(),
    )
    .unwrap();
    let job = Job {
        tracks: path,
        bounds: Some(Bounds {
            south: 34.9,
            north: 35.1,
            west: -110.1,
            east: -109.9,
        }),
        ..Default::default()
    };
    let result = data::load_for_job(&job, 30., &Control::default()).unwrap();
    assert_eq!(result.tracks[0].samples.len(), 12);
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| w.contains("pressure altitude")),
        "{:?}",
        result.warnings
    );
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&job.tracks).unwrap()).unwrap();
    value["trace"][0][10] = serde_json::Value::Null;
    std::fs::write(&job.tracks, serde_json::to_vec(&value).unwrap()).unwrap();
    let pressure = data::load_for_job(&job, 30., &Control::default()).unwrap();
    assert!(
        pressure
            .warnings
            .iter()
            .any(|w| w.starts_with("1 positions use pressure altitude"))
    );
}

#[test]
fn synthetic_video_unknown_clock_recovers_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = soundgeo::demo::create(dir.path()).unwrap();
    // Exercise actual video -> FFmpeg -> spectrum -> detection -> flight matching.
    let video = dir.path().join("synthetic.mp4");
    let result = audio::quiet_command(audio::ffmpeg_path(&job))
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x101928:s=160x90:r=1",
            "-i",
        ])
        .arg(&job.media)
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-shortest",
        ])
        .arg(&video)
        .status()
        .expect("FFmpeg is required for the end-to-end test");
    assert!(result.success());
    job.media = video;
    let report =
        soundgeo::analyze(&job, &Control::default(), &|m, p| eprintln!("{p:.2} {m}")).unwrap();
    eprintln!(
        "Events: {:?}",
        report
            .events
            .iter()
            .map(|e| (e.cpa_s, e.base_hz, e.speed_mps, e.slant_m, e.fit_rmse_hz))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        report.events.len(),
        2,
        "should count two independent flyovers, not their harmonics"
    );
    let best = report
        .candidates
        .first()
        .expect("synthetic data should produce candidates");
    let estimate = Point {
        lat: best.latitude,
        lon: best.longitude,
        alt: best.ground_m,
    };
    eprintln!(
        "Best candidate distance from truth: {} m",
        estimate.distance(soundgeo::demo::TRUTH)
    );
    assert!(
        estimate.distance(soundgeo::demo::TRUTH) < 200.,
        "synthetic best candidate must be within 200 m"
    );
    assert!(
        (best.recording_start_unix - soundgeo::demo::EPOCH).abs() < 3.,
        "must infer the shared recording clock"
    );
    let path = dir.path().join("report.json");
    let input_size = std::fs::metadata(&job.media).unwrap().len();
    assert!(soundgeo::save_report(&report, &job.media).is_err());
    assert_eq!(std::fs::metadata(&job.media).unwrap().len(), input_size);
    soundgeo::save_report(&report, &path).unwrap();
    let parsed: soundgeo::Report =
        serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
    assert_eq!(parsed.candidates.len(), report.candidates.len());

    // Breaking the relative flight timing must remove the match; independent
    // per-event clock adjustments would incorrectly keep it.
    let mut wrong_tracks = soundgeo::demo::tracks();
    for sample in &mut wrong_tracks[1].samples {
        sample.time += 400.;
    }
    let mut terrain = soundgeo::geo::Terrain::new(None, soundgeo::demo::TRUTH.alt);
    let wrong = soundgeo::search::locate(
        &job,
        &report.events,
        &wrong_tracks,
        &mut terrain,
        &Control::default(),
        &|_, _| {},
    )
    .unwrap();
    assert!(
        wrong.candidates.is_empty(),
        "inconsistent two-event timing must not produce a location"
    );
}

#[test]
fn split_archive_streams_gzip_traces_without_extraction() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let trace: Vec<_> = (0..24)
        .map(|i| serde_json::json!([i, 35., -110. + i as f64 * 0.0005, 4000, 100, 90, 8, 0, null]))
        .collect();
    let bytes = serde_json::to_vec(
        &serde_json::json!({"icao":"abc123","timestamp":1788861600.,"trace":trace}),
    )
    .unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&bytes).unwrap();
    let compressed = gzip.finish().unwrap();
    let mut archive = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(compressed.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            "2026/09/08/traces/23/trace_full_abc123.json",
            &compressed[..],
        )
        .unwrap();
    let data = archive.into_inner().unwrap();
    let cut = data.len() / 2 + 13;
    let first = dir.path().join("day.tar.aa");
    std::fs::write(&first, &data[..cut]).unwrap();
    std::fs::write(dir.path().join("day.tar.ab"), &data[cut..]).unwrap();
    let imported = data::load(&first, &Control::default()).unwrap();
    assert_eq!(imported.tracks.len(), 1);
    assert!(!dir.path().join("2026").exists());
}

#[test]
fn srtm_big_endian_and_missing_tiles_use_explicit_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = 123i16
        .to_be_bytes()
        .into_iter()
        .cycle()
        .take(1201 * 1201 * 2)
        .collect();
    std::fs::write(dir.path().join("N35W110.hgt"), bytes).unwrap();
    let mut terrain = soundgeo::geo::Terrain::new(Some(dir.path().to_owned()), 42.);
    assert_eq!(
        terrain.elevation(Point {
            lat: 35.25,
            lon: -109.75,
            alt: 0.
        }),
        // Independently evaluated with PROJ vgridshift and the bundled grid.
        123. - 23.394_083_023_071_29
    );
    assert_eq!(
        terrain.elevation(Point {
            lat: -35.25,
            lon: -109.75,
            alt: 0.
        }),
        42.
    );
    assert_eq!(terrain.missing, 1);
}
