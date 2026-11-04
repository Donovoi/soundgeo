use crate::{
    Control, Job, Progress,
    audio::Event,
    geo::{Bounds, Point, Terrain},
    model::Track,
};
use anyhow::{Result, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Match {
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
}
pub struct SearchResult {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
    pub coarse_grid_m: f64,
}
#[derive(Clone)]
struct Seed {
    point: Point,
    start: f64,
    indices: Vec<usize>,
    score: f64,
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
    b.south = (b.south - 0.04).max(-85.);
    b.north = (b.north + 0.04).min(85.);
    b.west = (b.west - 0.04).max(-180.);
    b.east = (b.east + 0.04).min(180.);
    b.validate()?;
    Ok(b)
}
fn feature_error(e: &Event, range: f64, speed: f64) -> f64 {
    ((range / e.slant_m.max(100.)).ln() / 0.7).powi(2) + ((speed / e.speed_mps).ln() / 0.5).powi(2)
}
fn within_time(job: &Job, start: f64) -> bool {
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
    let (mut grid, spacing) = bounds.grid(job.grid_m, job.max_grid_points);
    for p in &mut grid {
        control.check()?;
        p.alt = terrain.elevation(*p);
    }
    let c = job.sound_speed();
    let first = &events[0];
    let mut seeds = vec![];
    for (batch_index, batch) in grid.chunks(64).enumerate() {
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
                    let mut score = feature_error(first, range, speed);
                    if score > 12. {
                        continue;
                    }
                    let mut indices = vec![i];
                    for event in events.iter().skip(1) {
                        let wanted = start + event.cpa_s;
                        let best = cpas
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| !indices.contains(j))
                            .filter_map(|(j, cpa)| {
                                let (time, range, speed) = (*cpa)?;
                                if speed < 25. || range > 40000. || (time - wanted).abs() > 45. {
                                    return None;
                                }
                                Some((
                                    j,
                                    feature_error(event, range, speed)
                                        + ((time - wanted) / 10.).powi(2),
                                ))
                            })
                            .min_by(|a, b| a.1.total_cmp(&b.1));
                        if let Some((j, s)) = best {
                            indices.push(j);
                            score += s;
                        } else {
                            score = f64::INFINITY;
                            break;
                        }
                    }
                    if score.is_finite() {
                        here.push(Seed {
                            point,
                            start,
                            indices,
                            score,
                        });
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
        if diverse.len() == 48 {
            break;
        }
    }
    let mut candidates = vec![];
    let count = diverse.len();
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
        let (score, matches) = exact_score(&seed, events, tracks, c);
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
        candidates.push(Candidate {
            latitude: seed.point.lat,
            longitude: seed.point.lon,
            ground_m: seed.point.alt,
            recording_start_utc: time.to_rfc3339(),
            recording_start_unix: seed.start,
            relative_error_percent: score,
            matches,
        });
    }
    candidates.sort_by(|a, b| {
        a.relative_error_percent
            .total_cmp(&b.relative_error_percent)
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
    if spacing > job.grid_m * 1.1 {
        warnings.push(format!("Search budget enlarged the initial grid to {spacing:.0} m. Local refinement does not make this an exhaustive search; narrow the region to avoid missing locations."));
    }
    if distinct.is_empty() {
        warnings.push("No candidate passed the joint curve-fit threshold in the supplied flight data and search region. This does not prove the location is absent.".into());
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
    })
}

fn exact_score(seed: &Seed, events: &[Event], tracks: &[Track], c: f64) -> (f64, Vec<Match>) {
    let mut matches = vec![];
    let mut relative = 0.;
    for (event, &index) in events.iter().zip(&seed.indices) {
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
            return (f64::INFINITY, vec![]);
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
        relative += (100. * rmse / base).powi(2) + (1. - coverage).powi(2);
        matches.push(Match {
            aircraft: track.id.clone(),
            rmse_hz: rmse,
            fitted_tone_hz: base,
            coverage,
            modeled_hz: predicted,
        });
    }
    ((relative / events.len() as f64).sqrt(), matches)
}
