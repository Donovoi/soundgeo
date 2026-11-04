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
    pub modeled_hz: Vec<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub latitude: f64,
    pub longitude: f64,
    pub ground_m: f64,
    pub recording_start_utc: String,
    pub recording_start_unix: f64,
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
    pub indices: Vec<usize>,
    pub score: f64,
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
    for (i, mut seed) in diverse.into_iter().enumerate() {
        control.check()?;
        progress(
            "Refining location and the shared recording clock",
            0.73 + 0.25 * i as f32 / count.max(1) as f32,
        );
        seed.score = exact_score(&seed, events, tracks, c).0;
        let mut spatial = (spacing * 0.6).max(job.grid_m);
        let mut temporal = 4.;
        for iteration in 0..26 {
            control.check()?;
            let mut improved = false;
            for axis in 0..3 {
                for sign in [-1., 1.] {
                    let mut trial = seed.clone();
                    match axis {
                        0 => trial.point.lat += sign * spatial / 111320.,
                        1 => {
                            trial.point.lon += sign * spatial
                                / (111320. * trial.point.lat.to_radians().cos().max(0.05))
                        }
                        _ => trial.start += sign * temporal,
                    }
                    if !bounds.contains(trial.point) || !within_time(job, trial.start) {
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
            }
            if spatial < 15. && temporal < 0.08 {
                break;
            }
        }
        let (objective, matches) = exact_score(&seed, events, tracks, c);
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
            silence_conflicts(events, tracks, seed.point, seed.start, c)
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
    let radius = events
        .iter()
        .map(|e| e.slant_m)
        .fold(f64::INFINITY, f64::min)
        * 0.8;
    let mut conflicts = vec![];
    for gap in events.windows(2) {
        let from = start + gap[0].end_s + 10.;
        let to = start + gap[1].start_s - 10.;
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
                    time_in_recording_s: arrival - start,
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
        let mut ff = 0.;
        let mut yf = 0.;
        for p in &event.ridge {
            if let Some(f) = track.factor(seed.point.ecef(), seed.start + p.time_s, c) {
                ff += f * f;
                yf += p.hz * f;
                predicted.push(f);
                used.push(p.hz);
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
        matches.push(Match {
            event_index: Some(event_index),
            aircraft: track.id.clone(),
            rmse_hz: rmse,
            fitted_tone_hz: base,
            coverage,
            modeled_hz: predicted,
        });
    }
    let required = events.len().min(3);
    if matches.len() < required {
        return (f64::INFINITY, vec![]);
    }
    // Optimize a fixed-size independent consensus, so an unrelated detection
    // cannot pull the clock or point away. Report every additional passing fit.
    matches
        .sort_by(|a, b| (a.rmse_hz / a.fitted_tone_hz).total_cmp(&(b.rmse_hz / b.fitted_tone_hz)));
    let objective = match_error(&matches[..required]);
    let passing = matches
        .iter()
        .take_while(|m| m.rmse_hz / m.fitted_tone_hz <= 0.015)
        .count();
    matches.truncate(passing.max(required));
    matches.sort_by_key(|m| m.event_index);
    (objective, matches)
}
