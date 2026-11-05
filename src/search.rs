use crate::{
    Control, Job, Progress,
    audio::Event,
    geo::{Bounds, Point, Terrain},
    model::Track,
};
use anyhow::{Result, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
    /// Zero is the primary trace; higher values select an alternative of that sound.
    #[serde(default)]
    pub event_variant: usize,
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
impl Candidate {
    /// Three sounds can fit the extra clock-rate parameter without providing
    /// further corroboration. A real negative control passed that minimum.
    /// This conservative guard is not a calibrated probability of correctness.
    pub fn clock_fit_is_provisional(&self) -> bool {
        (self.recording_clock_rate - 1.).abs() > 1e-12 && self.matches.len() < 4
    }
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
    pub anchor: usize,
    pub indices: Vec<usize>,
    pub alternatives: Arc<Vec<[usize; 3]>>,
    pub score: f64,
    // Coarse pruning does not change a retained proposal's point or options.
    // Profile new proposals once instead of repeating the same clock search
    // whenever another worker batch is merged into the beam.
    pub coarse_profiled: bool,
    // Full-curve corroboration at the immutable coarse point and clock.
    // Recompute after profiling before final refinement selection.
    corroboration: Option<Corroboration>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Corroboration {
    matches: usize,
    error: f64,
}

struct CorroboratedFit {
    seed: Seed,
    matches: Vec<Match>,
    error: f64,
}

fn accepted_fit_error(
    seed: &Seed,
    events: &[Event],
    objective: f64,
    matches: &[Match],
) -> Option<f64> {
    if !objective.is_finite() || matches.len() < events.len().min(3) {
        return None;
    }
    if (seed.clock_rate - 1.).abs() > 1e-12 {
        let mut times = matches
            .iter()
            .filter_map(|m| m.event_index)
            .map(|i| events[i].cpa_s);
        let first = times.next().unwrap_or(0.);
        let (min, max) = times.fold((first, first), |(min, max), t| (min.min(t), max.max(t)));
        if max - min < 600. {
            return None;
        }
    }
    let error = match_error(matches);
    if !error.is_finite()
        || error > 1.
        || matches
            .iter()
            .any(|m| m.coverage < 0.9 || m.rmse_hz / m.fitted_tone_hz > 0.015)
    {
        return None;
    }
    Some(error)
}

fn retain_corroborated_fit(
    best: &mut Option<CorroboratedFit>,
    seed: &Seed,
    events: &[Event],
    objective: f64,
    matches: Vec<Match>,
) {
    // The objective fits a minimum consensus. Preserve additional valid curves
    // if a later numerical improvement would discard their corroboration.
    // Minimum-consensus-only results retain their existing behavior.
    if matches.len() <= events.len().min(3) {
        return;
    }
    let Some(error) = accepted_fit_error(seed, events, objective, &matches) else {
        return;
    };
    if best.as_ref().is_none_or(|previous| {
        matches.len() > previous.matches.len()
            || (matches.len() == previous.matches.len() && error < previous.error)
    }) {
        *best = Some(CorroboratedFit {
            seed: seed.clone(),
            matches,
            error,
        });
    }
}

/// Refine the complete accepted consensus without changing its observations.
/// The minimum-consensus endpoint is retained separately. Every extra fit must
/// preserve all of these curves and their original acceptance checks.
fn refine_complete_fit(
    original: &CorroboratedFit,
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    bounds: Bounds,
    terrain: &mut Terrain,
    control: &Control,
) -> Result<Option<CorroboratedFit>> {
    if original.matches.len() <= events.len().min(3) {
        return Ok(None);
    }
    let c = job.sound_speed();
    let drift = allowed_clock_drift(job, events);
    let first = events.iter().map(|e| e.cpa_s).fold(f64::INFINITY, f64::min);
    let last = events.iter().map(|e| e.cpa_s).fold(0., f64::max);
    let pivot = if job.start_utc.is_none() {
        (first + last) * 0.5
    } else {
        0.
    };
    let mut seed = original.seed.clone();
    let mut error = original.error;
    let mut improved_fit = None;
    let mut spatial = job.grid_m;
    let mut temporal = 4.;
    let mut rate_step = drift * 0.5;
    for iteration in 0..50 {
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
                let matches: Option<Vec<_>> = original
                    .matches
                    .iter()
                    .map(|previous| {
                        let index = previous.event_index?;
                        let event = events.get(index)?.variant(previous.event_variant)?;
                        let track = tracks.get(*trial.indices.get(index)?)?;
                        let fitted = fit_observation(
                            &trial,
                            event,
                            track,
                            c,
                            index,
                            previous.event_variant,
                        )?;
                        let preserves_samples = fitted.modeled_hz.len()
                            == previous.modeled_hz.len()
                            && previous
                                .modeled_hz
                                .iter()
                                .zip(&fitted.modeled_hz)
                                .all(|(old, new)| *old == 0. || *new != 0.);
                        (fitted.aircraft == previous.aircraft
                            && preserves_samples
                            && has_curvature(&fitted))
                        .then_some(fitted)
                    })
                    .collect();
                let Some(matches) = matches else { continue };
                let objective = match_error(&matches);
                let Some(trial_error) = accepted_fit_error(&trial, events, objective, &matches)
                else {
                    continue;
                };
                if trial_error < error {
                    trial.score = objective;
                    seed = trial;
                    error = trial_error;
                    improved = true;
                    improved_fit = Some(CorroboratedFit {
                        seed: seed.clone(),
                        matches,
                        error,
                    });
                }
            }
        }
        if !improved || iteration % 5 == 4 {
            spatial *= 0.5;
            temporal *= 0.5;
            rate_step *= 0.5;
        }
        if spatial < 1. && temporal < 0.01 && rate_step < 0.000002 {
            break;
        }
    }
    Ok(improved_fit)
}

fn unit_clock_rate() -> f64 {
    1.
}

fn full_corroboration(
    seed: &Seed,
    events: &[Event],
    tracks: &[Track],
    c: f64,
) -> Option<Corroboration> {
    let (objective, matches) = exact_score(seed, events, tracks, c);
    corroboration_for_fit(seed, events, objective, &matches)
}

fn corroboration_for_fit(
    seed: &Seed,
    events: &[Event],
    objective: f64,
    matches: &[Match],
) -> Option<Corroboration> {
    if matches.len() <= events.len().min(3) {
        return None;
    }
    accepted_fit_error(seed, events, objective, matches).map(|error| Corroboration {
        matches: matches.len(),
        error,
    })
}

fn corroboration_order(a: &Seed, b: &Seed) -> std::cmp::Ordering {
    let a_support = a.corroboration.expect("Only corroborated seeds are ranked");
    let b_support = b.corroboration.expect("Only corroborated seeds are ranked");
    b_support
        .matches
        .cmp(&a_support.matches)
        .then_with(|| a_support.error.total_cmp(&b_support.error))
        .then_with(|| a.score.total_cmp(&b.score))
}

fn coarse_cell(seed: &Seed, spacing: f64) -> (Vec<usize>, i64, i64, i64, i64) {
    let xyz = seed.point.ecef();
    let cell = spacing * 0.8;
    (
        seed.indices.clone(),
        (seed.start / 20.).floor() as i64,
        (xyz.0 / cell).floor() as i64,
        (xyz.1 / cell).floor() as i64,
        (xyz.2 / cell).floor() as i64,
    )
}

fn coarse_indices(
    seeds: &[Seed],
    order: impl Iterator<Item = usize>,
    limit: usize,
    spacing: f64,
) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    let mut selected = vec![];
    for i in order {
        if selected.len() == limit {
            break;
        }
        if seen.insert(coarse_cell(&seeds[i], spacing)) {
            selected.push(i);
        }
    }
    selected
}

fn combine_beams(seeds: &mut Vec<Seed>, primary: Vec<usize>, additional: Vec<usize>) {
    let mut seen = std::collections::HashSet::new();
    let selected = primary
        .into_iter()
        .chain(additional)
        .filter(|&i| seen.insert(i))
        .map(|i| seeds[i].clone())
        .collect();
    *seeds = selected;
}

/// Bound the coarse beam before expensive curve fitting.
pub(crate) fn prune_coarse_seeds(seeds: &mut Vec<Seed>, limit: usize, spacing: f64) {
    seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
    // Repeated segment/anchor proposals for the same basin must not consume
    // the entire beam before full curves are considered. ECEF cells avoid
    // latitude-dependent sizes; flight identities and clock bins stay distinct.
    let mut seen = std::collections::HashSet::new();
    seeds.retain(|seed| {
        if seen.len() == limit {
            return false;
        }
        seen.insert(coarse_cell(seed, spacing))
    });
}

