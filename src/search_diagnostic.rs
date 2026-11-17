//! Conditional pair diagnostics using the production fitter at an inferred fit.
use super::*;

#[derive(Serialize)]
pub struct PairDiagnostic {
    pub event_index: usize,
    pub event_variant: usize,
    pub aircraft: String,
    pub coverage: f64,
    pub fit: Option<Match>,
    pub rejection_reasons: Vec<String>,
    pub aggregate_error_with_existing_percent: Option<f64>,
    pub already_assigned_to: Vec<usize>,
    /// This is measured at the final fit, not at the original proposal point.
    pub coarse_feature_error_at_final_point: Option<f64>,
    pub coarse_gates_pass_at_final_point: bool,
}

pub fn pair(
    candidate: &Candidate,
    event: &Event,
    track: &Track,
    sound_speed: f64,
    event_index: usize,
    event_variant: usize,
) -> PairDiagnostic {
    let seed = Seed {
        point: Point {
            lat: candidate.latitude,
            lon: candidate.longitude,
            alt: candidate.ground_m,
        },
        start: candidate.recording_start_unix,
        clock_rate: candidate.recording_clock_rate,
        anchor: 0,
        indices: vec![],
        alternatives: Arc::new(vec![]),
        score: 0.,
        coarse_profiled: false,
        corroboration: None,
    };
    let (coverage, fit) =
        fit_observation_details(&seed, event, track, sound_speed, event_index, event_variant);
    let already_assigned_to: Vec<_> = candidate
        .matches
        .iter()
        .filter(|m| m.aircraft == track.id && m.event_index != Some(event_index))
        .filter_map(|m| m.event_index)
        .collect();
    let mut reasons = vec![];
    let mut aggregate = None;
    if let Some(m) = &fit {
        if !has_curvature(m) {
            reasons.push("insufficient_improvement_over_linear_drift".into());
        }
        if m.rmse_hz / m.fitted_tone_hz > 0.015 {
            reasons.push("relative_rmse_above_1_5_percent".into());
        }
        let mut combined: Vec<_> = candidate
            .matches
            .iter()
            .filter(|m| m.event_index != Some(event_index))
            .cloned()
            .collect();
        combined.push(m.clone());
        let error = match_error(&combined);
        aggregate = Some(error);
        if error > 1. {
            reasons.push("joint_error_above_1_percent".into());
        }
    } else {
        reasons.push(
            if coverage < 0.9 {
                "reference_coverage_below_90_percent"
            } else {
                "invalid_doppler_factors"
            }
            .into(),
        );
    }
    if !already_assigned_to.is_empty() {
        reasons.push("flight_already_assigned".into());
    }
    // Match the coarse proposal's unit-rate clock convention. Refinement may
    // subsequently change the rate; this does not reconstruct its search path.
    let wanted = seed.start + event.cpa_s;
    let cpa = track.cpa_near(
        seed.point.ecef(),
        sound_speed,
        Some((wanted - 45., wanted + 45.)),
    );
    let feature = cpa.map(|(time, range, speed)| {
        feature_error(event, range, speed) + ((time - wanted) / 10.).powi(2)
    });
    let coarse_pass = cpa.is_some_and(|(_, range, speed)| range <= 40000. && speed >= 25.)
        && feature.is_some_and(|e| e <= FEATURE_ERROR_LIMIT);
    PairDiagnostic {
        event_index,
        event_variant,
        aircraft: track.id.clone(),
        coverage,
        fit,
        rejection_reasons: reasons,
        aggregate_error_with_existing_percent: aggregate,
        already_assigned_to,
        coarse_feature_error_at_final_point: feature,
        coarse_gates_pass_at_final_point: coarse_pass,
    }
}
