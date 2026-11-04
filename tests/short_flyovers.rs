use soundgeo::{Control, Job, audio};

fn signal(seconds: usize, passes: &[(f64, f64, f64, f64)]) -> Vec<f32> {
    synthesize(seconds, passes, false)
}

fn synthesize(seconds: usize, passes: &[(f64, f64, f64, f64)], harmonic: bool) -> Vec<f32> {
    let rate = audio::SAMPLE_RATE;
    let c = 340.39;
    let mut random = 83_u64;
    (0..rate * seconds)
        .map(|i| {
            let receive = i as f64 / rate as f64;
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let mut value = (random >> 32) as u32 as f64 / u32::MAX as f64 * 0.03
                + 2.0 * (std::f64::consts::TAU * 65. * receive).sin()
                + 0.2 * (std::f64::consts::TAU * 1700. * receive).sin();
            for &(center, speed, slant, tone) in passes {
                // Independently synthesize phase at emission, solving propagation
                // delay by bisection instead of the detector's straight_factor.
                let mut lo = receive - 20.;
                let mut hi = receive;
                for _ in 0..35 {
                    let emit = (lo + hi) * 0.5;
                    let distance = f64::hypot(speed * (emit - center), slant);
                    if emit + distance / c < receive {
                        lo = emit;
                    } else {
                        hi = emit;
                    }
                }
                let emit = (lo + hi) * 0.5;
                let distance = f64::hypot(speed * (emit - center), slant);
                let envelope = (1. - ((emit - center) / 15.).powi(2)).max(0.);
                let phase = std::f64::consts::TAU * tone * emit;
                let wave = if harmonic {
                    0.08 * phase.sin() + (2. * phase).sin()
                } else {
                    phase.sin()
                };
                value += envelope * slant / distance * wave;
            }
            value as f32
        })
        .collect()
}

#[test]
fn upper_jet_harmonics_are_retained_with_a_faint_fundamental() {
    let samples = synthesize(
        90,
        &[(22., 77., 130., 1300.), (65., 86., 250., 1400.)],
        true,
    );
    let events = audio::detect(&samples, &Job::default(), &Control::default(), &|_, _| {}).unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    for (event, center) in events
        .iter()
        .zip([22. + 130. / 340.39, 65. + 250. / 340.39])
    {
        assert!((event.cpa_s - center).abs() < 1.);
        assert!(
            (2400. ..3000.).contains(&event.base_hz),
            "{}",
            event.base_hz
        );
    }
}

#[test]
fn short_jet_tones_survive_loud_bass_and_are_not_counted_twice() {
    let c = 340.39;
    let samples = signal(90, &[(22., 77., 130., 1180.), (65., 86., 250., 1250.)]);
    let events = audio::detect(&samples, &Job::default(), &Control::default(), &|_, _| {}).unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    for (event, center) in events.iter().zip([22. + 130. / c, 65. + 250. / c]) {
        assert!(
            (event.cpa_s - center).abs() < 1.,
            "{} vs {center}",
            event.cpa_s
        );
        assert!((1000. ..1400.).contains(&event.base_hz));
    }
    let slow_only = Job {
        fast_flyovers: false,
        ..Job::default()
    };
    assert!(
        audio::detect(&samples, &slow_only, &Control::default(), &|_, _| {})
            .unwrap()
            .is_empty()
    );
}

#[test]
fn longer_recording_retains_evidence_beyond_the_eight_search_anchors() {
    let passes: Vec<_> = (0..10)
        .map(|i| (22. + i as f64 * 40., 77., 130., 1180.))
        .collect();
    let samples = signal(410, &passes);
    let events = audio::detect(&samples, &Job::default(), &Control::default(), &|_, _| {}).unwrap();
    assert_eq!(events.len(), 10);
    for &(center, _, slant, _) in &passes {
        assert!(
            events
                .iter()
                .any(|e| (e.cpa_s - center - slant / 340.39).abs() < 1.)
        );
    }
}