fn prune_coarse_with_corroboration(seeds: &mut Vec<Seed>, limit: usize, spacing: f64) {
    if !seeds.iter().any(|seed| seed.corroboration.is_some()) {
        prune_coarse_seeds(seeds, limit, spacing);
        return;
    }
    seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
    let primary = coarse_indices(seeds, 0..seeds.len(), limit, spacing);
    let mut support_order: Vec<_> = (0..seeds.len())
        .filter(|&i| seeds[i].corroboration.is_some())
        .collect();
    support_order.sort_by(|&a, &b| corroboration_order(&seeds[a], &seeds[b]));
    // Preserve the error-ranked beam, with a separately bounded alternative.
    // A lower three-curve residual must not erase extra verified curves.
    let additional = coarse_indices(seeds, support_order.into_iter(), limit.div_ceil(4), spacing);
    combine_beams(seeds, primary, additional);
}

fn refinement_indices(
    seeds: &[Seed],
    order: impl Iterator<Item = usize>,
    limit: usize,
    spacing: f64,
) -> Vec<usize> {
    let mut selected: Vec<usize> = vec![];
    for i in order {
        if selected.len() == limit {
            break;
        }
        let seed = &seeds[i];
        if selected.iter().any(|&j| {
            let previous = &seeds[j];
            previous.indices == seed.indices
                && previous.point.distance(seed.point) < spacing * 0.8
                && (previous.start - seed.start).abs() < 20.
        }) {
            continue;
        }
        selected.push(i);
    }
    selected
}

fn select_refinement_seeds(seeds: &mut Vec<Seed>, limit: usize, spacing: f64) {
    seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
    let primary = refinement_indices(seeds, 0..seeds.len(), limit, spacing);
    let mut support_order: Vec<_> = (0..seeds.len())
        .filter(|&i| seeds[i].corroboration.is_some())
        .collect();
    support_order.sort_by(|&a, &b| corroboration_order(&seeds[a], &seeds[b]));
    let additional =
        refinement_indices(seeds, support_order.into_iter(), limit.div_ceil(4), spacing);
    combine_beams(seeds, primary, additional);
}

pub(crate) fn score_and_prune_seeds(
    seeds: &mut Vec<Seed>,
    limit: usize,
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    control: &Control,
) -> Result<()> {
    let evaluate = |seed: &mut Seed| -> Result<()> {
        control.check()?;
        if !seed.coarse_profiled {
            profile_seed_clock(seed, job, events, tracks);
            seed.corroboration = full_corroboration(seed, events, tracks, job.sound_speed());
            seed.coarse_profiled = true;
        }
        Ok(())
    };
    // Use the caller's bounded corridor pool, including nested worker batches.
    // Keep the terrain-cache fallback serial. Each seed is independent, and
    // sorting/pruning still happens once in its original deterministic order.
    if rayon::current_thread_index().is_some() {
        seeds.par_iter_mut().try_for_each(evaluate)?;
    } else {
        for seed in seeds.iter_mut() {
            evaluate(seed)?;
        }
    }
    prune_coarse_with_corroboration(seeds, limit, job.grid_m);
    Ok(())
}

fn allowed_clock_drift(job: &Job, events: &[Event]) -> f64 {
    let first = events.iter().map(|e| e.cpa_s).fold(f64::INFINITY, f64::min);
    let last = events.iter().map(|e| e.cpa_s).fold(0., f64::max);
    if events.len() >= 3 && last - first >= 600. {
        job.max_clock_drift_ppm * 1e-6
    } else {
        0.
    }
}

/// Proposal ranking only: sample the curve across its full extent. Final
/// optimization and acceptance still evaluate every observed ridge sample.
fn sampled_curve_fit(seed: &Seed, events: &[Event], tracks: &[Track], c: f64) -> (f64, Vec<usize>) {
    sampled_curve_fit_prepared(seed, events, tracks, c, None)
}

fn sampled_curve_fit_prepared<'a>(
    seed: &Seed,
    events: &[Event],
    tracks: &'a [Track],
    c: f64,
    mut prepared: Option<&mut std::collections::HashMap<usize, crate::model::PreparedTrack<'a>>>,
) -> (f64, Vec<usize>) {
    let observer = seed.point.ecef();
    let mut current_errors = Vec::new();
    let mut current_used = std::collections::HashSet::new();
    let mut edges = Vec::new();
    for (event_index, (event, &current)) in events.iter().zip(&seed.indices).enumerate() {
        let options = seed.alternatives.get(event_index).copied().unwrap_or([
            current,
            usize::MAX,
            usize::MAX,
        ]);
        for index in options.into_iter().filter(|&index| index != usize::MAX) {
            let track = prepared.as_mut().map(|cache| {
                cache
                    .entry(index)
                    .or_insert_with(|| tracks[index].prepare(observer, c))
            });
            let mut best = f64::INFINITY;
            for event in event.variants() {
                let n = event.ridge.len().min(16);
                if n < 2 {
                    continue;
                }
                let (mut ff, mut yf, mut yy, mut used) = (0., 0., 0., 0);
                for i in 0..n {
                    let p = &event.ridge[i * (event.ridge.len() - 1) / (n - 1)];
                    let arrival = seed.start + seed.clock_rate * p.time_s;
                    let factor = match &track {
                        Some(prepared) => prepared.factor(arrival),
                        None => tracks[index].factor(observer, arrival, c),
                    };
                    if let Some(f) = factor {
                        ff += f * f;
                        yf += p.hz * f;
                        yy += p.hz * p.hz;
                        used += 1;
                    }
                }
                let coverage = used as f64 / n as f64;
                if coverage >= 0.9 && ff > 0. && yf > 0. {
                    let base = yf / ff;
                    let mse = (yy - yf * yf / ff).max(0.) / used as f64;
                    best = best.min(10000. * mse / base.powi(2) + (1. - coverage).powi(2));
                }
            }
            if best.is_finite() {
                edges.push((best, event_index, index));
                if index == current && current_used.insert(index) {
                    current_errors.push(best);
                }
            }
        }
    }
    let required = events.len().min(3);
    let core_score = |errors: &mut Vec<f64>| {
        if required == 0 || errors.len() < required {
            return f64::INFINITY;
        }
        errors.sort_by(f64::total_cmp);
        (errors[..required].iter().sum::<f64>() / required as f64).sqrt()
    };
    let current_score = core_score(&mut current_errors);
    if seed.alternatives.is_empty() {
        return (current_score, seed.indices.clone());
    }
    // Compare plausible associations with the measured curves at one shared
    // clock. One flight segment cannot corroborate two separate detections.
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let (score, selected) = independent_curve_assignment(&edges, events.len(), required);
    if score < current_score {
        (score, selected)
    } else {
        (current_score, seed.indices.clone())
    }
}

fn independent_curve_assignment(
    edges: &[(f64, usize, usize)],
    event_count: usize,
    required: usize,
) -> (f64, Vec<usize>) {
    let mut selected = vec![usize::MAX; event_count];
    let mut used = std::collections::HashSet::new();
    let mut greedy = Vec::new();
    for (i, &(_, event, track)) in edges.iter().enumerate() {
        if selected[event] == usize::MAX && used.insert(track) {
            selected[event] = track;
            if greedy.len() < required {
                greedy.push(i);
            }
        }
    }
    if required == 0 || edges.len() < required {
        return (f64::INFINITY, selected);
    }
    if greedy.len() < required {
        let tracks: std::collections::HashSet<_> = edges.iter().map(|e| e.2).collect();
        let events: std::collections::HashSet<_> = edges.iter().map(|e| e.1).collect();
        if tracks.len() < required || events.len() < required {
            return (f64::INFINITY, selected);
        }
    }
    let mut best = if greedy.len() == required {
        (greedy.iter().map(|&i| edges[i].0).sum::<f64>(), greedy)
    } else {
        (f64::INFINITY, Vec::new())
    };

    // Only the best three independent pairs define the coarse objective.
    // Search that small core exactly. Sorted costs give an admissible lower
    // bound, so conflict-free greedy choices finish without branching.
    fn choose(
        edges: &[(f64, usize, usize)],
        from: usize,
        remaining: usize,
        cost: f64,
        chosen: &mut Vec<usize>,
        best: &mut (f64, Vec<usize>),
    ) {
        if edges.len() - from < remaining {
            return;
        }
        for i in from..=edges.len() - remaining {
            let lower = cost + edges[i..i + remaining].iter().map(|e| e.0).sum::<f64>();
            if lower >= best.0 {
                break;
            }
            let edge = edges[i];
            if chosen
                .iter()
                .any(|&j| edges[j].1 == edge.1 || edges[j].2 == edge.2)
            {
                continue;
            }
            chosen.push(i);
            if remaining == 1 {
                *best = (cost + edge.0, chosen.clone());
                chosen.pop();
                break;
            }
            choose(edges, i + 1, remaining - 1, cost + edge.0, chosen, best);
            chosen.pop();
        }
    }
    choose(edges, 0, required, 0., &mut Vec::new(), &mut best);
    if !best.0.is_finite() {
        return (f64::INFINITY, selected);
    }
    selected.fill(usize::MAX);
    used.clear();
    for i in best.1 {
        let (_, event, track) = edges[i];
        selected[event] = track;
        used.insert(track);
    }
    // Retain additional independent associations for full-curve corroboration.
    for &(_, event, track) in edges {
        if selected[event] == usize::MAX && used.insert(track) {
            selected[event] = track;
        }
    }
    ((best.0 / required as f64).sqrt(), selected)
}

