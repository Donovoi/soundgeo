use crate::{
    Control, Job, Progress,
    audio::Event,
    geo::{Bounds, Point, Terrain},
    model::Track,
};
use anyhow::{Result, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Auto,
    RegionalGrid,
    FlightCorridors,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilenceMode {
    Off,
    #[default]
    SupportingEvidence,
    Strict,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SilenceConflict {
    pub aircraft: String,
    pub time_in_recording_s: f64,
    pub slant_m: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Match {
    /// Original detection index. None keeps older reports readable in order.
    #[serde(default)]
    pub event_index: Option<usize>,
    pub aircraft: String,
    pub rmse_hz: f64,
    pub fitted_tone_hz: f64,
    pub coverage: f64,
    /// Freely fitted linear drift on exactly the covered observations.
    #[serde(default)]
    pub linear_baseline_rmse_hz: Option<f64>,
    pub modeled_hz: Vec<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub latitude: f64,
    pub longitude: f64,
    pub ground_m: f64,
    pub recording_start_utc: String,
    pub recording_start_unix: f64,
    /// UTC elapsed seconds per media second; a single rate for every flyover.
    #[serde(default = "unit_clock_rate")]
    pub recording_clock_rate: f64,
    pub relative_error_percent: f64,
    pub matches: Vec<Match>,
    #[serde(default)]
    pub silence_conflicts: Vec<SilenceConflict>,
}
pub struct SearchResult {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
    pub coarse_grid_m: f64,
    pub hypotheses: usize,
    pub method: String,
}
#[derive(Clone)]
pub(crate) struct Seed {
    pub point: Point,
    pub start: f64,
    pub clock_rate: f64,
    pub indices: Vec<usize>,
    pub score: f64,
}

fn unit_clock_rate() -> f64 {
    1.
}

/// An unmatched detection is retained as an outlier, never counted as evidence.
/// Three or more detections require at least three independent flight matches.
pub(crate) fn complete_seed(
    point: Point,
    start: f64,
    anchor: usize,
    track: usize,
    initial_error: f64,
    events: &[Event],
    mut nearby: impl FnMut(f64, &Event) -> Vec<(usize, f64, f64, f64)>,
) -> Option<Seed> {
    let mut indices = vec![usize::MAX; events.len()];
    indices[anchor] = track;
    let mut errors = vec![initial_error];
    let mut matched = 1;
    for (i, event) in events.iter().enumerate().filter(|(i, _)| *i != anchor) {
        let wanted = start + event.cpa_s;
        let best = nearby(wanted, event)
            .into_iter()
            .filter(|(j, time, range, speed)| {
                !indices.contains(j)
                    && *speed >= 25.
                    && *range <= 40000.
                    && (time - wanted).abs() <= 45.
            })
            .map(|(j, time, range, speed)| {
                (
                    j,
                    feature_error(event, range, speed) + ((time - wanted) / 10.).powi(2),
                )
            })
            .filter(|(_, error)| *error <= 12.)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((j, error)) = best {
            indices[i] = j;
            errors.push(error);
            matched += 1;
        }
    }
    if matched < events.len().min(3) {
        return None;
    }
    Some(Seed {
        point,
        start,
        clock_rate: 1.,
        indices,
        // Coarse selection must use the same consensus size as exact fitting.
        // Rewarding every loose association first lets unrelated sounds crowd
        // out three precise matches before they ever reach refinement.
        score: {
            errors.sort_by(f64::total_cmp);
            let required = events.len().min(3);
            errors[..required].iter().sum::<f64>() / required as f64
        },
    })
}

fn inferred_bounds(tracks: &[Track]) -> Result<Bounds> {
    let mut b = Bounds {
        south: 85.,
        north: -85.,
        west: 180.,
        east: -180.,
    };
    for t in tracks {
        for s in &t.samples {
            b.south = b.south.min(s.point.lat);
            b.north = b.north.max(s.point.lat);
            b.west = b.west.min(s.point.lon);
            b.east = b.east.max(s.point.lon);
        }
    }
    b.south = (b.south - 0.6).max(-85.);
    b.north = (b.north + 0.6).min(85.);
    let lon_margin = 0.6 / ((b.south + b.north) * 0.5).to_radians().cos().max(0.05);
    b.west = (b.west - lon_margin).max(-180.);
    b.east = (b.east + lon_margin).min(180.);
    b.validate()?;
    Ok(b)
}
pub(crate) fn feature_error(e: &Event, range: f64, speed: f64) -> f64 {
    ((range / e.slant_m.max(100.)).ln() / 0.7).powi(2) + ((speed / e.speed_mps).ln() / 0.5).powi(2)
}
pub(crate) fn within_time(job: &Job, start: f64) -> bool {
    job.start_utc
        .is_none_or(|t| (start - t).abs() <= job.time_tolerance_s)
}

pub fn locate(
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    terrain: &mut Terrain,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<SearchResult> {
    if events.is_empty() {
        bail!("No flyover events to match");
    }
    let bounds = job.bounds.unwrap_or(inferred_bounds(tracks)?);
    let (mut grid, grid_spacing) = bounds.grid(job.grid_m, job.max_grid_points);
    let indexed = job.search_mode == SearchMode::FlightCorridors
        || (job.search_mode == SearchMode::Auto && grid_spacing > job.grid_m * 1.1);
    let spacing = if indexed { job.grid_m } else { grid_spacing };
    for p in grid.iter_mut().filter(|_| !indexed) {
        control.check()?;
        p.alt = terrain.elevation(*p);
    }
    let c = job.sound_speed();
    let anchors = crate::audio::anchor_indices(events);
    let (mut seeds, hypotheses) = if indexed {
        crate::index::corridor_seeds(job, events, tracks, bounds, terrain, control, progress)?
    } else {
        (vec![], grid.len())
    };
    for (batch_index, batch) in grid.chunks(64).enumerate() {
        if indexed {
            break;
        }
        control.check()?;
        progress(
            &format!(
                "Searching {} grid points across {} flight segments",
                grid.len(),
                tracks.len()
            ),
            0.42 + 0.30 * (batch_index * 64) as f32 / grid.len() as f32,
        );
        let mut batch_seeds: Vec<Seed> = batch
            .par_iter()
            .flat_map_iter(|&point| {
                let cpas: Vec<_> = tracks.iter().map(|t| t.cpa(point.ecef(), c)).collect();
                let mut here = vec![];
                for &anchor in &anchors {
                    let first = &events[anchor];
                    for (i, cpa) in cpas.iter().enumerate() {
                        let Some((arrival, range, speed)) = *cpa else {
                            continue;
                        };
                        if speed < 25. || range > 40000. {
                            continue;
                        }
                        let start = arrival - first.cpa_s;
                        if !within_time(job, start) {
                            continue;
                        }
                        let score = feature_error(first, range, speed);
                        if score > 12. {
                            continue;
                        }
                        if let Some(seed) =
                            complete_seed(point, start, anchor, i, score, events, |_, _| {
                                cpas.iter()
                                    .enumerate()
                                    .filter_map(|(j, cpa)| cpa.map(|(t, r, v)| (j, t, r, v)))
                                    .collect()
                            })
                        {
                            here.push(seed);
                        }
                    }
                }
                here.sort_by(|a, b| a.score.total_cmp(&b.score));
                here.truncate(3);
                here
            })
            .collect();
        seeds.append(&mut batch_seeds);
    }
    seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
    // Keep multiple basins/flight identities so the refinement is not all one grid cell.
    let mut diverse: Vec<Seed> = vec![];
    for seed in seeds {
        if diverse.iter().any(|s| {
            s.indices == seed.indices
                && s.point.distance(seed.point) < spacing * 0.8
                && (s.start - seed.start).abs() < 20.
        }) {
            continue;
        }
        diverse.push(seed);
        // Real long recordings admit many accidental coarse associations.
        // Preserve more basins before applying the full observed-curve model.
        if diverse.len() == 512 {
            break;
        }
    }
    let mut candidates = vec![];
    let count = diverse.len();
    let mut incomplete_references = 0;
    let first_event = events.iter().map(|e| e.cpa_s).fold(f64::INFINITY, f64::min);
    let last_event = events.iter().map(|e| e.cpa_s).fold(0., f64::max);
    // A short recording cannot constrain another clock parameter independently.
    let drift = if events.len() >= 3 && last_event - first_event >= 600. {
        job.max_clock_drift_ppm * 1e-6
    } else {
        0.
    };
    let pivot = if job.start_utc.is_none() {
        (first_event + last_event) * 0.5
    } else {
        0.
    };
    for (i, mut seed) in diverse.into_iter().enumerate() {
        control.check()?;
        progress(
            "Refining location and the shared recording clock",
            0.73 + 0.25 * i as f32 / count.max(1) as f32,
        );
        seed.score = exact_score(&seed, events, tracks, c).0;
        let mut spatial = (spacing * 0.6).max(job.grid_m);
        let mut temporal = 4.;
        let mut rate_step = drift * 0.5;
        for iteration in 0..if drift > 0. { 50 } else { 26 } {
            control.check()?;
            let mut improved = false;
            for axis in 0..if drift > 0. { 4 } else { 3 } {
                for sign in [-1., 1.] {
                    let mut trial = seed.clone();
                    match axis {
                        0 => trial.point.lat += sign * spatial / 111320.,
                        1 => {
                            trial.point.lon += sign * spatial
                                / (111320. * trial.point.lat.to_radians().cos().max(0.05))
                        }
                        2 => trial.start += sign * temporal,
                        _ => {
                            trial.clock_rate += sign * rate_step;
                            trial.start -= sign * rate_step * pivot;
                        }
                    }
                    if !bounds.contains(trial.point)
                        || !within_time(job, trial.start)
                        || (trial.clock_rate - 1.).abs() > drift + 1e-12
                    {
                        continue;
                    }
                    trial.point.alt = terrain.elevation(trial.point);
                    trial.score = exact_score(&trial, events, tracks, c).0;
                    if trial.score < seed.score {
                        seed = trial;
                        improved = true;
                    }
                }
            }
            if !improved || iteration % 5 == 4 {
                spatial *= 0.5;
                temporal *= 0.5;
                rate_step *= 0.5;
            }
            if spatial < 15. && temporal < 0.08 && rate_step < 0.000025 {
                break;
            }
        }
        let (objective, matches) = exact_score(&seed, events, tracks, c);
        if matches.len() < events.len().min(3) {
            continue;
        }
        if (seed.clock_rate - 1.).abs() > 1e-12 {
            let mut matched_times = matches
                .iter()
                .filter_map(|m| m.event_index)
                .map(|i| events[i].cpa_s);
            let first = matched_times.next().unwrap_or(0.);
            let (min, max) =
                matched_times.fold((first, first), |(min, max), t| (min.min(t), max.max(t)));
            if max - min < 600. {
                continue;
            }
        }
        if !objective.is_finite() {
            incomplete_references += 1;
        }
        let score = if objective.is_finite() {
            match_error(&matches)
        } else {
            objective
        };
        if !score.is_finite()
            || score > 1.0
            || matches
                .iter()
                .any(|m| m.coverage < 0.9 || m.rmse_hz / m.fitted_tone_hz > 0.015)
        {
            continue;
        }
        let Some(time) =
            chrono::DateTime::from_timestamp_millis((seed.start * 1000.).round() as i64)
        else {
            continue;
        };
        let conflicts = if job.silence_mode != SilenceMode::Off {
            silence_conflicts_with_rate(events, tracks, seed.point, seed.start, seed.clock_rate, c)
        } else {
            vec![]
        };
        if job.silence_mode == SilenceMode::Strict && !conflicts.is_empty() {
            continue;
        }
        candidates.push(Candidate {
            latitude: seed.point.lat,
            longitude: seed.point.lon,
            ground_m: seed.point.alt,
            recording_start_utc: time.to_rfc3339(),
            recording_start_unix: seed.start,
            recording_clock_rate: seed.clock_rate,
            relative_error_percent: score,
            matches,
            silence_conflicts: conflicts,
        });
    }
    candidates.sort_by(|a, b| {
        b.matches.len().cmp(&a.matches.len()).then_with(|| {
            (a.relative_error_percent + 0.25 * a.silence_conflicts.len() as f64)
                .total_cmp(&(b.relative_error_percent + 0.25 * b.silence_conflicts.len() as f64))
        })
    });
    let mut distinct: Vec<Candidate> = vec![];
    for candidate in candidates {
        let p = Point {
            lat: candidate.latitude,
            lon: candidate.longitude,
            alt: 0.,
        };
        if distinct.iter().any(|s| {
            p.distance(Point {
                lat: s.latitude,
                lon: s.longitude,
                alt: 0.,
            }) < 250.
                && (candidate.recording_start_unix - s.recording_start_unix).abs() < 10.
                && (candidate.recording_start_unix - s.recording_start_unix
                    + last_event * (candidate.recording_clock_rate - s.recording_clock_rate))
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
    let mut warnings = vec![];
    if events.len() > anchors.len() {
        warnings.push(format!("The strongest {} of {} detections seed the bounded search; all retained detections are available as corroborating evidence. Weak anchors can therefore be missed.", anchors.len(), events.len()));
    }
    if job.silence_mode != SilenceMode::Off {
        warnings.push("Quiet-gap constraints compare known propeller aircraft passing closer than 80% of the detected flyovers' minimum slant range. Audibility is an assumption: strict mode can reject the true location. Unknown aircraft are not negative evidence.".into());
    }
    if spacing > job.grid_m * 1.1 {
        warnings.push(format!("Search budget enlarged the initial grid to {spacing:.0} m. Local refinement does not make this an exhaustive search; narrow the region to avoid missing locations."));
    }
    if distinct.is_empty() {
        warnings.push("No candidate passed the joint curve-fit threshold in the supplied flight data and search region. This does not prove the location is absent.".into());
    }
    if distinct.iter().any(|c| c.matches.len() < events.len()) {
        warnings.push("Some candidates leave detections unmatched. Only explicitly indexed matches support a candidate; the other sounds may be unrelated or lack reference data. Partial consensus remains ambiguous.".into());
    }
    if incomplete_references > 0 {
        warnings.push(format!("{incomplete_references} of {count} refined hypotheses lacked enough independent events with at least 90% reference-position coverage. A downloaded archive can still have receiver-coverage gaps; a denser reference source may be needed."));
    }
    if distinct.len() > 1
        && distinct[1].relative_error_percent < distinct[0].relative_error_percent * 1.5 + 0.05
    {
        warnings.push(
            "Several distinct locations or times fit similarly well. The result is ambiguous."
                .into(),
        );
    }
    Ok(SearchResult {
        candidates: distinct,
        warnings,
        coarse_grid_m: spacing,
        hypotheses,
        method: if indexed {
            "indexed_flight_corridors"
        } else {
            "regional_grid"
        }
        .into(),
    })
}

/// A silence constraint is conditional negative evidence, not a claim that every
/// airplane must be audible. Only known propellers appreciably closer than the
/// observed aircraft count, and event edges have a ten-second guard.
pub fn silence_conflicts(
    events: &[Event],
    tracks: &[Track],
    point: Point,
    start: f64,
    c: f64,
) -> Vec<SilenceConflict> {
    silence_conflicts_with_rate(events, tracks, point, start, 1., c)
}

fn silence_conflicts_with_rate(
    events: &[Event],
    tracks: &[Track],
    point: Point,
    start: f64,
    rate: f64,
    c: f64,
) -> Vec<SilenceConflict> {
    let radius = events
        .iter()
        .map(|e| e.slant_m)
        .fold(f64::INFINITY, f64::min)
        * 0.8;
    let mut conflicts = vec![];
    for gap in events.windows(2) {
        let from = start + rate * (gap[0].end_s + 10.);
        let to = start + rate * (gap[1].start_s - 10.);
        if to - from < 10. {
            continue;
        }
        for track in tracks
            .iter()
            .filter(|t| t.kind == crate::aircraft::AircraftKind::Propeller)
        {
            if let Some((arrival, range, speed)) = track.cpa_near(point.ecef(), c, Some((from, to)))
                && range < radius
                && speed > 25.
            {
                conflicts.push(SilenceConflict {
                    aircraft: track.id.clone(),
                    time_in_recording_s: (arrival - start) / rate,
                    slant_m: range,
                });
            }
        }
    }
    conflicts
}

fn match_error(matches: &[Match]) -> f64 {
    (matches
        .iter()
        .map(|m| (100. * m.rmse_hz / m.fitted_tone_hz).powi(2) + (1. - m.coverage).powi(2))
        .sum::<f64>()
        / matches.len() as f64)
        .sqrt()
}

fn exact_score(seed: &Seed, events: &[Event], tracks: &[Track], c: f64) -> (f64, Vec<Match>) {
    let mut matches = vec![];
    for (event_index, (event, &index)) in events.iter().zip(&seed.indices).enumerate() {
        if index == usize::MAX {
            continue;
        }
        let track = &tracks[index];
        let mut predicted = vec![];
        let mut used = vec![];
        let mut used_times = vec![];
        let mut ff = 0.;
        let mut yf = 0.;
        for p in &event.ridge {
            if let Some(f) = track.factor(
                seed.point.ecef(),
                seed.start + seed.clock_rate * p.time_s,
                c,
            ) {
                ff += f * f;
                yf += p.hz * f;
                predicted.push(f);
                used.push(p.hz);
                used_times.push(p.time_s);
            } else {
                predicted.push(f64::NAN);
            }
        }
        let coverage = used.len() as f64 / event.ridge.len() as f64;
        if coverage < 0.9 || ff == 0. {
            continue;
        }
        let base = yf / ff;
        let mut sum = 0.;
        let mut i = 0;
        for f in &mut predicted {
            if f.is_finite() {
                *f *= base;
                sum += (used[i] - *f).powi(2);
                i += 1;
            } else {
                *f = 0.;
            }
        }
        let rmse = (sum / used.len() as f64).sqrt();
        let linear_rmse =
            crate::audio::linear_baseline_rmse(used_times.into_iter().zip(used.iter().copied()));
        matches.push(Match {
            event_index: Some(event_index),
            aircraft: track.id.clone(),
            rmse_hz: rmse,
            fitted_tone_hz: base,
            coverage,
            linear_baseline_rmse_hz: Some(linear_rmse),
            modeled_hz: predicted,
        });
    }
    let required = events.len().min(3);
    if matches.len() < required {
        return (f64::INFINITY, vec![]);
    }
    // Optimize a fixed-size independent consensus, so an unrelated detection
    // cannot pull the clock or point away. Extra associations must also preserve
    // the joint acceptance threshold; individually plausible fits can otherwise
    // invalidate a passing core merely because more sounds were detected.
    matches
        .sort_by(|a, b| (a.rmse_hz / a.fitted_tone_hz).total_cmp(&(b.rmse_hz / b.fitted_tone_hz)));
    let objective = match_error(&matches[..required]);
    // Keep a finite optimization objective outside the acceptance boundary.
    // Only models that explain curvature better than a free linear drift can
    // count toward the final independent-flight consensus.
    matches.retain(|m| {
        m.linear_baseline_rmse_hz.is_some_and(|linear| {
            linear > f64::EPSILON && m.rmse_hz.powi(2) <= 0.5 * linear.powi(2)
        })
    });
    if matches.len() < required {
        return (objective, vec![]);
    }
    let mut passing = required;
    while passing < matches.len()
        && matches[passing].rmse_hz / matches[passing].fitted_tone_hz <= 0.015
        && match_error(&matches[..=passing]) <= 1.0
    {
        passing += 1;
    }
    matches.truncate(passing);
    matches.sort_by_key(|m| m.event_index);
    (objective, matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{audio::RidgePoint, demo};

    #[test]
    fn a_linear_drift_does_not_corroborate_an_unrelated_flight() {
        let tracks = demo::tracks();
        let point = Point {
            lat: demo::TRUTH.lat + 0.05,
            lon: demo::TRUTH.lon + 0.05,
            alt: demo::TRUTH.alt,
        };
        let first = tracks[0]
            .factor(point.ecef(), demo::EPOCH + 10., 340.39)
            .unwrap();
        let last = tracks[0]
            .factor(point.ecef(), demo::EPOCH + 70., 340.39)
            .unwrap();
        let ridge = (0..121)
            .map(|i| RidgePoint {
                time_s: 10. + i as f64 * 0.5,
                hz: 100. * (first + (last - first) * i as f64 / 120.),
                contrast_db: 20.,
            })
            .collect();
        let event = Event {
            start_s: 10.,
            end_s: 70.,
            cpa_s: 40.,
            base_hz: 100.,
            speed_mps: 72.,
            slant_m: 7000.,
            fit_rmse_hz: 0.,
            contrast_db: 20.,
            ridge,
        };
        let seed = Seed {
            point,
            start: demo::EPOCH,
            clock_rate: 1.,
            indices: vec![0],
            score: 0.,
        };
        let (_, matches) = exact_score(&seed, &[event], &tracks, 340.39);
        assert!(
            matches.is_empty(),
            "Small relative tone error is not evidence when a straight line explains the observations better"
        );
    }

    #[test]
    fn weak_extra_match_preserves_a_passing_three_flight_consensus() {
        let mut tracks = Vec::new();
        let mut events = Vec::new();
        for index in 0..4 {
            let shift = index as f64 * 200.;
            let mut track = demo::tracks().remove(0);
            track.id = format!("flight-{index}");
            for sample in &mut track.samples {
                sample.time += shift;
            }
            let noise = if index == 3 { 1.4 } else { 0.92 };
            let ridge = (0..120)
                .map(|i| {
                    let time_s = 10. + i as f64 * 0.5 + shift;
                    let factor = track
                        .factor(demo::TRUTH.ecef(), demo::EPOCH + time_s, 340.39)
                        .unwrap();
                    RidgePoint {
                        time_s,
                        hz: 100. * factor + if i % 2 == 0 { noise } else { -noise },
                        contrast_db: 20.,
                    }
                })
                .collect();
            events.push(Event {
                start_s: 10. + shift,
                end_s: 69.5 + shift,
                cpa_s: 39. + shift,
                base_hz: 100.,
                speed_mps: 72.,
                slant_m: 1300.,
                fit_rmse_hz: noise,
                contrast_db: 20.,
                ridge,
            });
            tracks.push(track);
        }
        let seed = Seed {
            point: demo::TRUTH,
            start: demo::EPOCH,
            clock_rate: 1.,
            indices: vec![0, 1, 2, 3],
            score: 0.,
        };
        let (core_error, matches) = exact_score(&seed, &events, &tracks, 340.39);
        assert!(core_error < 1., "The three-flight core must pass");
        assert!(
            match_error(&matches) <= 1.,
            "Adding a weaker fourth fit must not invalidate the core"
        );
        assert_eq!(
            matches.iter().map(|m| m.event_index).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
    }
}