#[test]
fn long_propeller_pass_retains_both_sides_of_its_frequency_curve() {
    let (speed, slant, tone, center, c) = (110_f64, 4000_f64, 120_f64, 110_f64, 340.39_f64);
    let rate = audio::SAMPLE_RATE;
    let mut random = 971_u64;
    let samples: Vec<f32> = (0..240 * rate)
        .map(|i| {
            let receive = i as f64 / rate as f64;
            let (mut lo, mut hi) = (receive - 120., receive);
            for _ in 0..32 {
                let emit = (lo + hi) * 0.5;
                if emit + f64::hypot(speed * (emit - center), slant) / c < receive {
                    lo = emit;
                } else {
                    hi = emit;
                }
            }
            let emit = (lo + hi) * 0.5;
            let distance = f64::hypot(speed * (emit - center), slant);
            let envelope = (1. - ((emit - center) / 100.).powi(2)).max(0.);
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (random >> 32) as u32 as f64 / u32::MAX as f64 - 0.5;
            (0.03 * noise
                + (std::f64::consts::TAU * 65. * receive).sin()
                + 0.2 * (std::f64::consts::TAU * 180. * receive).sin()
                + envelope * slant / distance * (std::f64::consts::TAU * tone * emit).sin())
                as f32
        })
        .collect();
    let job = Job {
        fast_flyovers: false,
        ..Default::default()
    };
    let events = audio::detect(&samples, &job, &Control::default(), &|_, _| {}).unwrap();
    let received_cpa = center + slant / c;
    let event = events
        .iter()
        .min_by(|a, b| {
            (a.cpa_s - received_cpa)
                .abs()
                .total_cmp(&(b.cpa_s - received_cpa).abs())
        })
        .expect("A clear long flyover must be detected");
    assert!(
        (event.cpa_s - received_cpa).abs() < 3.,
        "CPA {} vs {}; interval {}..{}",
        event.cpa_s,
        received_cpa,
        event.start_s,
        event.end_s
    );
    assert!(
        event.end_s - event.start_s > 120.,
        "The 90-second scan must not truncate a longer supported curve: {}..{}",
        event.start_s,
        event.end_s
    );
    assert!((event.speed_mps / speed - 1.).abs() < 0.15);
    assert!((event.slant_m / slant - 1.).abs() < 0.2);
}

#[test]
fn persistent_tones_do_not_exhaust_the_slow_curve_tracing_budget() {
    let (speed, slant, tone, center, c) = (80_f64, 1800_f64, 120_f64, 70_f64, 340.39_f64);
    let rate = audio::SAMPLE_RATE;
    let mut random = 3983_u64;
    let samples: Vec<f32> = (0..160 * rate)
        .map(|i| {
            let receive = i as f64 / rate as f64;
            let (mut lo, mut hi) = (receive - 90., receive);
            for _ in 0..32 {
                let emit = (lo + hi) * 0.5;
                if emit + f64::hypot(speed * (emit - center), slant) / c < receive {
                    lo = emit;
                } else {
                    hi = emit;
                }
            }
            let emit = (lo + hi) * 0.5;
            let envelope = (1. - ((emit - center) / 65.).powi(2)).max(0.);
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (random >> 32) as u32 as f64 / u32::MAX as f64 - 0.5;
            let stationary: f64 = [82., 100., 119., 139., 160.]
                .iter()
                .map(|hz| 0.15 * (std::f64::consts::TAU * hz * receive).sin())
                .sum();
            (stationary
                + 0.001 * noise
                + 0.025 * envelope * (std::f64::consts::TAU * tone * emit).sin()) as f32
        })
        .collect();
    let job = Job {
        fast_flyovers: false,
        ..Default::default()
    };
    let events = audio::detect(&samples, &job, &Control::default(), &|_, _| {}).unwrap();
    let received_cpa = center + slant / c;
    let event = events
        .iter()
        .find(|e| (e.cpa_s - received_cpa).abs() < 3.)
        .expect("A moving tone must survive five louder stationary tones, including crossings");
    assert!(
        (event.speed_mps / speed - 1.).abs() < 0.15,
        "speed {}",
        event.speed_mps
    );
    assert!(
        (event.slant_m / slant - 1.).abs() < 0.25,
        "slant {}",
        event.slant_m
    );
}

#[test]
fn a_brief_propeller_pass_survives_temporary_background_tones() {
    let (speed, slant, tone, center, c) = (60_f64, 900_f64, 160_f64, 58_f64, 340.39_f64);
    let rate = audio::SAMPLE_RATE;
    let mut random = 9013_u64;
    let samples: Vec<f32> = (0..160 * rate)
        .map(|i| {
            let receive = i as f64 / rate as f64;
            let (mut lo, mut hi) = (receive - 90., receive);
            // Solve propagation independently of the detector's Doppler model.
            for _ in 0..32 {
                let emit = (lo + hi) * 0.5;
                if emit + f64::hypot(speed * (emit - center), slant) / c < receive {
                    lo = emit;
                } else {
                    hi = emit;
                }
            }
            let emit = (lo + hi) * 0.5;
            let audible =
                ((receive - 38.) / 2.).clamp(0., 1.) * ((82. - receive) / 2.).clamp(0., 1.);
            let background: f64 = [92., 120., 150., 175., 210.]
                .iter()
                .map(|hz| 0.15 * (std::f64::consts::TAU * hz * receive).sin())
                .sum();
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (random >> 32) as u32 as f64 / u32::MAX as f64 - 0.5;
            (0.001 * noise
                + audible * (background + 0.025 * (std::f64::consts::TAU * tone * emit).sin()))
                as f32
        })
        .collect();
    let job = Job {
        fast_flyovers: false,
        ..Default::default()
    };
    let events = audio::detect(&samples, &job, &Control::default(), &|_, _| {}).unwrap();
    let received_cpa = center + slant / c;
    let event = events
        .iter()
        .find(|event| (event.cpa_s - received_cpa).abs() < 3.)
        .expect("A short propeller pass must survive temporary stationary background tones");
    assert!((event.speed_mps / speed - 1.).abs() < 0.15);
    assert!((event.slant_m / slant - 1.).abs() < 0.25);
}
