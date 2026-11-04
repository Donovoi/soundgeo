use soundgeo::{Control, Job, audio};

fn signal(seconds: usize, passes: &[(f64, f64, f64, f64)]) -> Vec<f32> {
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
                value += envelope * slant / distance * (std::f64::consts::TAU * tone * emit).sin();
            }
            value as f32
        })
        .collect()
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
