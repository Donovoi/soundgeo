//! Experimental location screening at the declared 1 km target.
//! These evidence thresholds are heuristics, not calibrated probabilities.
use crate::{Report, geo::Point, search::Candidate};
use serde::{Deserialize, Serialize};

pub const LOCATION_RADIUS_M: f64 = 1000.;
pub const COMPETITIVE_SUPPORT_FRACTION: f64 = 0.9;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FitSummary {
    pub latitude: f64,
    pub longitude: f64,
    pub recording_start_unix: f64,
    pub recording_clock_rate: f64,
    pub matches: usize,
    pub curve_support: f64,
    pub relative_error_percent: f64,
}

impl From<&Candidate> for FitSummary {
    fn from(value: &Candidate) -> Self {
        Self {
            latitude: value.latitude,
            longitude: value.longitude,
            recording_start_unix: value.recording_start_unix,
            recording_clock_rate: value.recording_clock_rate,
            matches: value.matches.len(),
            curve_support: value.curve_support(),
            relative_error_percent: value.relative_error_percent,
        }
    }
}

/// Summaries are captured before spatial deduplication and the display limit.
/// Only fits below 90% of the greatest support can be removed: subsequent
/// batches can only increase that maximum, making this pruning merge-safe.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Competition {
    pub complete: bool,
    pub fits: Vec<FitSummary>,
}

impl Competition {
    pub fn from_candidates(candidates: &[Candidate]) -> Self {
        let mut result = Self {
            complete: true,
            fits: candidates.iter().map(FitSummary::from).collect(),
        };
        result.prune();
        result
    }

    pub fn merge(&mut self, other: Self) {
        self.complete &= other.complete;
        self.fits.extend(other.fits);
        self.prune();
    }

