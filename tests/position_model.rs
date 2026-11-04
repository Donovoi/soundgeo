use soundgeo::{
    geo::{ElevationDatum, Point, Terrain, geoid_undulation},
    model::{Sample, Track},
};

#[test]
fn analytic_propagation_matches_independent_bisection_for_asymmetric_segments() {
    let observer = Point {
        lat: 40.,
        lon: -75.,
        alt: 100.,
    }
    .ecef();
    for (east, north, height) in [(80., 0., 150.), (-130., 40., 800.), (200., -90., 3000.)] {
        let start = Point {
            lat: 40.001,
            lon: -75.004,
            alt: height,
        };
        let end = Point {
            lat: start.lat + north * 30. / 111320.,
            lon: start.lon + east * 30. / (111320. * 40_f64.to_radians().cos()),
            alt: height + 90.,
        };
        let track = Track {
            id: "analytic".into(),
            kind: Default::default(),
            samples: vec![Sample::new(1000., start), Sample::new(1030., end)],
        };
        let velocity = (end.ecef() - start.ecef()) * (1. / 30.);
        for step in 1..60 {
            let emit = step as f64 / 2.;
            let arrival = 1000. + emit + (start.ecef() + velocity * emit - observer).norm() / 340.;
            let mut low = 0.;
            let mut high = 30.;
            for _ in 0..60 {
                let middle = (low + high) / 2.;
                let received =
                    1000. + middle + (start.ecef() + velocity * middle - observer).norm() / 340.;
                if received < arrival {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let r = start.ecef() + velocity * ((low + high) / 2.) - observer;
            let expected = 340. / (340. + velocity.dot(r) / r.norm());
            assert!((track.factor(observer, arrival, 340.).unwrap() - expected).abs() < 1e-9);
        }
    }
}

#[test]
fn geoid_conversion_matches_independent_proj_values_and_wraps_longitude() {
    // Reference values from PROJ's vgridshift (+multiplier=1), not this parser.
    for (lat, lon, expected) in [
        (0., 0., 17.161_579_132_080_078),
        (
            41.778_442_502_260_28,
            -87.742_778_104_052_09,
            -33.807_815_677_925_2,
        ),
        (50.89677, -1.40477, 46.731_835_694_341_235),
        (-33.86, 151.21, 22.455_431_790_161_086),
        (80., 179.999, 4.409_899_497_985_843),
        (80., -180.001, 4.409_899_497_985_843),
    ] {
        assert!((geoid_undulation(lat, lon) - expected).abs() < 1e-6);
    }
    for lat in [-90., 90.] {
        assert!((geoid_undulation(lat, -180.) - geoid_undulation(lat, 180.)).abs() < 1e-6);
    }
    let point = Point {
        lat: 0.,
        lon: 0.,
        alt: 0.,
    };
    assert!(
        (Terrain::with_datum(None, 100., ElevationDatum::Egm96).elevation(point)
            - 117.161_579_132_080_08)
            .abs()
            < 1e-6
    );
    assert_eq!(Terrain::new(None, 100.).elevation(point), 100.);
}

#[test]
fn local_fit_reduces_velocity_noise_without_extending_or_joining_tracks() {
    let observer = Point {
        lat: 0.,
        lon: 0.,
        alt: 0.,
    };
    let truth = Track {
        id: "truth".into(),
        kind: Default::default(),
        samples: (-20..=20)
            .map(|i| {
                Sample::new(
                    1000. + 2. * i as f64,
                    Point {
                        lat: 0.,
                        lon: i as f64 * 160. / 111320.,
                        alt: 150.,
                    },
                )
            })
            .collect(),
    };
    let mut noisy = truth.clone();
    for (i, s) in noisy.samples.iter_mut().enumerate() {
        let sign = if i % 2 == 0 { 1. } else { -1. };
        *s = Sample::new(
            s.time,
            Point {
                lon: s.point.lon + sign * 5. / 111320.,
                alt: s.point.alt + sign * 3.81,
                ..s.point
            },
        );
    }
    let raw = noisy.clone();
    noisy.smooth_positions();
    let error = |track: &Track| -> f64 {
        (980..1020)
            .map(|t| {
                let f = track.factor(observer.ecef(), t as f64 + 0.3, 340.).unwrap();
                let expected = truth.factor(observer.ecef(), t as f64 + 0.3, 340.).unwrap();
                (f - expected).powi(2)
            })
            .sum()
    };
    assert!(error(&noisy) < error(&raw) * 0.3);
    assert_eq!(noisy.samples.len(), raw.samples.len());
    for (before, after) in raw.samples.iter().zip(&noisy.samples) {
        assert_eq!(before.time, after.time);
    }
    assert_eq!(noisy.samples[0].point.alt, raw.samples[0].point.alt);
    assert_eq!(
        noisy.samples.last().unwrap().point.alt,
        raw.samples.last().unwrap().point.alt
    );
    assert!(noisy.factor(observer.ecef(), 950., 340.).is_none());
    let mut gaps = truth.clone();
    gaps.samples.retain(|s| s.time < 970. || s.time > 1030.);
    gaps.smooth_positions();
    assert!(gaps.factor(observer.ecef(), 1000., 340.).is_none());
}
