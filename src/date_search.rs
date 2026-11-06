use crate::{Control, Job, Progress, Report, geo::Point, timing};
use anyhow::{Result, ensure};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coverage {
    pub dates: Vec<String>,
    pub completed_batches: usize,
    pub complete: bool,
    pub track_count_includes_overlap: bool,
}

fn batches(dates: &[String]) -> Result<Vec<(String, String)>> {
    let parsed: Vec<_> = dates
        .iter()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .collect::<std::result::Result<_, _>>()?;
    let mut result = Vec::new();
    for i in 0..dates.len() {
        if i + 1 < dates.len() && (parsed[i + 1] - parsed[i]).num_days() == 1 {
            result.push((dates[i].clone(), dates[i + 1].clone()));
        } else if i == 0 || (parsed[i] - parsed[i - 1]).num_days() != 1 {
            result.push((dates[i].clone(), dates[i].clone()));
        }
    }
    Ok(result)
}

pub(crate) fn run(
    job: &Job,
    template: Report,
    peaks: Option<Vec<timing::Peak>>,
    dates: &[String],
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Report> {
    let windows = batches(dates)?;
    let mut combined = template.clone();
    for (index, (from, to)) in windows.iter().enumerate() {
        control.check()?;
        let mut batch = job.clone();
        let archive = batch.archive.as_mut().unwrap();
        archive.date_from = from.clone();
        archive.date_to = Some(to.clone());
        let update = |message: &str, fraction: f32| {
            progress(
                &format!(
                    "Dates {from} to {to} ({}/{}): {message}",
                    index + 1,
                    windows.len()
                ),
                0.34 + 0.66 * (index as f32 + fraction) / windows.len() as f32,
            );
        };
        let paths = archive.ensure_for_bounds(job.bounds, control, &|m, p| update(m, p * 0.34))?;
        let mut seed = template.clone();
        seed.job = batch.clone();
        let result =
            crate::analyze_sources(&batch, seed, peaks.clone(), &paths, true, control, &update)?;
        combined.track_count += result.track_count;
        combined.hypotheses += result.hypotheses;
        combined.search_grid_m = combined.search_grid_m.max(result.search_grid_m);
        combined.search_method = result.search_method;
        combined.reference_sources.extend(result.reference_sources);
        combined.terrain_sources.extend(result.terrain_sources);
        let mut warnings = result.warnings;
        if let Some(assessment) = &result.timing {
            warnings.retain(|w| *w != assessment.explanation);
        }
        combined.warnings.extend(warnings);
        combined.candidates.extend(result.candidates);
        rank_doppler(&mut combined);
        if let Some(assessment) = result.timing {
            merge_timing(&mut combined.timing, assessment)?;
        }
    }
    if combined.timing.is_none()
        && let Some(peaks) = peaks
    {
        combined.timing = Some(timing::Assessment::pending(peaks));
    }
    if combined.search_method.is_empty() {
        combined.search_method = "date_batches".into();
    }
    if let Some(assessment) = &mut combined.timing {
        timing::apply_screen(assessment);
        combined.status = match assessment.screen {
            timing::Screen::InsufficientEvidence => "insufficient_evidence",
            timing::Screen::Ambiguous => "ambiguous",
            timing::Screen::TimingCandidate => "candidates_found",
        }
        .into();
        combined.warnings.push(assessment.explanation.clone());
    } else {
        crate::set_doppler_status(&mut combined);
    }
    combined.reference_sources.sort();
    combined.reference_sources.dedup();
    combined.terrain_sources.sort_by(|a, b| a.tile.cmp(&b.tile));
    combined.terrain_sources.dedup_by(|a, b| a.tile == b.tile);
    combined.warnings = combined
        .warnings
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    combined.warnings.push("Date search completed all overlapping daily batches. Track counts include overlap. Results were compared across all searched batches.".into());
    combined.date_search = Some(Coverage {
        dates: dates.to_vec(),
        completed_batches: windows.len(),
        complete: true,
        track_count_includes_overlap: true,
    });
    progress("Date search complete", 1.);
    Ok(combined)
}

fn rank_doppler(report: &mut Report) {
    report.candidates.sort_by(|a, b| {
        b.matches.len().cmp(&a.matches.len()).then_with(|| {
            (a.relative_error_percent + 0.25 * a.silence_conflicts.len() as f64)
                .total_cmp(&(b.relative_error_percent + 0.25 * b.silence_conflicts.len() as f64))
        })
    });
    let mut distinct: Vec<crate::search::Candidate> = Vec::new();
    for candidate in report.candidates.drain(..) {
        let point = Point {
            lat: candidate.latitude,
            lon: candidate.longitude,
            alt: 0.,
        };
        if distinct.iter().any(|other| {
            point.distance(Point {
                lat: other.latitude,
                lon: other.longitude,
                alt: 0.,
            }) < 250.
                && (candidate.recording_start_unix - other.recording_start_unix).abs() < 10.
                && (candidate.recording_start_unix - other.recording_start_unix
                    + report.duration_s
                        * (candidate.recording_clock_rate - other.recording_clock_rate))
                    .abs()
                    < 10.
        }) {
            continue;
        }
        distinct.push(candidate);
        if distinct.len() == 10 {
            break;
        }
    }
    report.candidates = distinct;
}

fn merge_timing(
    target: &mut Option<timing::Assessment>,
    mut next: timing::Assessment,
) -> Result<()> {
    let Some(current) = target else {
        *target = Some(next);
        return Ok(());
    };
    ensure!(
        current.control_permutation_seeds == next.control_permutation_seeds,
        "Date batches used different shuffled controls"
    );
    ensure!(
        current.control_maximum_scores.len() == next.control_maximum_scores.len(),
        "Date batch controls are incomplete"
    );
    for (score, other) in current
        .control_maximum_scores
        .iter_mut()
        .zip(next.control_maximum_scores)
    {
        *score = score.max(other);
    }
    current.control_likelihood_evaluations += next.control_likelihood_evaluations;
    if let Some(mut incoming) = next.search.take() {
        if let Some(search) = &mut current.search {
            search.likelihood_evaluations += incoming.likelihood_evaluations;
            search.terrain_fallback_points += incoming.terrain_fallback_points;
            search.eligible_points = search.eligible_points.max(incoming.eligible_points);
            search.candidates.append(&mut incoming.candidates);
            search
                .candidates
                .sort_by(|a, b| b.log_likelihood_ratio.total_cmp(&a.log_likelihood_ratio));
            search.candidates.dedup_by(|a, b| {
                a.latitude == b.latitude
                    && a.longitude == b.longitude
                    && (a.offset_utc - b.offset_utc).abs() < 0.001
            });
            search
                .candidates
                .truncate(search.options.retained_candidates);
            let mut scores = BTreeMap::new();
            for point in search
                .location_scores
                .drain(..)
                .chain(incoming.location_scores)
            {
                let key = (point.latitude.to_bits(), point.longitude.to_bits());
                scores
                    .entry(key)
                    .and_modify(|old: &mut timing::LocationScore| {
                        old.log_likelihood_ratio =
                            old.log_likelihood_ratio.max(point.log_likelihood_ratio);
                    })
                    .or_insert(point);
            }
            search.location_scores = scores.into_values().collect();
        } else {
            current.search = Some(incoming);
        }
    }
    current.region = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assessment(score: f64, lon: f64, null: f64) -> timing::Assessment {
        let mut value = timing::Assessment::pending(vec![]);
        value.control_permutation_seeds = (0..19).collect();
        value.control_maximum_scores = vec![null; 19];
        value.search = Some(timing::SearchResult {
            options: Default::default(),
            points: 1,
            eligible_points: 1,
            likelihood_evaluations: 1,
            terrain_fallback_points: 0,
            location_scores: vec![timing::LocationScore {
                latitude: 41.,
                longitude: lon,
                log_likelihood_ratio: score,
            }],
            candidates: vec![timing::Candidate {
                latitude: 41.,
                longitude: lon,
                elevation_m: 0.,
                offset_utc: 1000.,
                log_likelihood_ratio: score,
                reference_passages: 6,
                reference_exposure: 6.,
                background_expected_events: 1.,
                detection_probability: 0.5,
                pairs: (0..6)
                    .map(|peak| timing::Pair {
                        peak,
                        track: format!("flight-{peak}"),
                        residual_s: 0.,
                    })
                    .collect(),
            }],
        });
        timing::apply_screen(&mut value);
        value
    }
    #[test]
    fn another_dates_control_can_reject_a_daily_match() {
        let first = assessment(40., -87., 10.);
        assert_eq!(first.screen, timing::Screen::TimingCandidate);
        let mut combined = Some(first);
        merge_timing(&mut combined, assessment(35., -87., 41.)).unwrap();
        let combined = combined.as_mut().unwrap();
        timing::apply_screen(combined);
        assert_eq!(combined.screen, timing::Screen::InsufficientEvidence);
        assert!(combined.region.is_none());
        assert_eq!(combined.control_maximum_scores, vec![41.; 19]);
    }
    #[test]
    fn another_dates_spatial_alternative_prevents_a_single_location() {
        let mut combined = Some(assessment(40., -87., 10.));
        merge_timing(&mut combined, assessment(39., -86., 10.)).unwrap();
        let combined = combined.as_mut().unwrap();
        timing::apply_screen(combined);
        assert_eq!(combined.screen, timing::Screen::Ambiguous);
        assert!(combined.region.is_none());
        assert_eq!(combined.search.as_ref().unwrap().location_scores.len(), 2);
    }
    #[test]
    fn windows_cover_midnight_and_preserve_cache_gaps() {
        let dates = ["2024-01-01", "2024-01-02", "2024-01-03", "2024-03-01"].map(String::from);
        assert_eq!(
            batches(&dates).unwrap(),
            vec![
                ("2024-01-01".into(), "2024-01-02".into()),
                ("2024-01-02".into(), "2024-01-03".into()),
                ("2024-03-01".into(), "2024-03-01".into())
            ]
        );
    }
}