    fn prune(&mut self) {
        let best = self.fits.iter().map(|f| f.curve_support).fold(0., f64::max);
        self.fits
            .retain(|f| f.curve_support >= best * COMPETITIVE_SUPPORT_FRACTION);
        self.fits.sort_by(|a, b| {
            b.curve_support
                .total_cmp(&a.curve_support)
                .then(a.latitude.total_cmp(&b.latitude))
                .then(a.longitude.total_cmp(&b.longitude))
                .then(a.recording_start_unix.total_cmp(&b.recording_start_unix))
                .then(a.recording_clock_rate.total_cmp(&b.recording_clock_rate))
                .then(a.matches.cmp(&b.matches))
                .then(
                    a.relative_error_percent
                        .total_cmp(&b.relative_error_percent),
                )
        });
        self.fits.dedup();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assessment {
    pub policy: String,
    pub location_radius_m: f64,
    pub competitive_support_fraction: f64,
    pub required_matches: usize,
    pub matched_events: usize,
    pub unmatched_event_indices: Vec<usize>,
    pub best_curve_support: f64,
    pub competitor_evidence_complete: bool,
    pub clock_ambiguous: bool,
    pub distant_competitors: Vec<FitSummary>,
    pub reasons: Vec<String>,
    pub passes: bool,
}

pub fn assess(report: &Report) -> Option<Assessment> {
    let first = report.candidates.first()?;
    // Preserve the two-sound unit-rate case. Larger recordings need at least
    // three sounds and a two-thirds consensus; a fitted rate needs four.
    let minimum = if (first.recording_clock_rate - 1.).abs() > 1e-12 {
        4
    } else {
        report.events.len().clamp(2, 3)
    };
    let required = minimum.max(report.events.len() - report.events.len() / 3);
    let mut matched = std::collections::BTreeSet::new();
    let mut aircraft = std::collections::BTreeSet::new();
    let mut valid_indices = true;
    for m in &first.matches {
        valid_indices &= m
            .event_index
            .is_some_and(|i| i < report.events.len() && matched.insert(i));
        valid_indices &= aircraft.insert(&m.aircraft);
    }
    let support = first.curve_support();
    let point = Point {
        lat: first.latitude,
        lon: first.longitude,
        alt: 0.,
    };
    // Include displayed fits too, so an incomplete older report still exposes
    // its known competitors. Missing pre-limit evidence prevents acceptance.
    let mut evidence = report.doppler_competition.clone();
    evidence.merge(Competition::from_candidates(&report.candidates));
    let clock_ambiguous = evidence.fits.iter().any(|other| {
        other.curve_support >= support * COMPETITIVE_SUPPORT_FRACTION
            && ((other.recording_start_unix - first.recording_start_unix).abs() >= 10.
                || (other.recording_start_unix - first.recording_start_unix
                    + report.duration_s
                        * (other.recording_clock_rate - first.recording_clock_rate))
                    .abs()
                    >= 10.)
    });
    let distant: Vec<_> = evidence
        .fits
        .into_iter()
        .filter(|other| {
            other.curve_support >= support * COMPETITIVE_SUPPORT_FRACTION
                && point.distance(Point {
                    lat: other.latitude,
                    lon: other.longitude,
                    alt: 0.,
                }) > LOCATION_RADIUS_M
        })
        .collect();
    let mut reasons = vec![];
    if !valid_indices {
        reasons.push("invalid_or_reused_match_indices".into());
    }
    if matched.len() < required {
        reasons.push("insufficient_consensus".into());
    }
    if first.clock_fit_is_provisional() {
        reasons.push("provisional_clock_rate".into());
    }
    if support <= 0. || !support.is_finite() {
        reasons.push("missing_curve_support".into());
    }
    if !report.doppler_competition.complete {
        reasons.push("incomplete_competitor_evidence".into());
    }
    if !distant.is_empty() {
        reasons.push("competitive_location_beyond_1km".into());
    }
    Some(Assessment {
        policy: "doppler-location-1km-v1".into(),
        location_radius_m: LOCATION_RADIUS_M,
        competitive_support_fraction: COMPETITIVE_SUPPORT_FRACTION,
        required_matches: required,
        matched_events: matched.len(),
        unmatched_event_indices: (0..report.events.len())
            .filter(|i| !matched.contains(i))
            .collect(),
        best_curve_support: support,
        competitor_evidence_complete: report.doppler_competition.complete,
        clock_ambiguous,
        distant_competitors: distant,
        passes: reasons.is_empty(),
        reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Match;

    fn report(detections: usize, matches: usize) -> Report {
        let event = serde_json::json!({"start_s":0.,"end_s":10.,"cpa_s":5.,
            "base_hz":1000.,"speed_mps":70.,"slant_m":1000.,"fit_rmse_hz":2.,
            "contrast_db":20.,"ridge":[]});
        let candidate = serde_json::json!({"latitude":0.,"longitude":0.,"ground_m":0.,
            "recording_start_utc":"1970-01-01T00:00:00Z","recording_start_unix":0.,
            "relative_error_percent":0.2,"matches":(0..matches).map(|i| Match {
                event_index:Some(i), event_variant:0, aircraft:format!("flight-{i}"),
                rmse_hz:2.,fitted_tone_hz:1000.,coverage:1.,linear_baseline_rmse_hz:Some(10.),modeled_hz:vec![]
            }).collect::<Vec<_>>()});
        let mut report: Report = serde_json::from_value(serde_json::json!({
            "version":"test","status":"ambiguous","media":"unused.wav","job":crate::Job::default(),
            "duration_s":1800.,"track_count":matches,"events":vec![event;detections],
            "candidates":[candidate],"warnings":[],"search_grid_m":200.,"created_utc":"test"
        }))
        .unwrap();
        report.doppler_competition = Competition::from_candidates(&report.candidates);
        report
    }

    fn at_distance(mut candidate: Candidate, meters: f64) -> Candidate {
        candidate.longitude = (2. * (meters / (2. * 6_378_137.)).asin()).to_degrees();
        candidate
    }

    #[test]
    fn nearby_location_with_different_clock_is_within_target() {
        let mut r = report(10, 10);
        let mut other = at_distance(r.candidates[0].clone(), 999.999);
        other.recording_start_unix += 86400.;
        r.doppler_competition
            .merge(Competition::from_candidates(&[other]));
        let assessment = assess(&r).unwrap();
        assert!(assessment.passes && assessment.clock_ambiguous);
    }

    #[test]
    fn competitive_fit_just_outside_target_blocks_location() {
        let mut r = report(10, 10);
        let other = at_distance(r.candidates[0].clone(), 1000.001);
        r.doppler_competition
            .merge(Competition::from_candidates(&[other]));
        let result = assess(&r).unwrap();
        assert_eq!(result.reasons, ["competitive_location_beyond_1km"]);
    }

    #[test]
    fn weaker_distant_low_residual_fit_does_not_override_corroboration() {
        let mut r = report(10, 10);
        let mut other = at_distance(r.candidates[0].clone(), 2000.);
        other.matches.truncate(8);
        other.relative_error_percent = 0.01;
        r.doppler_competition
            .merge(Competition::from_candidates(&[other]));
        assert!(assess(&r).unwrap().passes);
    }

    #[test]
    fn support_fraction_boundary_is_inclusive() {
        let mut r = report(10, 10);
        let mut other = FitSummary::from(&at_distance(r.candidates[0].clone(), 2000.));
        other.curve_support *= COMPETITIVE_SUPPORT_FRACTION;
        r.doppler_competition.fits.push(other.clone());
        assert!(!assess(&r).unwrap().passes);
        r.doppler_competition.fits.last_mut().unwrap().curve_support -= 1e-8;
        assert!(assess(&r).unwrap().passes);
    }

    #[test]
    fn partial_consensus_retains_unmatched_indices_and_requires_two_thirds() {
        let result = assess(&report(24, 21)).unwrap();
        assert!(result.passes);
        assert_eq!(result.required_matches, 16);
        assert_eq!(result.unmatched_event_indices, [21, 22, 23]);
        assert!(assess(&report(24, 16)).unwrap().passes);
        assert!(!assess(&report(24, 15)).unwrap().passes);
        assert!(!assess(&report(5, 3)).unwrap().passes);
        assert!(assess(&report(2, 2)).unwrap().passes);
        assert!(!assess(&report(1, 1)).unwrap().passes);
    }

    #[test]
    fn fitted_clock_still_requires_four_sounds() {
        let mut r = report(3, 3);
        r.candidates[0].recording_clock_rate = 1.0001;
        assert!(
            assess(&r)
                .unwrap()
                .reasons
                .contains(&"provisional_clock_rate".into())
        );
        let mut r = report(4, 4);
        r.candidates[0].recording_clock_rate = 1.0001;
        assert!(assess(&r).unwrap().passes);
    }

    #[test]
    fn old_reports_require_pre_limit_evidence_and_measured_curvature() {
        let mut r = report(6, 6);
        r.doppler_competition = Competition::default();
        assert!(
            assess(&r)
                .unwrap()
                .reasons
                .contains(&"incomplete_competitor_evidence".into())
        );
        r.doppler_competition.complete = true;
        for m in &mut r.candidates[0].matches {
            m.linear_baseline_rmse_hz = None;
        }
        assert!(
            assess(&r)
                .unwrap()
                .reasons
                .contains(&"missing_curve_support".into())
        );
    }

    #[test]
    fn duplicated_sounds_or_flights_cannot_supply_consensus() {
        let mut r = report(4, 4);
        r.candidates[0].matches[3].event_index = Some(2);
        assert!(!assess(&r).unwrap().passes);
        let mut r = report(4, 4);
        r.candidates[0].matches[3].aircraft = "flight-2".into();
        assert!(!assess(&r).unwrap().passes);
    }

    #[test]
    fn competitor_beyond_display_limit_survives_batch_and_date_merges() {
        let mut r = report(10, 10);
        let mut all = vec![];
        for i in 0..12 {
            let mut near = r.candidates[0].clone();
            near.recording_start_unix = 20. * i as f64;
            all.push(near);
        }
        let mut distant = at_distance(r.candidates[0].clone(), 2000.);
        distant.recording_start_unix = 1000.;
        all.push(distant);
        let mut first_batch = Competition::from_candidates(&all[..6]);
        first_batch.merge(Competition::from_candidates(&all[6..]));
        let mut dates = Competition::from_candidates(&[]);
        dates.merge(first_batch);
        dates.merge(Competition::from_candidates(&[]));
        r.doppler_competition = dates;
        r.candidates = crate::search::rank_candidates(all, r.duration_s);
        assert_eq!(r.candidates.len(), 10);
        assert!(r.candidates.iter().all(|c| c.longitude == 0.));
        assert!(!assess(&r).unwrap().passes);
        r.doppler_competition.merge(Competition::default());
        assert!(!r.doppler_competition.complete);
    }

    #[test]
    fn pruning_across_merges_matches_unpartitioned_competition() {
        let r = report(12, 12);
        let mut values = vec![];
        for i in 3..=12 {
            let mut c = at_distance(r.candidates[0].clone(), i as f64 * 400.);
            c.matches.truncate(i);
            values.push(c);
        }
        let direct = Competition::from_candidates(&values);
        let mut merged = Competition::from_candidates(&[]);
        for c in values {
            merged.merge(Competition::from_candidates(&[c]));
        }
        assert_eq!(
            serde_json::to_value(direct).unwrap(),
            serde_json::to_value(merged).unwrap()
        );
    }
}