fn profile_seed_clock(seed: &mut Seed, job: &Job, events: &[Event], tracks: &[Track]) {
    let c = job.sound_speed();
    let drift = allowed_clock_drift(job, events);
    if drift == 0. {
        (seed.score, seed.indices) = sampled_curve_fit(seed, events, tracks, c);
        return;
    }
    // Only the clock changes during this profile. Reuse arrival geometry for
    // each competing track at this observer; discard it before the point moves.
    let mut prepared = std::collections::HashMap::new();
    (seed.score, seed.indices) =
        sampled_curve_fit_prepared(seed, events, tracks, c, Some(&mut prepared));
    // The initial start was derived from this anchor's received CPA. Preserve
    // that observation while profiling one rate shared by all associations.
    let pivot = events[seed.anchor].cpa_s;
    let unit_start = seed.start + (seed.clock_rate - 1.) * pivot;
    let template = seed.clone();
    // Different coarse starts revisit identical rates. Within this proposal,
    // rate bits determine the same start, point, observations and flight options.
    let mut evaluated_rates = std::collections::HashMap::new();
    let mut evaluate_rate = |rate: f64| {
        let start = unit_start - (rate - 1.) * pivot;
        if (rate - 1.).abs() > drift + 1e-12 || !within_time(job, start) {
            return None;
        }
        let mut trial = template.clone();
        trial.clock_rate = rate;
        trial.start = start;
        let fit = evaluated_rates.entry(rate.to_bits()).or_insert_with(|| {
            sampled_curve_fit_prepared(&trial, events, tracks, c, Some(&mut prepared))
        });
        trial.score = fit.0;
        trial.indices.clone_from(&fit.1);
        Some(trial)
    };
    // A narrow correct clock basin can have a worse coarse score than a broad
    // fit to unrelated flights. Refine each coarse start before comparing them.
    for grid in -4..=4 {
        let Some(mut basin) = evaluate_rate(1. + drift * grid as f64 / 4.) else {
            continue;
        };
        let mut step = drift / 4.;
        for _ in 0..6 {
            step *= 0.5;
            for sign in [-1., 1.] {
                if let Some(trial) = evaluate_rate(basin.clock_rate + sign * step)
                    && trial.score < basin.score
                {
                    basin = trial;
                }
            }
        }
        if basin.score < seed.score {
            *seed = basin;
        }
    }
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
    let mut alternatives = vec![[usize::MAX; 3]; events.len()];
    alternatives[anchor][0] = track;
    let mut edges = vec![(initial_error, anchor, track)];
    for (i, event) in events.iter().enumerate().filter(|(i, _)| *i != anchor) {
        let mut choices = Vec::new();
        for event in event.variants() {
            let wanted = start + event.cpa_s;
            choices.extend(
                nearby(wanted, event)
                    .into_iter()
                    .filter(|(j, time, range, speed)| {
                        *j != track
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
                    .filter(|(_, error)| *error <= FEATURE_ERROR_LIMIT),
            );
        }
        choices.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let mut seen = std::collections::HashSet::new();
        choices.retain(|(id, _)| seen.insert(*id));
        choices.truncate(3);
        for (slot, &(j, _)) in choices.iter().enumerate() {
            alternatives[i][slot] = j;
        }
        edges.extend(choices.into_iter().map(|(j, error)| (error, i, j)));
    }
    // Keep competing options until independent assignment. Reserving a
    // non-anchor flight greedily can discard the only option for a later sound.
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let (score, indices) = independent_curve_assignment(&edges, events.len(), events.len().min(3));
    if !score.is_finite() {
        return None;
    }
    Some(Seed {
        point,
        start,
        clock_rate: 1.,
        anchor,
        indices,
        alternatives: Arc::new(alternatives),
        // Coarse selection must use the same consensus size as exact fitting.
        // Rewarding every loose association first lets unrelated sounds crowd
        // out three precise matches before they ever reach refinement.
        score: score.powi(2),
        coarse_profiled: false,
        corroboration: None,
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
pub(crate) const FEATURE_ERROR_LIMIT: f64 = 12.;
const RANGE_FEATURE_SIGMA: f64 = 0.7;
pub(crate) fn feature_range_factor() -> f64 {
    (RANGE_FEATURE_SIGMA * FEATURE_ERROR_LIMIT.sqrt()).exp()
}
pub(crate) fn feature_error(e: &Event, range: f64, speed: f64) -> f64 {
    ((range / e.slant_m.max(100.)).ln() / RANGE_FEATURE_SIGMA).powi(2)
        + ((speed / e.speed_mps).ln() / 0.5).powi(2)
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
                // complete_seed only accepts CPA times within 45 seconds.
                // Index them once instead of scanning a whole reference day
                // for each alternate observation of every proposed sound.
                let mut timed_cpas: Vec<_> = cpas
                    .iter()
                    .enumerate()
                    .filter_map(|(j, cpa)| cpa.map(|(t, r, v)| (j, t, r, v)))
                    .collect();
                timed_cpas.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                let mut here = vec![];
                for &anchor in &anchors {
                    for first in events[anchor].variants() {
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
                            if score > FEATURE_ERROR_LIMIT {
                                continue;
                            }
                            if let Some(seed) = complete_seed(
                                point,
                                start,
                                anchor,
                                i,
                                score,
                                events,
                                |wanted, _| {
                                    let lo = timed_cpas.partition_point(|v| v.1 < wanted - 45.);
                                    let hi = timed_cpas.partition_point(|v| v.1 <= wanted + 45.);
                                    timed_cpas[lo..hi].to_vec()
                                },
                            ) {
                                here.push(seed);
                            }
                        }
                    }
                }
                // Regional grids need the same acoustic ranking as corridors.
                // A biased straight-flight summary must not discard a curve
                // match before the measured samples have been compared.
                for seed in &mut here {
                    if control.check().is_err() {
                        return Vec::new();
                    }
                    profile_seed_clock(seed, job, events, tracks);
                    seed.corroboration = full_corroboration(seed, events, tracks, c);
                }
                // Keep the original three starts at each point, plus one
                // corroborated start. Zero disables spatial deduplication here.
                select_refinement_seeds(&mut here, 3, 0.);
                here
            })
            .collect();
        control.check()?;
        seeds.append(&mut batch_seeds);
    }
    progress(
        "Ranking complete Doppler curves before local refinement",
        0.72,
    );
    // CPA/range/speed summaries are only a proposal mechanism. Rank surviving
    // proposals using all observed curve samples before applying the final cap.
    for seed in &mut seeds {
        control.check()?;
        profile_seed_clock(seed, job, events, tracks);
        let (objective, matches) = exact_score(seed, events, tracks, c);
        seed.score = objective;
        seed.corroboration = corroboration_for_fit(seed, events, objective, &matches);
    }
    // Keep multiple basins/flight identities so the refinement is not all one grid cell.
    // Extra full-curve support gets its own bounded set, without replacing
    // the original error-ranked 512 basins with loose feature associations.
    select_refinement_seeds(&mut seeds, 512, spacing);
    let diverse = seeds;
    let mut candidates = vec![];
    let count = diverse.len();
    let mut incomplete_references = 0;
    let first_event = events.iter().map(|e| e.cpa_s).fold(f64::INFINITY, f64::min);
    let last_event = events.iter().map(|e| e.cpa_s).fold(0., f64::max);
    // A short recording cannot constrain another clock parameter independently.
    let drift = allowed_clock_drift(job, events);
    let pivot = if job.start_utc.is_none() {
        (first_event + last_event) * 0.5
    } else {
        0.
    };
    for (i, mut seed) in diverse.into_iter().enumerate() {
        control.check()?;
        progress(
            &format!(
                "Refining location and shared clock: hypothesis {}/{}",
                i + 1,
                count
            ),
            0.73 + 0.25 * i as f32 / count.max(1) as f32,
        );
        let (initial_objective, initial_matches) = exact_score(&seed, events, tracks, c);
        seed.score = initial_objective;
        let mut corroborated = None;
        retain_corroborated_fit(
            &mut corroborated,
            &seed,
            events,
            initial_objective,
            initial_matches,
        );
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
                    let (objective, matches) = exact_score(&trial, events, tracks, c);
                    trial.score = objective;
                    retain_corroborated_fit(&mut corroborated, &trial, events, objective, matches);
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
        // Missing temporal coverage produces no matches; count it before that
        // early return so downloaded-but-incomplete tracks remain visible.
        if !objective.is_finite() {
            incomplete_references += 1;
        }
        let final_error = accepted_fit_error(&seed, events, objective, &matches);
        let final_count = final_error.map_or(0, |_| matches.len());
        let mut fits = Vec::with_capacity(2);
        if let Some(error) = final_error {
            fits.push(CorroboratedFit {
                seed,
                matches,
                error,
            });
        }
        // Keep the original endpoint, including its quiet-gap checks. An extra
        // result is considered only when refinement lost valid corroboration.
        if let Some(best) = corroborated
            && best.matches.len() > final_count
        {
            fits.push(best);
        }
        let mut complete_fits = Vec::new();
        for fit in &fits {
            if let Some(refined) =
                refine_complete_fit(fit, job, events, tracks, bounds, terrain, control)?
            {
                complete_fits.push(refined);
            }
        }
        fits.extend(complete_fits);
        for CorroboratedFit {
            seed,
            matches,
            error: score,
        } in fits
        {
            let Some(time) =
                chrono::DateTime::from_timestamp_millis((seed.start * 1000.).round() as i64)
            else {
                continue;
            };
            let conflicts = if job.silence_mode != SilenceMode::Off {
                silence_conflicts_with_rate(
                    events,
                    tracks,
                    seed.point,
                    seed.start,
                    seed.clock_rate,
                    c,
                )
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
    if events.iter().any(|e| e.activity_span_s.is_some()) {
        warnings.push("Overlapping curves and connected slow fragments were counted as one sound. Matching can select a retained alternative, but it cannot count alternatives as independent flights. The full activity interval is excluded from quiet-gap evidence.".into());
    }
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
        warnings.push(format!("{incomplete_references} of {count} final refinement points lacked enough independent events with at least 90% reference-position coverage. A downloaded archive can still have receiver-coverage gaps; a denser reference source may be needed."));
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
        let from = start + rate * (gap[0].activity_end_s() + 10.);
        let to = start + rate * (gap[1].activity_start_s() - 10.);
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

fn fit_observation(
    seed: &Seed,
    event: &Event,
    track: &Track,
    c: f64,
    event_index: usize,
    event_variant: usize,
) -> Option<Match> {
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
        return None;
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
    Some(Match {
        event_index: Some(event_index),
        event_variant,
        aircraft: track.id.clone(),
        rmse_hz: rmse,
        fitted_tone_hz: base,
        coverage,
        linear_baseline_rmse_hz: Some(linear_rmse),
        modeled_hz: predicted,
    })
}

fn has_curvature(m: &Match) -> bool {
    m.linear_baseline_rmse_hz
        .is_some_and(|linear| linear > f64::EPSILON && m.rmse_hz.powi(2) <= 0.5 * linear.powi(2))
}

fn exact_score(seed: &Seed, events: &[Event], tracks: &[Track], c: f64) -> (f64, Vec<Match>) {
    let mut matches = vec![];
    let mut objectives = vec![];
    for (event_index, (event, &index)) in events.iter().zip(&seed.indices).enumerate() {
        if index == usize::MAX {
            continue;
        }
        let mut fitted: Vec<_> = event
            .variants()
            .enumerate()
            .filter_map(|(variant, observation)| {
                fit_observation(seed, observation, &tracks[index], c, event_index, variant)
            })
            .collect();
        fitted.sort_by(|a, b| {
            (a.rmse_hz / a.fitted_tone_hz).total_cmp(&(b.rmse_hz / b.fitted_tone_hz))
        });
        if let Some(best) = fitted.first() {
            objectives.push(best.clone());
        }
        if let Some(best) = fitted.into_iter().find(has_curvature) {
            matches.push(best);
        }
    }
    let required = events.len().min(3);
    if objectives.len() < required {
        return (f64::INFINITY, vec![]);
    }
    // Optimize a fixed-size independent consensus, so an unrelated detection
    // cannot pull the clock or point away. Extra associations must also preserve
    // the joint acceptance threshold; individually plausible fits can otherwise
    // invalidate a passing core merely because more sounds were detected.
    matches
        .sort_by(|a, b| (a.rmse_hz / a.fitted_tone_hz).total_cmp(&(b.rmse_hz / b.fitted_tone_hz)));
    objectives
        .sort_by(|a, b| (a.rmse_hz / a.fitted_tone_hz).total_cmp(&(b.rmse_hz / b.fitted_tone_hz)));
    let objective = match_error(&objectives[..required]);
    // Keep a finite optimization objective outside the acceptance boundary.
    // Only models that explain curvature better than a free linear drift can
    // count toward the final independent-flight consensus.
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
    fn proposal_assembly_preserves_competing_flight_choices() {
        let event = |cpa_s| Event {
            start_s: cpa_s - 10.,
            end_s: cpa_s + 10.,
            cpa_s,
            base_hz: 100.,
            speed_mps: 70.,
            slant_m: 1000.,
            fit_rmse_hz: 0.,
            contrast_db: 20.,
            ridge: vec![],
            activity_span_s: None,
            alternatives: vec![],
        };
        // The first sound has two nearby flights. The second can only use 10.
        // Reserving 10 greedily for the first sound destroys a valid proposal.
        let events = vec![event(100.), event(120.), event(500.)];
        let assemble = |events: &[Event], alternative: bool| {
            complete_seed(
                demo::TRUTH,
                demo::EPOCH,
                2,
                30,
                0.,
                events,
                |time, event| {
                    if event.cpa_s == 100. {
                        let mut options = vec![(10, time, 1000., 70.)];
                        if alternative {
                            options.push((20, time, 1100., 70.));
                        }
                        options
                    } else {
                        vec![(10, time, 1000., 70.)]
                    }
                },
            )
        };
        let seed = assemble(&events, true).expect("Three independent assignments are possible");
        assert_eq!(seed.indices, vec![20, 10, 30]);
        assert!(seed.alternatives[0].contains(&10) && seed.alternatives[0].contains(&20));
        assert!(seed.alternatives[1].contains(&10));
        let mut reordered = events.clone();
        reordered.swap(0, 1);
        assert_eq!(
            assemble(&reordered, true).unwrap().indices,
            vec![10, 20, 30]
        );
        assert!(
            assemble(&events, false).is_none(),
            "One flight cannot count twice"
        );
    }

    fn perturbed_corroboration_fixture() -> (Point, Point, Vec<Event>, Vec<Track>) {
        let truth = demo::TRUTH;
        let shifted = Point {
            lat: truth.lat + 500. / 111320.,
            ..truth
        };
        let c = Job::default().sound_speed();
        let mut tracks = Vec::new();
        let mut events = Vec::new();
        for (id, cpa) in [400., 1400., 2400., 3400.].into_iter().enumerate() {
            let angle = if id < 3 {
                id as f64 * std::f64::consts::TAU / 3.
            } else {
                0.
            };
            let lateral = if id < 3 { 1500. } else { 50. };
            let height = if id < 3 { 8000. } else { 220. };
            let samples = (-100..=100)
                .map(|i| {
                    let along = i as f64 * 4. * 70.;
                    let east = along * angle.sin() + lateral * angle.cos();
                    let north = along * angle.cos() - lateral * angle.sin();
                    crate::model::Sample::new(
                        demo::EPOCH + cpa + i as f64 * 4.,
                        Point {
                            lat: truth.lat + north / 111320.,
                            lon: truth.lon + east / (111320. * truth.lat.to_radians().cos()),
                            alt: truth.alt + height,
                        },
                    )
                })
                .collect();
            let track = Track {
                id: format!("test-{id}"),
                samples,
                kind: crate::aircraft::AircraftKind::Unknown,
            };
            let observer = if id < 3 { shifted } else { truth };
            let (arrival, range, speed) = track.cpa(observer.ecef(), c).unwrap();
            let time = arrival - demo::EPOCH;
            let (extent, step) = if id < 3 { (300., 2.) } else { (30., 0.5) };
            let ridge = (0..=(2. * extent / step) as usize)
                .map(|i| {
                    let time_s = time - extent + i as f64 * step;
                    RidgePoint {
                        time_s,
                        hz: 150.
                            * track
                                .factor(observer.ecef(), demo::EPOCH + time_s, c)
                                .unwrap(),
                        contrast_db: 20.,
                    }
                })
                .collect();
            events.push(Event {
                start_s: time - extent,
                end_s: time + extent,
                cpa_s: time,
                base_hz: 150.,
                speed_mps: speed,
                slant_m: range,
                fit_rmse_hz: 0.,
                contrast_db: 20.,
                ridge,
                activity_span_s: None,
                alternatives: vec![],
            });
            tracks.push(track);
        }
        (truth, shifted, events, tracks)
    }

    #[test]
    fn complete_consensus_refinement_preserves_all_original_observations() {
        let (truth, _, events, tracks) = perturbed_corroboration_fixture();
        let seed = corroboration_seed(truth);
        let job = Job {
            bounds: Some(Bounds {
                south: truth.lat - 750. / 111320.,
                north: truth.lat + 750. / 111320.,
                west: truth.lon - 750. / (111320. * truth.lat.to_radians().cos()),
                east: truth.lon + 750. / (111320. * truth.lat.to_radians().cos()),
            }),
            start_utc: Some(demo::EPOCH),
            time_tolerance_s: 0.01,
            max_clock_drift_ppm: 0.,
            ground_m: truth.alt,
            grid_m: 200.,
            ..Default::default()
        };
        let (objective, matches) = exact_score(&seed, &events, &tracks, job.sound_speed());
        assert_eq!(matches.len(), 4);
        let original = CorroboratedFit {
            error: accepted_fit_error(&seed, &events, objective, &matches).unwrap(),
            seed,
            matches,
        };
        let refined = refine_complete_fit(
            &original,
            &job,
            &events,
            &tracks,
            job.bounds.unwrap(),
            &mut Terrain::new(None, truth.alt),
            &Control::default(),
        )
        .unwrap()
        .expect("The complete consensus has a lower-error nearby fit");
        assert!(refined.error < original.error - 1e-6);
        assert_eq!(refined.matches.len(), original.matches.len());
        for (old, new) in original.matches.iter().zip(&refined.matches) {
            assert_eq!(old.aircraft, new.aircraft);
            assert_eq!(old.event_index, new.event_index);
            assert_eq!(old.event_variant, new.event_variant);
            assert!(
                old.modeled_hz
                    .iter()
                    .zip(&new.modeled_hz)
                    .all(|(old, new)| *old == 0. || *new != 0.)
            );
            assert!(has_curvature(new));
            assert!(new.coverage >= 0.9);
            assert!(new.rmse_hz / new.fitted_tone_hz <= 0.015);
        }
        assert!(
            accepted_fit_error(&refined.seed, &events, refined.error, &refined.matches).is_some()
        );
        assert!(job.bounds.unwrap().contains(refined.seed.point));
        assert!(within_time(&job, refined.seed.start));
        assert_eq!(refined.seed.clock_rate, 1.);
        assert_eq!(original.seed.point.lat, truth.lat);

        let mut core_only = original;
        core_only.matches.truncate(3);
        assert!(
            refine_complete_fit(
                &core_only,
                &job,
                &events,
                &tracks,
                job.bounds.unwrap(),
                &mut Terrain::new(None, truth.alt),
                &Control::default(),
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn refinement_preserves_additional_supported_curves() {
        let (truth, shifted, events, tracks) = perturbed_corroboration_fixture();
        let c = Job::default().sound_speed();
        let mut seed = Seed {
            point: truth,
            start: demo::EPOCH,
            clock_rate: 1.,
            anchor: 0,
            indices: vec![0, 1, 2, 3],
            alternatives: Default::default(),
            score: 0.,
            coarse_profiled: false,
            corroboration: None,
        };
        let first = exact_score(&seed, &events, &tracks, c);
        seed.point = shifted;
        let second = exact_score(&seed, &events, &tracks, c);
        assert_eq!(first.1.len(), 4);
        assert_eq!(second.1.len(), 3);
        assert!(second.0 < first.0);
        assert!(match_error(&first.1) <= 1.);

        // Three distant curves have a small, coherent measurement perturbation.
        // A close fourth curve still supports the first position. Minimizing
        // only the best three can move 500 m and discard that extra evidence.
        let mut retained = None;
        seed.point = truth;
        retain_corroborated_fit(&mut retained, &seed, &events, first.0, first.1);
        seed.point = shifted;
        retain_corroborated_fit(&mut retained, &seed, &events, second.0, second.1);
        let retained = retained.unwrap();
        assert_eq!(retained.matches.len(), 4);
        assert!(retained.seed.point.distance(truth) < 1.);

        // An acceptable mean error must not hide a failing individual curve.
        let mut weak = retained.matches.clone();
        weak[3].rmse_hz = 0.016 * weak[3].fitted_tone_hz;
        assert!(match_error(&weak) < 1.);
        let mut invalid = None;
        retain_corroborated_fit(&mut invalid, &retained.seed, &events, first.0, weak);
        assert!(invalid.is_none());

        // Fix time only in this synthetic regression to isolate spatial
        // refinement; the real diagnostic also exercises an inferred clock.
        let job = Job {
            bounds: Some(Bounds {
                south: truth.lat,
                north: truth.lat + 750. / 111320.,
                west: truth.lon,
                east: truth.lon + 500. / (111320. * truth.lat.to_radians().cos()),
            }),
            start_utc: Some(demo::EPOCH),
            time_tolerance_s: 0.01,
            ground_m: truth.alt,
            grid_m: 250.,
            search_mode: SearchMode::RegionalGrid,
            max_clock_drift_ppm: 0.,
            silence_mode: SilenceMode::Off,
            ..Default::default()
        };
        let result = locate(
            &job,
            &events,
            &tracks,
            &mut Terrain::new(None, truth.alt),
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        let first = result
            .candidates
            .first()
            .expect("The corroborated location must survive refinement");
        assert_eq!(first.matches.len(), 4);
        assert!(
            Point {
                lat: first.latitude,
                lon: first.longitude,
                alt: first.ground_m
            }
            .distance(truth)
                < 250.
        );
    }

    fn corroboration_seed(point: Point) -> Seed {
        Seed {
            point,
            start: demo::EPOCH,
            clock_rate: 1.,
            anchor: 0,
            indices: vec![0, 1, 2, 3],
            alternatives: Default::default(),
            score: 0.,
            coarse_profiled: false,
            corroboration: None,
        }
    }

    #[test]
    fn coarse_and_final_beams_preserve_extra_verified_curves() {
        let (truth, shifted, events, tracks) = perturbed_corroboration_fixture();
        let job = Job {
            max_clock_drift_ppm: 0.,
            ..Default::default()
        };
        let mut seeds = vec![corroboration_seed(truth), corroboration_seed(shifted)];
        score_and_prune_seeds(&mut seeds, 1, &job, &events, &tracks, &Control::default()).unwrap();
        // The exact three-curve fit still keeps its original place. The
        // four-curve fit must independently survive the small resource limit.
        assert_eq!(seeds.len(), 2);
        assert!(seeds[0].point.distance(shifted) < 1.);
        assert!(seeds[0].corroboration.is_none());
        assert!(seeds[1].point.distance(truth) < 1.);
        assert_eq!(seeds[1].corroboration.unwrap().matches, 4);
        for seed in &mut seeds {
            seed.score = exact_score(seed, &events, &tracks, job.sound_speed()).0;
        }
        select_refinement_seeds(&mut seeds, 1, job.grid_m);
        assert_eq!(seeds.len(), 2);
        assert!(seeds.iter().any(|seed| seed.point.distance(truth) < 1.));
    }

    #[test]
    fn extra_beam_requires_full_curve_coverage_and_curvature() {
        let (truth, _, mut events, mut tracks) = perturbed_corroboration_fixture();
        let seed = corroboration_seed(truth);
        let c = Job::default().sound_speed();
        assert_eq!(
            full_corroboration(&seed, &events, &tracks, c)
                .unwrap()
                .matches,
            4
        );
        let original = events[3].clone();
        let pivot = original.cpa_s;
        for sample in &mut events[3].ridge {
            sample.hz = 150. - 0.5 * (sample.time_s - pivot);
        }
        assert!(full_corroboration(&seed, &events, &tracks, c).is_none());
        events[3] = original;
        tracks[3]
            .samples
            .retain(|sample| sample.time > demo::EPOCH + pivot);
        assert!(full_corroboration(&seed, &events, &tracks, c).is_none());
    }

    #[test]
    fn corroboration_beams_are_bounded_and_parallel_deterministic() {
        let (truth, _, events, tracks) = perturbed_corroboration_fixture();
        let job = Job {
            grid_m: 50.,
            max_clock_drift_ppm: 0.,
            ..Default::default()
        };
        let seeds: Vec<_> = (0..64)
            .map(|i| {
                corroboration_seed(Point {
                    lat: truth.lat + (i as f64 - 32.) * 20. / 111320.,
                    ..truth
                })
            })
            .collect();
        let mut serial = seeds.clone();
        let mut parallel = seeds.clone();
        let control = Control::default();
        score_and_prune_seeds(&mut serial, 4, &job, &events, &tracks, &control).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();
        pool.install(|| score_and_prune_seeds(&mut parallel, 4, &job, &events, &tracks, &control))
            .unwrap();
        let signature = |seeds: &[Seed]| {
            seeds
                .iter()
                .map(|seed| {
                    (
                        seed.point.lat.to_bits(),
                        seed.point.lon.to_bits(),
                        seed.start.to_bits(),
                        seed.clock_rate.to_bits(),
                        seed.score.to_bits(),
                        seed.indices.clone(),
                        seed.corroboration
                            .map(|support| (support.matches, support.error.to_bits())),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(serial.len(), 5);
        assert_eq!(signature(&serial), signature(&parallel));
        let before = signature(&serial);
        score_and_prune_seeds(&mut serial, 4, &job, &events, &tracks, &control).unwrap();
        assert_eq!(signature(&serial), before);

        // All original error-ranked proposals remain in the combined beam.
        let mut legacy = seeds;
        for seed in &mut legacy {
            profile_seed_clock(seed, &job, &events, &tracks);
        }
        prune_coarse_seeds(&mut legacy, 4, job.grid_m);
        for seed in legacy {
            assert!(serial.iter().any(|retained| retained.point.lat.to_bits()
                == seed.point.lat.to_bits()
                && retained.start.to_bits() == seed.start.to_bits()
                && retained.indices == seed.indices));
        }
        for seed in &mut serial {
            seed.score = exact_score(seed, &events, &tracks, job.sound_speed()).0;
        }
        select_refinement_seeds(&mut serial, 4, job.grid_m);
        assert!(serial.len() <= 5);
        assert!(serial.iter().any(|seed| seed.corroboration.is_some()));
    }

    #[test]
    fn alternative_associations_resolve_conflicts_before_rejecting_consensus() {
        // The cheapest edge consumes the only flight available to event 1.
        // Event 0 must use its second choice to retain three independent flights.
        let edges = [(0.01, 0, 10), (0.02, 1, 10), (0.03, 2, 30), (0.04, 0, 20)];
        let (score, selected) = independent_curve_assignment(&edges, 3, 3);
        assert!(score.is_finite(), "A valid three-flight assignment exists");
        assert_eq!(selected, vec![20, 10, 30]);
        assert!((score - (0.09_f64 / 3.).sqrt()).abs() < 1e-12);

        let mut expensive_fallback = edges.to_vec();
        expensive_fallback.push((0.8, 1, 40));
        assert_eq!(
            independent_curve_assignment(&expensive_fallback, 3, 3).1,
            selected,
            "A complete greedy assignment can still miss the least-error core"
        );

        let impossible = [(0.01, 0, 10), (0.02, 1, 10), (0.03, 2, 30)];
        assert!(
            !independent_curve_assignment(&impossible, 3, 3)
                .0
                .is_finite()
        );
    }

    #[test]
    fn independent_curve_assignment_matches_exhaustive_small_graphs() {
        let mut random = 829_u64;
        for _ in 0..64 {
            let mut edges = Vec::new();
            for event in 0..5 {
                for track in 0..5 {
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    if random >> 62 != 0 {
                        edges.push((((random >> 32) % 1000) as f64 / 1000., event, track));
                    }
                }
            }
            edges.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut expected = f64::INFINITY;
            for i in 0..edges.len() {
                for j in i + 1..edges.len() {
                    for k in j + 1..edges.len() {
                        let (a, b, c) = (edges[i], edges[j], edges[k]);
                        if a.1 != b.1
                            && a.1 != c.1
                            && b.1 != c.1
                            && a.2 != b.2
                            && a.2 != c.2
                            && b.2 != c.2
                        {
                            expected = expected.min((a.0 + b.0 + c.0) / 3.);
                        }
                    }
                }
            }
            let (actual, selected) = independent_curve_assignment(&edges, 5, 3);
            assert!((actual.powi(2) - expected).abs() < 1e-12);
            let assigned: Vec<_> = selected.iter().filter(|&&x| x != usize::MAX).collect();
            let unique: std::collections::HashSet<_> = assigned.iter().copied().collect();
            assert_eq!(assigned.len(), unique.len());
            let mut selected_costs: Vec<_> = selected
                .iter()
                .enumerate()
                .filter(|(_, track)| **track != usize::MAX)
                .map(|(event, &track)| {
                    edges
                        .iter()
                        .find(|e| e.1 == event && e.2 == track)
                        .unwrap()
                        .0
                })
                .collect();
            selected_costs.sort_by(f64::total_cmp);
            assert!((selected_costs[..3].iter().sum::<f64>() / 3. - expected).abs() < 1e-12);
        }
    }

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
            activity_span_s: None,
            alternatives: Vec::new(),
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
            anchor: 0,
            indices: vec![0],
            alternatives: Default::default(),
            score: 0.,
            coarse_profiled: false,
            corroboration: None,
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
                activity_span_s: None,
                alternatives: Vec::new(),
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
            anchor: 0,
            indices: vec![0, 1, 2, 3],
            alternatives: Default::default(),
            score: 0.,
            coarse_profiled: false,
            corroboration: None,
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
    #[test]
    fn coarse_beam_preserves_distinct_basins_before_refinement() {
        let base = Seed {
            point: demo::TRUTH,
            start: demo::EPOCH,
            clock_rate: 1.,
            anchor: 0,
            indices: vec![0, 1, 2],
            alternatives: Default::default(),
            score: 0.,
            coarse_profiled: false,
            corroboration: None,
        };
        let mut seeds: Vec<_> = (0..9000)
            .map(|i| {
                let mut s = base.clone();
                s.score = i as f64 * 0.0001;
                s
            })
            .collect();
        let mut other_flights = base.clone();
        other_flights.indices = vec![3, 4, 5];
        other_flights.score = 2.;
        let mut other_time = base.clone();
        other_time.start += 120.;
        other_time.score = 3.;
        let mut other_place = base.clone();
        other_place.point.lat += 0.02;
        other_place.score = 4.;
        seeds.extend([other_flights, other_time, other_place]);
        prune_coarse_seeds(&mut seeds, 4, 200.);
        assert_eq!(seeds.len(), 4);
        assert!(
            seeds.iter().any(|s| s.indices == [3, 4, 5]),
            "Repeated cheap hypotheses must not evict a different flight association"
        );
        assert!(seeds.iter().any(|s| (s.start - base.start).abs() > 100.));
        assert!(seeds.iter().any(|s| s.point.distance(base.point) > 1000.));
        assert_eq!(seeds[0].score, 0.);
    }
    #[test]
    fn observed_curves_rank_before_coarse_beam_truncation() {
        let tracks = demo::tracks();
        let ridge: Vec<_> = (0..120)
            .map(|i| {
                let time_s = 10. + i as f64 * 0.5;
                RidgePoint {
                    time_s,
                    hz: 100.
                        * tracks[0]
                            .factor(demo::TRUTH.ecef(), demo::EPOCH + time_s, 340.39)
                            .unwrap(),
                    contrast_db: 20.,
                }
            })
            .collect();
        let event = Event {
            activity_span_s: None,
            alternatives: Vec::new(),
            start_s: 10.,
            end_s: 69.5,
            cpa_s: 39.,
            base_hz: 100.,
            speed_mps: 72.,
            slant_m: 1300.,
            fit_rmse_hz: 0.,
            contrast_db: 20.,
            ridge,
        };
        let correct = Seed {
            point: demo::TRUTH,
            start: demo::EPOCH,
            clock_rate: 1.,
            anchor: 0,
            indices: vec![0],
            alternatives: Default::default(),
            score: 1.,
            coarse_profiled: false,
            corroboration: None,
        };
        let mut wrong = correct.clone();
        wrong.point.lat += 0.03;
        wrong.score = 0.;
        let mut seeds = vec![wrong, correct];
        score_and_prune_seeds(
            &mut seeds,
            1,
            &Job::default(),
            &[event],
            &tracks,
            &Control::default(),
        )
        .unwrap();
        assert!(
            seeds[0].point.distance(demo::TRUTH) < 1.,
            "A cheap CPA summary must not evict the proposal whose full curve fits"
        );
    }

    #[test]
    fn regional_search_keeps_the_curve_match_despite_biased_summary_features() {
        let real = demo::tracks().remove(0);
        let c = Job::default().sound_speed();
        let (arrival, range, speed) = real.cpa(demo::TRUTH.ecef(), c).unwrap();
        let event = Event {
            activity_span_s: None,
            alternatives: Vec::new(),
            start_s: 10.,
            end_s: 79.5,
            cpa_s: arrival - demo::EPOCH,
            base_hz: 100.,
            speed_mps: speed * 1.3,
            slant_m: range * 1.3,
            fit_rmse_hz: 0.,
            contrast_db: 20.,
            ridge: (0..140)
                .map(|i| {
                    let time_s = 10. + i as f64 * 0.5;
                    RidgePoint {
                        time_s,
                        hz: 100.
                            * real
                                .factor(demo::TRUTH.ecef(), demo::EPOCH + time_s, c)
                                .unwrap(),
                        contrast_db: 20.,
                    }
                })
                .collect(),
        };
        let mut tracks = vec![real.clone()];
        for n in 1..=5 {
            let mut decoy = real.clone();
            decoy.id = format!("DECOY{n}");
            for sample in &mut decoy.samples {
                let p = sample.point;
                *sample = crate::model::Sample::new(
                    sample.time + n as f64 * 600.,
                    Point {
                        lat: demo::TRUTH.lat + 1.3 * (p.lat - demo::TRUTH.lat),
                        lon: demo::TRUTH.lon + 1.3 * (p.lon - demo::TRUTH.lon),
                        alt: demo::TRUTH.alt + 1.3 * (p.alt - demo::TRUTH.alt),
                    },
                );
            }
            tracks.push(decoy);
        }
        let job = Job {
            bounds: Some(Bounds {
                south: demo::TRUTH.lat,
                north: demo::TRUTH.lat + 1e-8,
                west: demo::TRUTH.lon,
                east: demo::TRUTH.lon + 1e-8,
            }),
            ground_m: demo::TRUTH.alt,
            search_mode: SearchMode::RegionalGrid,
            silence_mode: SilenceMode::Off,
            ..Default::default()
        };
        let result = locate(
            &job,
            &[event],
            &tracks,
            &mut Terrain::new(None, job.ground_m),
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(
            result
                .candidates
                .first()
                .map(|c| c.matches[0].aircraft.as_str()),
            Some(real.id.as_str()),
            "The measured curve must survive regional pruning even when CPA summary features favor several other flights"
        );
    }

    #[test]
    fn indexed_search_retains_ranges_allowed_by_its_feature_gate() {
        let tracks = demo::tracks();
        let c = Job::default().sound_speed();
        let events: Vec<_> = tracks
            .iter()
            .enumerate()
            .map(|(n, track)| {
                let (arrival, range, speed) = track.cpa(demo::TRUTH.ecef(), c).unwrap();
                let begin = if n == 0 { 10. } else { 135. };
                let event = Event {
                    activity_span_s: None,
                    alternatives: Vec::new(),
                    start_s: begin,
                    end_s: begin + 69.5,
                    cpa_s: arrival - demo::EPOCH,
                    base_hz: 100.,
                    speed_mps: speed,
                    slant_m: range * 0.2,
                    fit_rmse_hz: 0.,
                    contrast_db: 20.,
                    ridge: (0..140)
                        .map(|i| {
                            let time_s = begin + i as f64 * 0.5;
                            RidgePoint {
                                time_s,
                                hz: 100.
                                    * track
                                        .factor(demo::TRUTH.ecef(), demo::EPOCH + time_s, c)
                                        .unwrap(),
                                contrast_db: 20.,
                            }
                        })
                        .collect(),
                };
                assert!(feature_error(&event, range, speed) < 12.);
                event
            })
            .collect();
        let job = Job {
            bounds: Some(Bounds {
                south: 34.97,
                north: 35.03,
                west: -110.04,
                east: -109.96,
            }),
            ground_m: demo::TRUTH.alt,
            grid_m: 200.,
            search_mode: SearchMode::FlightCorridors,
            silence_mode: SilenceMode::Off,
            max_hypotheses: 1_000_000,
            ..Default::default()
        };
        let result = locate(
            &job,
            &events,
            &tracks,
            &mut Terrain::new(None, job.ground_m),
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        assert!(
            result.candidates.iter().any(|candidate| Point {
                lat: candidate.latitude,
                lon: candidate.longitude,
                alt: candidate.ground_m
            }
            .distance(demo::TRUTH)
                < 250.),
            "A permitted range error must not eliminate every observer proposal before curve fitting"
        );
    }

    #[test]
    fn curve_scoring_can_replace_a_misleading_flight_association() {
        let template = demo::tracks().remove(0);
        let job = Job {
            max_clock_drift_ppm: 0.,
            ..Default::default()
        };
        let c = job.sound_speed();
        let mut tracks = Vec::new();
        for n in 0..3 {
            let mut track = template.clone();
            track.id = format!("REAL{n}");
            for p in &mut track.samples {
                p.time += n as f64 * 600.;
            }
            tracks.push(track);
        }
        let events: Vec<_> = tracks
            .iter()
            .enumerate()
            .map(|(n, track)| {
                let (arrival, range, speed) = track.cpa(demo::TRUTH.ecef(), c).unwrap();
                Event {
                    activity_span_s: None,
                    alternatives: Vec::new(),
                    start_s: 10. + n as f64 * 600.,
                    end_s: 79.5 + n as f64 * 600.,
                    cpa_s: arrival - demo::EPOCH,
                    base_hz: 100.,
                    speed_mps: speed * 1.3,
                    slant_m: range * 1.3,
                    fit_rmse_hz: 0.,
                    contrast_db: 20.,
                    ridge: (0..140)
                        .map(|i| {
                            let time_s = 10. + n as f64 * 600. + i as f64 * 0.5;
                            RidgePoint {
                                time_s,
                                hz: 100.
                                    * track
                                        .factor(demo::TRUTH.ecef(), demo::EPOCH + time_s, c)
                                        .unwrap(),
                                contrast_db: 20.,
                            }
                        })
                        .collect(),
                }
            })
            .collect();
        for n in 0..3 {
            let mut decoy = tracks[n].clone();
            decoy.id = format!("WRONG{n}");
            for sample in &mut decoy.samples {
                let p = sample.point;
                *sample = crate::model::Sample::new(
                    sample.time,
                    Point {
                        lat: demo::TRUTH.lat + 1.3 * (p.lat - demo::TRUTH.lat),
                        lon: demo::TRUTH.lon + 1.3 * (p.lon - demo::TRUTH.lon),
                        alt: demo::TRUTH.alt + 1.3 * (p.alt - demo::TRUTH.alt),
                    },
                );
            }
            tracks.push(decoy);
        }
        let mut seed = complete_seed(demo::TRUTH, demo::EPOCH, 0, 0, 0., &events, |_, _| {
            tracks
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.cpa(demo::TRUTH.ecef(), c).map(|(a, r, v)| (i, a, r, v)))
                .collect()
        })
        .unwrap();
        assert_ne!(
            seed.indices,
            vec![0, 1, 2],
            "The fixture must initially prefer misleading CPA summaries"
        );
        profile_seed_clock(&mut seed, &job, &events, &tracks);
        assert_eq!(
            seed.indices,
            vec![0, 1, 2],
            "Measured curves must select the actual flights among similar summaries"
        );
        assert!(exact_score(&seed, &events, &tracks, c).0 < 0.001);
        // The best standalone summary can describe a different tone or short
        // fragment. Retained observations must participate in proposal lookup,
        // shared-clock scoring, and exact matching without adding extra votes.
        let variants: Vec<_> = events
            .iter()
            .map(|correct| {
                let mut misleading = correct.clone();
                misleading.speed_mps = 250.;
                misleading.slant_m = 20000.;
                for p in &mut misleading.ridge {
                    p.hz = 100.;
                }
                misleading.alternatives = vec![correct.clone()];
                misleading
            })
            .collect();
        let mut alternative_seed =
            complete_seed(demo::TRUTH, demo::EPOCH, 0, 0, 0., &variants, |_, event| {
                if event.slant_m >= 19000. {
                    return vec![];
                }
                tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| t.cpa(demo::TRUTH.ecef(), c).map(|(a, r, v)| (i, a, r, v)))
                    .collect()
            })
            .expect("Alternate observations must be considered before rejecting a proposal");
        profile_seed_clock(&mut alternative_seed, &job, &variants, &tracks);
        let (_, alternative_matches) = exact_score(&alternative_seed, &variants, &tracks, c);
        assert_eq!(alternative_matches.len(), 3);
        assert!(
            alternative_matches
                .iter()
                .all(|m| m.event_variant == 1 && m.rmse_hz < 0.001)
        );
        for m in &alternative_matches {
            assert_eq!(
                m.modeled_hz.len(),
                variants[m.event_index.unwrap()]
                    .variant(m.event_variant)
                    .unwrap()
                    .ridge
                    .len()
            );
        }
        let mut incomplete = variants.clone();
        incomplete[2].alternatives.clear();
        assert!(
            exact_score(&alternative_seed, &incomplete, &tracks, c)
                .1
                .is_empty(),
            "Several observations of two sounds cannot supply a third flight"
        );
        // Alternative associations must not count one flight three times.
        seed.indices = vec![0, usize::MAX, usize::MAX];
        seed.alternatives = Arc::new(vec![[0, usize::MAX, usize::MAX]; 3]);
        profile_seed_clock(&mut seed, &job, &vec![events[0].clone(); 3], &tracks);
        assert!(!seed.score.is_finite());
        assert!(
            exact_score(&seed, &vec![events[0].clone(); 3], &tracks, c)
                .1
                .is_empty()
        );
    }
    #[test]
    fn clock_profile_does_not_commit_to_the_best_coarse_basin() {
        let job = Job::default();
        let c = job.sound_speed();
        let rate = 1.00207;
        let decoy_rate = 1.00375;
        let mut tracks = vec![];
        let mut events = vec![];
        for (i, passage) in [700., 1800., 3300.].into_iter().enumerate() {
            let track = Track {
                id: format!("real-{i}"),
                kind: crate::aircraft::AircraftKind::Unknown,
                samples: (-50..=50)
                    .map(|offset| {
                        crate::model::Sample::new(
                            demo::EPOCH + passage + offset as f64,
                            Point {
                                lat: demo::TRUTH.lat + offset as f64 * 70. / 111320.,
                                lon: demo::TRUTH.lon
                                    + 50. / (111320. * demo::TRUTH.lat.to_radians().cos()),
                                alt: demo::TRUTH.alt + 220.,
                            },
                        )
                    })
                    .collect(),
            };
            let (arrival, range, speed) = track.cpa(demo::TRUTH.ecef(), c).unwrap();
            let center = (arrival - demo::EPOCH) / rate;
            events.push(Event {
                start_s: center - 12.,
                end_s: center + 12.,
                cpa_s: center,
                base_hz: 150.,
                speed_mps: speed,
                slant_m: range,
                fit_rmse_hz: 0.,
                contrast_db: 20.,
                ridge: (0..=96)
                    .map(|k| {
                        let time_s = center - 12. + k as f64 * 0.25;
                        RidgePoint {
                            time_s,
                            hz: 150.
                                * track
                                    .factor(demo::TRUTH.ecef(), demo::EPOCH + rate * time_s, c)
                                    .unwrap(),
                            contrast_db: 20.,
                        }
                    })
                    .collect(),
                activity_span_s: None,
                alternatives: vec![],
            });
            tracks.push(track);
        }
        let pivot = events[2].cpa_s;
        for i in 0..3 {
            let mut decoy = tracks[i].clone();
            decoy.id = format!("decoy-{i}");
            let shift =
                (decoy_rate - rate) * (events[i].cpa_s - pivot) - 0.08 * events[i].slant_m / c;
            for sample in &mut decoy.samples {
                let p = sample.point;
                *sample = crate::model::Sample::new(
                    sample.time + shift,
                    Point {
                        lat: p.lat,
                        lon: demo::TRUTH.lon + 1.08 * (p.lon - demo::TRUTH.lon),
                        alt: demo::TRUTH.alt + 1.08 * (p.alt - demo::TRUTH.alt),
                    },
                );
            }
            tracks.push(decoy);
        }
        let mut seed = Seed {
            point: demo::TRUTH,
            start: demo::EPOCH + (rate - 1.) * pivot,
            clock_rate: 1.,
            anchor: 2,
            indices: vec![0, 1, 2],
            alternatives: Arc::new(vec![
                [0, 3, usize::MAX],
                [1, 4, usize::MAX],
                [2, 5, usize::MAX],
            ]),
            score: 0.,
            coarse_profiled: false,
            corroboration: None,
        };
        profile_seed_clock(&mut seed, &job, &events, &tracks);
        assert!(
            (seed.clock_rate - rate).abs() < 0.00005,
            "Coarse clock winner hid a better basin: rate {}, error {}",
            seed.clock_rate,
            seed.score
        );
        assert!(seed.score < 0.05);
        assert_eq!(seed.indices, vec![0, 1, 2]);
    }

    #[test]
    fn coarse_scoring_profiles_the_same_shared_clock_as_refinement() {
        let rate = 1.004;
        let mut events = vec![];
        let mut tracks = vec![];
        for shift in [0., 1000., 5000.] {
            let mut track = demo::tracks().remove(0);
            for p in &mut track.samples {
                p.time += shift;
            }
            let ridge = (0..120)
                .map(|i| {
                    let time_s = (10. + i as f64 * 0.5 + shift) / rate;
                    RidgePoint {
                        time_s,
                        hz: 100.
                            * track
                                .factor(demo::TRUTH.ecef(), demo::EPOCH + rate * time_s, 340.39)
                                .unwrap(),
                        contrast_db: 20.,
                    }
                })
                .collect();
            events.push(Event {
                activity_span_s: None,
                alternatives: Vec::new(),
                start_s: (10. + shift) / rate,
                end_s: (69.5 + shift) / rate,
                cpa_s: (39. + shift) / rate,
                base_hz: 100.,
                speed_mps: 72.,
                slant_m: 1300.,
                fit_rmse_hz: 0.,
                contrast_db: 20.,
                ridge,
            });
            tracks.push(track);
        }
        let seed = Seed {
            point: demo::TRUTH,
            start: demo::EPOCH + (rate - 1.) * events[2].cpa_s,
            clock_rate: 1.,
            anchor: 2,
            indices: vec![0, 1, 2],
            alternatives: Default::default(),
            score: 0.,
            coarse_profiled: false,
            corroboration: None,
        };
        assert!(exact_score(&seed, &events, &tracks, 340.39).0 > 1.);
        let mut seeds = vec![seed.clone()];
        score_and_prune_seeds(
            &mut seeds,
            1,
            &Job::default(),
            &events,
            &tracks,
            &Control::default(),
        )
        .unwrap();
        assert!(
            (seeds[0].clock_rate - rate).abs() < 0.00005,
            "Early pruning must not assume a fixed rate for a clock-drift job"
        );
        assert!(exact_score(&seeds[0], &events, &tracks, 340.39).0 < 0.05);
        let mut serial: Vec<_> = (0..128)
            .map(|i| {
                let mut trial = seed.clone();
                trial.point.lat += (i % 8) as f64 * 0.0002;
                trial.point.lon += (i / 8) as f64 * 0.0002;
                trial.start += (i % 5) as f64 - 2.;
                trial
            })
            .collect();
        let mut parallel = serial.clone();
        let default_job = Job::default();
        let control = Control::default();
        score_and_prune_seeds(&mut serial, 32, &default_job, &events, &tracks, &control).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();
        pool.install(|| {
            score_and_prune_seeds(&mut parallel, 32, &default_job, &events, &tracks, &control)
        })
        .unwrap();
        assert_eq!(serial.len(), parallel.len());
        for (left, right) in serial.iter().zip(&parallel) {
            assert_eq!(left.indices, right.indices);
            assert_eq!(left.start.to_bits(), right.start.to_bits());
            assert_eq!(left.clock_rate.to_bits(), right.clock_rate.to_bits());
            assert_eq!(left.score.to_bits(), right.score.to_bits());
            assert_eq!(left.point.lat.to_bits(), right.point.lat.to_bits());
            assert_eq!(left.point.lon.to_bits(), right.point.lon.to_bits());
        }
        // Merging another batch must not shift already-ranked proposals again.
        let once = serial.clone();
        score_and_prune_seeds(&mut serial, 32, &default_job, &events, &tracks, &control).unwrap();
        for (before, after) in once.iter().zip(&serial) {
            assert_eq!(before.indices, after.indices);
            assert_eq!(before.start.to_bits(), after.start.to_bits());
            assert_eq!(before.clock_rate.to_bits(), after.clock_rate.to_bits());
            assert_eq!(before.score.to_bits(), after.score.to_bits());
        }
        let job = Job {
            max_clock_drift_ppm: 0.,
            ..Default::default()
        };
        let mut fixed = vec![seed];
        score_and_prune_seeds(&mut fixed, 1, &job, &events, &tracks, &Control::default()).unwrap();
        assert_eq!(fixed[0].clock_rate, 1.);
    }
}
