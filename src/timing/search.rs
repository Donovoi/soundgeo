use super::{
    assignment::assign,
    likelihood,
    reference::{Passage, Reference},
};
use crate::{
    Control, Job, Progress,
    geo::{Point, Terrain},
    model::Track,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub grid_m: f64,
    pub maximum_grid_points: usize,
    pub maximum_passage_distance_m: f64,
    pub retained_candidates: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            grid_m: 300.,
            maximum_grid_points: 50000,
            maximum_passage_distance_m: 500.,
            retained_candidates: 256,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pair {
    pub peak: usize,
    pub track: String,
    pub residual_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_m: f64,
    pub offset_utc: f64,
    pub log_likelihood_ratio: f64,
    pub reference_passages: usize,
    pub reference_exposure: f64,
    pub background_expected_events: f64,
    pub detection_probability: f64,
    pub pairs: Vec<Pair>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub options: Options,
    pub points: usize,
    pub eligible_points: usize,
    pub likelihood_evaluations: usize,
    pub candidates: Vec<Candidate>,
    /// Best evaluated score at each searched location, before candidate pruning.
    /// Retain these to avoid hiding spatial alternatives behind a full beam.
    #[serde(default)]
    pub location_scores: Vec<LocationScore>,
    #[serde(default)]
    pub terrain_fallback_points: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocationScore {
    pub latitude: f64,
    pub longitude: f64,
    pub log_likelihood_ratio: f64,
}

/// Search the configured region and reference days with one shared unknown clock.
/// This score ranks hypotheses; it is not a location confidence or error radius.
#[allow(clippy::too_many_arguments)]
pub fn locate(
    job: &Job,
    peaks: &[f64],
    duration: f64,
    tracks: &[Track],
    options: &Options,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<SearchResult> {
    control.check()?;
    let bounds = job.bounds.context("Passage timing needs search bounds")?;
    bounds.validate()?;
    ensure!(
        duration.is_finite() && duration > 0. && duration <= 86400.,
        "Timing recordings must be between zero and 24 hours long"
    );
    ensure!(
        peaks.len() <= 10000
            && peaks
                .iter()
                .all(|p| p.is_finite() && *p >= 0. && *p <= duration)
            && peaks.windows(2).all(|p| p[0] < p[1]),
        "Timing peaks must be finite, ordered, unique and inside the recording"
    );
    ensure!(
        options.grid_m.is_finite()
            && (50. ..=1000.).contains(&options.grid_m)
            && (100..=50000).contains(&options.maximum_grid_points)
            && (1..=1024).contains(&options.retained_candidates)
            && options.maximum_passage_distance_m.is_finite()
            && (100. ..=5000.).contains(&options.maximum_passage_distance_m),
        "Invalid passage-timing search options"
    );
    let (points, spacing) = bounds.grid(options.grid_m, options.maximum_grid_points);
    ensure!(
        spacing <= options.grid_m * (1. + 1e-12),
        "Timing grid exceeds its point budget; use a smaller region or raise the point limit. The grid was not silently coarsened."
    );
    let mut result = SearchResult {
        options: options.clone(),
        points: points.len(),
        eligible_points: 0,
        likelihood_evaluations: 0,
        candidates: vec![],
        location_scores: vec![],
        terrain_fallback_points: 0,
    };
    if peaks.len() < 3 {
        return Ok(result);
    }
    let reference = Reference::new(
        tracks,
        options.maximum_passage_distance_m,
        job.sound_speed(),
        control,
    )?;
    ensure!(
        reference.end_day - reference.start_day <= 32. * 86400.,
        "Timing search supports at most 32 reference days per run"
    );
    let mut terrain = Terrain::with_datum(job.terrain.clone(), job.ground_m, job.ground_datum);
    for (index, mut point) in points.into_iter().enumerate() {
        control.check()?;
        point.alt = terrain.elevation(point);
        let passages = reference.passages(point.ecef());
        if passages.len() >= 10 {
            result.eligible_points += 1;
            let proposals = propose(
                peaks,
                &passages,
                duration,
                reference.start_day,
                reference.end_day,
                job,
                control,
            )?;
            let mut location_best: Option<f64> = None;
            for offset in proposals {
                control.check()?;
                if let Some(candidate) = evaluate(
                    point,
                    peaks,
                    &passages,
                    tracks,
                    offset,
                    duration,
                    (reference.start_day, reference.end_day),
                    job,
                ) {
                    result.likelihood_evaluations += 1;
                    location_best = Some(
                        location_best.map_or(candidate.log_likelihood_ratio, |best| {
                            best.max(candidate.log_likelihood_ratio)
                        }),
                    );
                    result.candidates.push(candidate);
                }
            }
            if let Some(score) = location_best {
                result.location_scores.push(LocationScore {
                    latitude: point.lat,
                    longitude: point.lon,
                    log_likelihood_ratio: score,
                });
            }
            if result.candidates.len() > options.retained_candidates {
                sort(&mut result.candidates);
                result.candidates.truncate(options.retained_candidates);
            }
        }
        if index.is_multiple_of(256) || index + 1 == result.points {
            progress(
                &format!(
                    "Passage timing: {}/{} points, {} eligible, {} likelihood evaluations",
                    index + 1,
                    result.points,
                    result.eligible_points,
                    result.likelihood_evaluations
                ),
                index as f32 / result.points as f32,
            );
        }
    }
    result.terrain_fallback_points = terrain.missing;
    sort(&mut result.candidates);
    Ok(result)
}
fn sort(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        b.log_likelihood_ratio
            .total_cmp(&a.log_likelihood_ratio)
            .then(a.latitude.total_cmp(&b.latitude))
            .then(a.longitude.total_cmp(&b.longitude))
            .then(a.offset_utc.total_cmp(&b.offset_utc))
    });
}
fn allowed(offset: f64, duration: f64, start: f64, end: f64, job: &Job) -> bool {
    offset >= start
        && offset + duration <= end
        && job
            .start_utc
            .is_none_or(|t| (t - offset).abs() <= job.time_tolerance_s)
}
fn propose(
    peaks: &[f64],
    reference: &[Passage],
    duration: f64,
    start: f64,
    end: f64,
    job: &Job,
    control: &Control,
) -> Result<Vec<f64>> {
    let first = ((reference[0].time - peaks[peaks.len() - 1]) * 2.).round_ties_even() as i64;
    let last = ((reference[reference.len() - 1].time - peaks[0]) * 2.).round_ties_even() as i64;
    let length = (last - first + 1) as usize;
    ensure!(
        length <= 33 * 172800,
        "Timing histogram exceeds the supported reference span"
    );
    let mut histogram = vec![0u32; length];
    for (i, r) in reference.iter().enumerate() {
        if i.is_multiple_of(128) {
            control.check()?;
        }
        for peak in peaks {
            let index = ((r.time - peak) * 2.).round_ties_even() as i64 - first;
            histogram[index as usize] += 1;
        }
    }
    let kernel: Vec<_> = (-6..=6)
        .map(|i| (-0.5 * (i as f64 * 0.5).powi(2)).exp())
        .collect();
    let mean: f64 = kernel.iter().sum::<f64>() * 0.5;
    let variance: f64 = kernel.iter().map(|k| k * k).sum::<f64>() * 0.5;
    let mut raw = vec![0.; length];
    for (i, &count) in histogram
        .iter()
        .enumerate()
        .filter(|(_, count)| **count != 0)
    {
        for (k, weight) in kernel.iter().enumerate() {
            let to = i as isize + k as isize - 6;
            if to >= 0 && (to as usize) < length {
                raw[to as usize] += count as f64 * weight;
            }
        }
    }
    let mut standardized = vec![f64::NEG_INFINITY; length];
    let (mut a, mut b) = (0, 0);
    for i in 0..length {
        if i.is_multiple_of(16384) {
            control.check()?;
        }
        let offset = (i as i64 + first) as f64 * 0.5;
        while a < reference.len() && reference[a].time < offset {
            a += 1;
        }
        while b < reference.len() && reference[b].time <= offset + duration {
            b += 1;
        }
        if b - a < 3 || !allowed(offset, duration, start, end, job) {
            raw[i] = f64::NEG_INFINITY;
            continue;
        }
        let expected = peaks.len() as f64 * (b - a) as f64 / duration;
        standardized[i] = (raw[i] - expected * mean) / (expected * variance).max(0.01).sqrt();
    }
    let mut proposals: Vec<f64> = vec![];
    for mut criterion in [raw, standardized] {
        for _ in 0..4 {
            let mut index = 0;
            for i in 1..length {
                if criterion[i] > criterion[index] {
                    index = i;
                }
            }
            if !criterion[index].is_finite() {
                break;
            }
            let offset = (index as i64 + first) as f64 * 0.5;
            if proposals.iter().all(|old| (offset - old).abs() > 7.5) {
                proposals.push(offset);
            }
            criterion[index.saturating_sub(30)..(index + 31).min(length)].fill(f64::NEG_INFINITY);
        }
    }
    Ok(proposals)
}

#[allow(clippy::too_many_arguments)]
fn evaluate(
    point: Point,
    peaks: &[f64],
    reference: &[Passage],
    tracks: &[Track],
    mut offset: f64,
    duration: f64,
    days: (f64, f64),
    job: &Job,
) -> Option<Candidate> {
    let mut pairs = vec![];
    for _ in 0..3 {
        pairs = assign(peaks, reference, offset);
        if pairs.is_empty() {
            break;
        }
        let mut offsets: Vec<_> = pairs
            .iter()
            .map(|&(i, j)| reference[j].time - peaks[i])
            .collect();
        offsets.sort_by(f64::total_cmp);
        offset = (offsets[(offsets.len() - 1) / 2] + offsets[offsets.len() / 2]) * 0.5;
    }
    if !allowed(offset, duration, days.0, days.1, job) {
        return None;
    }
    let times: Vec<_> = reference
        .iter()
        .map(|r| r.time - offset)
        .filter(|t| *t >= -8. && *t <= duration + 8.)
        .collect();
    let density: Vec<_> = peaks
        .iter()
        .map(|p| {
            times
                .iter()
                .map(|t| (-0.5 * (p - t).powi(2)).exp() / (2. * std::f64::consts::PI).sqrt())
                .sum()
        })
        .collect();
    let exposure: f64 = times
        .iter()
        .map(|t| normal_cdf(duration - t) - normal_cdf(-t))
        .sum();
    let fit = likelihood::fit(&density, exposure, duration);
    Some(Candidate {
        latitude: point.lat,
        longitude: point.lon,
        elevation_m: point.alt,
        offset_utc: offset,
        log_likelihood_ratio: fit.score,
        reference_passages: times.len(),
        reference_exposure: exposure,
        background_expected_events: fit.background_events,
        detection_probability: fit.detection_probability,
        pairs: pairs
            .iter()
            .map(|&(i, j)| Pair {
                peak: i,
                track: tracks[reference[j].track].id.clone(),
                residual_s: reference[j].time - peaks[i] - offset,
            })
            .collect(),
    })
}

fn normal_cdf(x: f64) -> f64 {
    if x <= -8. {
        return 0.;
    }
    if x >= 8. {
        return 1.;
    }
    // Positive-term integral series avoids cancellation inside the sum.
    let mut term = x.abs();
    let mut sum = term;
    for k in 1..200 {
        term *= x * x / (2 * k + 1) as f64;
        sum += term;
        if term <= sum * 1e-16 {
            break;
        }
    }
    0.5 + x.signum() * (-0.5 * x * x).exp() * sum / (2. * std::f64::consts::PI).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broad_search_recovers_generated_location_and_unknown_clock() {
        use crate::{
            aircraft::AircraftKind,
            geo::{Bounds, Vec3},
            model::Sample,
        };
        let truth = Point {
            lat: 41.,
            lon: -87.,
            alt: 0.,
        };
        let origin = truth.ecef();
        let (phi, lambda) = (truth.lat.to_radians(), truth.lon.to_radians());
        let east = Vec3(-lambda.sin(), lambda.cos(), 0.);
        let north = Vec3(
            -phi.sin() * lambda.cos(),
            -phi.sin() * lambda.sin(),
            phi.cos(),
        );
        let up = Vec3(
            phi.cos() * lambda.cos(),
            phi.cos() * lambda.sin(),
            phi.sin(),
        );
        let job = Job {
            bounds: Some(Bounds {
                south: 40.995,
                north: 41.005,
                west: -87.006,
                east: -86.994,
            }),
            ..Default::default()
        };
        let offset = 192800.;
        let emissions = [
            40., 99., 181., 262., 393., 471., 592., 611., 722., 831., 932., 1044.,
        ];
        let peaks: Vec<_> = emissions
            .iter()
            .map(|t| t + 150. / job.sound_speed())
            .collect();
        let tracks: Vec<_> = emissions
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let angle = i as f64 * 0.7;
                let heading = east * angle.cos() + north * angle.sin();
                let samples = [-20., 0., 20.]
                    .iter()
                    .map(|u| {
                        let mut sample = Sample::new(offset + t + u, truth);
                        sample.xyz = origin + up * 150. + heading * (u * 50.);
                        sample
                    })
                    .collect();
                Track {
                    id: format!("generated-{i}"),
                    samples,
                    kind: AircraftKind::Unknown,
                }
            })
            .collect();
        let result = locate(
            &job,
            &peaks,
            1200.,
            &tracks,
            &Options {
                retained_candidates: 1,
                ..Default::default()
            },
            &Control::default(),
            &|_, _| {},
        )
        .unwrap();
        let best = &result.candidates[0];
        assert_eq!(result.candidates.len(), 1);
        assert!(result.location_scores.len() > 1);
        assert!(result.location_scores.iter().any(|s| truth.distance(Point {
            lat: s.latitude,
            lon: s.longitude,
            alt: 0.
        }) > 300.));
        assert!((best.latitude - truth.lat).abs() < 1e-10);
        assert!((best.longitude - truth.lon).abs() < 1e-10);
        assert!((best.offset_utc - offset).abs() < 1e-6);
        assert_eq!(best.pairs.len(), emissions.len());
        // Twelve isolated, exactly aligned Gaussian peaks have p=1 and zero
        // background. Compare with the closed-form likelihood, not a cutoff.
        let expected = 12. * (1200. / (12. * (2. * std::f64::consts::PI).sqrt())).ln();
        assert!((best.log_likelihood_ratio - expected).abs() < 1e-6);
    }
    #[test]
    fn truncated_gaussian_exposure_matches_reference_values() {
        for (x, expected) in [
            (0., 0.5),
            (0.5, 0.6914624612740131),
            (1., 0.8413447460685429),
            (2., 0.9772498680518208),
            (3., 0.9986501019683699),
            (5., 0.9999997133484281),
            (7., 0.9999999999987201),
        ] {
            assert!((normal_cdf(x) - expected).abs() < 2e-15);
            assert!((normal_cdf(-x) - (1. - expected)).abs() < 2e-15);
        }
    }
    #[test]
    fn invalid_inputs_and_grid_budget_fail_before_work() {
        let job = Job {
            bounds: Some(crate::geo::Bounds {
                south: 41.65,
                north: 42.01,
                west: -87.98,
                east: -87.5,
            }),
            ..Default::default()
        };
        let options = Options {
            maximum_grid_points: 100,
            ..Default::default()
        };
        assert!(
            locate(
                &job,
                &[1., 2., 3.],
                60.,
                &[],
                &options,
                &Control::default(),
                &|_, _| {}
            )
            .unwrap_err()
            .to_string()
            .contains("budget")
        );
        assert!(
            locate(
                &job,
                &[2., 1., 3.],
                60.,
                &[],
                &Options::default(),
                &Control::default(),
                &|_, _| {}
            )
            .is_err()
        );
    }
}
