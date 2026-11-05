//! A constant-rate acoustic background plus Gaussian flight-passage intensity.
//! These likelihood scores are not calibrated location probabilities.

#[derive(Clone, Copy, Debug)]
pub struct Fit {
    pub score: f64,
    pub background_events: f64,
    pub detection_probability: f64,
}

/// Fit two nonnegative intensity components without a general-purpose optimizer.
/// `density` is the summed passage density at each observed acoustic peak.
/// `exposure` is its integral over the recording, including boundary truncation.
pub fn fit(density: &[f64], exposure: f64, duration: f64) -> Fit {
    let n = density.len() as f64;
    let null = Fit {
        score: 0.,
        background_events: n,
        detection_probability: 0.,
    };
    if n == 0.
        || exposure <= 0.
        || duration <= 0.
        || !exposure.is_finite()
        || !duration.is_finite()
        || density.iter().any(|d| !d.is_finite() || *d < 0.)
    {
        return null;
    }
    // Multiplying the two interior score equations by their parameters and
    // adding gives background + p*exposure = number of observed events.
    let a: Vec<_> = density
        .iter()
        .map(|d| (duration * d - exposure) / n)
        .collect();
    if a.iter().sum::<f64>() <= 0. {
        return null;
    }
    let derivative = |p: f64| {
        let mut sum = 0.;
        for &a in &a {
            let den = 1. + p * a;
            if den <= 0. {
                return f64::NEG_INFINITY;
            }
            sum += a / den;
        }
        sum
    };
    let upper = (n / exposure).min(1.);
    let p = if derivative(upper) >= 0. {
        upper
    } else {
        let (mut low, mut high) = (0., upper);
        for _ in 0..50 {
            let p = (low + high) * 0.5;
            if derivative(p) > 0. {
                low = p;
            } else {
                high = p;
            }
        }
        (low + high) * 0.5
    };
    let mut background = (n - p * exposure).max(0.);
    if p == 1. {
        // At the probability boundary, optimize background on its own.
        let (mut low, mut high) = (0., n);
        for _ in 0..50 {
            background = (low + high) * 0.5;
            let gradient = 1.
                - density
                    .iter()
                    .map(|d| 1. / (background + duration * d))
                    .sum::<f64>();
            if gradient < 0. {
                low = background;
            } else {
                high = background;
            }
        }
        background = (low + high) * 0.5;
    }
    let score = density
        .iter()
        .map(|d| (background / n + p * d * duration / n).ln())
        .sum::<f64>()
        + n
        - background
        - p * exposure;
    Fit {
        score: score.max(0.),
        background_events: background,
        detection_probability: p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_independent_numerical_optimizer() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/timing-likelihood.json"))
                .unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            let density: Vec<_> = case["density"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let fitted = fit(
                &density,
                case["exposure"].as_f64().unwrap(),
                case["duration"].as_f64().unwrap(),
            );
            assert!((fitted.score - case["score"].as_f64().unwrap()).abs() < 1e-7);
            assert!((0. ..=1.).contains(&fitted.detection_probability));
            assert!(fitted.background_events >= 0.);
        }
    }
    #[test]
    fn absent_or_uniform_evidence_does_not_create_a_timing_signal() {
        assert_eq!(fit(&[], 20., 3600.).score, 0.);
        assert_eq!(fit(&[0.; 20], 20., 3600.).score, 0.);
        assert!(fit(&[20. / 3600.; 20], 20., 3600.).score < 1e-10);
        assert_eq!(fit(&[f64::NAN], 20., 3600.).score, 0.);
    }
}
