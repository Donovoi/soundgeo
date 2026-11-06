//! Conservative development screens. None is a calibrated location probability.
use super::{LocationScore, Options, Peak, SearchResult, search::PreparedSearch};
use crate::{Control, Job, Progress, geo::Point, model::Track};
use anyhow::Result;
use serde::{Deserialize, Serialize};

const CONTROLS: usize = 19;
const MIN_MATCHES: usize = 6;
const REGION_M: f64 = 1000.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    InsufficientEvidence,
    Ambiguous,
    TimingCandidate,
}
/// An enclosing region for evaluated competitive grid points, not an accuracy bound.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocationRegion {
    pub latitude: f64,
    pub longitude: f64,
    pub enclosing_radius_m: f64,
    pub competitive_points: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assessment {
    pub peaks: Vec<Peak>,
    pub search: Option<SearchResult>,
    pub control_maximum_scores: Vec<f64>,
    pub control_likelihood_evaluations: usize,
    pub control_permutation_seeds: Vec<u64>,
    pub screen: Screen,
    pub explanation: String,
    pub score_gap_beyond_1km: Option<f64>,
    pub competitive_location_spread_m: Option<f64>,
    pub minimum_matches: usize,
    pub required_controls: usize,
    pub calibrated_location_probability: bool,
    /// Present only after the signal and enclosing-region checks pass.
    /// Older saved reports do not contain this field.
    #[serde(default)]
    pub region: Option<LocationRegion>,
}
impl Assessment {
    pub fn pending(peaks: Vec<Peak>) -> Self {
        Self {peaks,search:None,control_maximum_scores:vec![],control_likelihood_evaluations:0,control_permutation_seeds:vec![],screen:Screen::InsufficientEvidence,
            explanation:"Passage timing needs at least six sound peaks and eight minutes of continuous audio.".into(),
            score_gap_beyond_1km:None,competitive_location_spread_m:None,minimum_matches:MIN_MATCHES,
            required_controls:CONTROLS,calibrated_location_probability:false,region:None}
    }
    pub fn candidate_count(&self) -> usize {
        if self.screen == Screen::InsufficientEvidence {
            0
        } else {
            self.search.as_ref().map_or(0, |s| s.candidates.len())
        }
    }
}

pub fn assess(
    job: &Job,
    peaks: Vec<Peak>,
    duration: f64,
    tracks: &[Track],
    options: &Options,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Assessment> {
    control.check()?;
    let mut result = Assessment::pending(peaks);
    if result.peaks.len() < MIN_MATCHES || duration < 480. {
        return Ok(result);
    }
    let times: Vec<_> = result.peaks.iter().map(|p| p.time_s).collect();
    let mut prepared = PreparedSearch::new(job, tracks, options, control)?;
    let search = prepared.locate(&times, duration, control, &|m, p| {
        progress(m, p / (CONTROLS + 1) as f32)
    })?;
    let sufficient = search.candidates.first().is_some_and(|c| {
        c.pairs.iter().filter(|p| p.residual_s.abs() <= 3.).count() >= MIN_MATCHES
    });
    result.search = Some(search);
    if !sufficient {
        result.explanation = "No leading location has six sound peaks matched to separate flight passages. More recording time or better flight coverage is needed.".into();
        return Ok(result);
    }
    for (index, (seed, mapping)) in control_permutations((duration / 120.).round() as usize)?
        .into_iter()
        .enumerate()
    {
        control.check()?;
        let shuffled = apply_permutation(&times, duration, &mapping);
        let null = prepared.locate(&shuffled, duration, control, &|m, p| {
            progress(
                &format!("Shuffled control {}/{}: {m}", index + 1, CONTROLS),
                (index as f32 + 1. + p) / (CONTROLS + 1) as f32,
            );
        })?;
        result.control_likelihood_evaluations += null.likelihood_evaluations;
        result.control_permutation_seeds.push(seed);
        result.control_maximum_scores.push(
            null.candidates
                .first()
                .map_or(0., |c| c.log_likelihood_ratio),
        );
    }
    apply_screen(&mut result);
    Ok(result)
}

fn apply_screen(result: &mut Assessment) {
    result.region = None;
    result.screen = Screen::InsufficientEvidence;
    result.score_gap_beyond_1km = None;
    result.competitive_location_spread_m = None;
    let Some(search) = &result.search else {
        return;
    };
    let Some(first) = search.candidates.first() else {
        return;
    };
    if result.control_maximum_scores.len() != CONTROLS
        || result
            .control_maximum_scores
            .iter()
            .any(|s| !s.is_finite() || *s + 1e-7 >= first.log_likelihood_ratio)
    {
        result.screen = Screen::InsufficientEvidence;
        result.explanation = "The leading score did not exceed every shuffled-sound control. No reliable timing signal was established.".into();
        return;
    }
    let center = Point {
        lat: first.latitude,
        lon: first.longitude,
        alt: 0.,
    };
    let gap = 100_f64.ln();
    let mut farthest: f64 = 0.;
    let mut distant: Option<f64> = None;
    for point in &search.location_scores {
        let distance = horizontal_distance(center, point);
        if distance > REGION_M {
            distant = Some(distant.map_or(point.log_likelihood_ratio, |s| {
                s.max(point.log_likelihood_ratio)
            }));
        }
        if point.log_likelihood_ratio >= first.log_likelihood_ratio - gap {
            farthest = farthest.max(distance);
        }
    }
    result.score_gap_beyond_1km = distant.map(|s| first.log_likelihood_ratio - s);
    result.competitive_location_spread_m = Some(farthest);
    let region = enclosing_region(center, first.log_likelihood_ratio, &search.location_scores);
    if let Some(region) = region {
        result.region = Some(region);
        result.screen = Screen::TimingCandidate;
        result.explanation = "The timing score exceeded all 19 shuffled controls. The evaluated locations within a likelihood factor of 100 fit within 1 km of the reported region center. This is a model screen, not a verified accuracy radius.".into();
    } else {
        result.screen = Screen::Ambiguous;
        result.explanation = "The timing signal has competing locations that do not fit the checked 1 km regions. The score difference is less than a likelihood factor of 100; the recording does not give a single useful location.".into();
    }
}

fn enclosing_region(
    first: Point,
    best_score: f64,
    scores: &[LocationScore],
) -> Option<LocationRegion> {
    if scores.is_empty()
        || !first.valid()
        || !best_score.is_finite()
        || scores.iter().any(|p| {
            !p.log_likelihood_ratio.is_finite()
                || !Point {
                    lat: p.latitude,
                    lon: p.longitude,
                    alt: 0.,
                }
                .valid()
        })
    {
        return None;
    }
    let mut points: Vec<_> = scores
        .iter()
        .filter(|p| p.log_likelihood_ratio >= best_score - 100_f64.ln())
        .map(|p| Point {
            lat: p.latitude,
            lon: p.longitude,
            alt: 0.,
        })
        .collect();
    if !points
        .iter()
        .any(|p| p.lat == first.lat && p.lon == first.lon)
    {
        points.push(first);
    }
    let region = |center: Point| {
        let radius = points
            .iter()
            .map(|p| center.distance(*p))
            .fold(0_f64, f64::max);
        (radius <= REGION_M).then_some(LocationRegion {
            latitude: center.lat,
            longitude: center.lon,
            enclosing_radius_m: radius,
            competitive_points: points.len(),
        })
    };
    // Keep existing accepted centers. If the first point lies near an edge,
    // also check the middle of the competitive latitude/longitude bounds.
    // This bounded calculation is conservative: it need not find the smallest
    // possible circle. Every point must pass the final physical-distance check.
    region(first).or_else(|| {
        let (mut south, mut north, mut west, mut east) = (first.lat, first.lat, 0_f64, 0_f64);
        for point in &points {
            south = south.min(point.lat);
            north = north.max(point.lat);
            let delta = (point.lon - first.lon + 180.).rem_euclid(360.) - 180.;
            west = west.min(delta);
            east = east.max(delta);
        }
        region(Point {
            lat: (south + north) * 0.5,
            lon: (first.lon + (west + east) * 0.5 + 180.).rem_euclid(360.) - 180.,
            alt: 0.,
        })
    })
}
fn horizontal_distance(center: Point, point: &LocationScore) -> f64 {
    center.distance(Point {
        lat: point.latitude,
        lon: point.longitude,
        alt: 0.,
    })
}

fn permutation(blocks: usize, seed: u64) -> Vec<usize> {
    let mut mapping: Vec<_> = (0..blocks).collect();
    let mut state = seed;
    for i in (1..blocks).rev() {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        mapping.swap(i, (z % (i as u64 + 1)) as usize);
    }
    mapping
}
fn control_permutations(blocks: usize) -> Result<Vec<(u64, Vec<usize>)>> {
    anyhow::ensure!(
        blocks >= 4,
        "At least four sound blocks are needed for timing controls"
    );
    let mut used = std::collections::HashSet::new();
    used.insert((0..blocks).collect::<Vec<_>>());
    let mut result = vec![];
    for seed in 202611090..202621090 {
        let mapping = permutation(blocks, seed);
        if used.insert(mapping.clone()) {
            result.push((seed, mapping));
        }
        if result.len() == CONTROLS {
            return Ok(result);
        }
    }
    anyhow::bail!("Could not construct distinct shuffled controls")
}
fn apply_permutation(times: &[f64], duration: f64, mapping: &[usize]) -> Vec<f64> {
    // Equal blocks keep even a fractional final audio second inside the clip.
    let blocks = mapping.len();
    let width = duration / blocks as f64;
    let mut result: Vec<_> = times
        .iter()
        .map(|t| {
            let block = ((*t / width).floor() as usize).min(blocks - 1);
            (t - block as f64 * width + mapping[block] as f64 * width).clamp(0., duration)
        })
        .collect();
    result.sort_by(f64::total_cmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn block_permutations_preserve_counts_and_fractional_duration() {
        let duration = 3599.998625;
        let times: Vec<_> = (0..100).map(|i| i as f64 * 35. + 0.05).collect();
        let blocks = (duration / 120_f64).round() as usize;
        let a = apply_permutation(&times, duration, &permutation(blocks, 1234));
        assert_eq!(
            a,
            apply_permutation(&times, duration, &permutation(blocks, 1234))
        );
        assert_ne!(
            a,
            apply_permutation(&times, duration, &permutation(blocks, 1235))
        );
        assert_eq!(a.len(), times.len());
        assert!(a.windows(2).all(|p| p[0] < p[1]));
        assert!(a.iter().all(|t| *t >= 0. && *t <= duration));
        let width = duration / (duration / 120.).round();
        let mut old: Vec<_> = times.iter().map(|t| t % width).collect();
        let mut new: Vec<_> = a.iter().map(|t| t % width).collect();
        old.sort_by(f64::total_cmp);
        new.sort_by(f64::total_cmp);
        // Compare the actual residuals. Rounding either side of a half-
        // microsecond boundary can turn floating-point noise into a false fail.
        assert!(old.iter().zip(&new).all(|(a, b)| (a - b).abs() < 1e-9));
    }
    #[test]
    fn controls_are_distinct_and_never_repeat_the_original_block_order() {
        for blocks in [4, 5, 15, 30, 60] {
            let controls = control_permutations(blocks).unwrap();
            assert_eq!(controls, control_permutations(blocks).unwrap());
            assert_eq!(controls.len(), CONTROLS);
            let unique: std::collections::HashSet<_> =
                controls.iter().map(|(_, m)| m.clone()).collect();
            assert_eq!(unique.len(), CONTROLS);
            assert!(!unique.contains(&(0..blocks).collect::<Vec<_>>()));
        }
        assert!(control_permutations(3).is_err());
    }
    #[test]
    fn unretained_spatial_alternative_keeps_a_strong_signal_ambiguous() {
        let candidate = super::super::Candidate {
            latitude: 41.,
            longitude: -87.,
            elevation_m: 0.,
            offset_utc: 10000.,
            log_likelihood_ratio: 40.,
            reference_passages: 20,
            reference_exposure: 20.,
            background_expected_events: 10.,
            detection_probability: 0.5,
            pairs: vec![],
        };
        let mut result = Assessment::pending(vec![]);
        result.search = Some(SearchResult {
            options: Options::default(),
            points: 2,
            eligible_points: 2,
            likelihood_evaluations: 2,
            terrain_fallback_points: 0,
            candidates: vec![candidate],
            location_scores: vec![
                LocationScore {
                    latitude: 41.,
                    longitude: -87.,
                    log_likelihood_ratio: 40.,
                },
                LocationScore {
                    latitude: 41.02,
                    longitude: -87.,
                    log_likelihood_ratio: 38.,
                },
            ],
        });
        result.control_maximum_scores = vec![10.; CONTROLS];
        apply_screen(&mut result);
        assert_eq!(result.screen, Screen::Ambiguous);
        result.search.as_mut().unwrap().location_scores[1].log_likelihood_ratio = 30.;
        apply_screen(&mut result);
        assert_eq!(result.screen, Screen::TimingCandidate);
        assert!(result.region.is_some());
        result.control_maximum_scores[3] = 41.;
        apply_screen(&mut result);
        assert_eq!(result.screen, Screen::InsufficientEvidence);
        assert_eq!(result.candidate_count(), 0);
        assert!(result.region.is_none());
        result.control_maximum_scores[3] = 40. - 1e-8;
        apply_screen(&mut result);
        assert_eq!(result.screen, Screen::InsufficientEvidence);
    }

    #[test]
    fn compact_region_can_have_its_leading_point_near_an_edge() {
        let first = Point {
            lat: 41.,
            lon: -87.,
            alt: 0.,
        };
        let scores = [
            LocationScore {
                latitude: 41.,
                longitude: -87.,
                log_likelihood_ratio: 40.,
            },
            LocationScore {
                latitude: 41.014,
                longitude: -87.,
                log_likelihood_ratio: 38.,
            },
            LocationScore {
                latitude: 42.,
                longitude: -87.,
                log_likelihood_ratio: 20.,
            },
        ];
        assert!(horizontal_distance(first, &scores[1]) > REGION_M);
        let region = enclosing_region(first, 40., &scores).unwrap();
        assert!((region.latitude - 41.007).abs() < 1e-10);
        assert_eq!(region.competitive_points, 2);
        assert!((700. ..800.).contains(&region.enclosing_radius_m));
        let center = Point {
            lat: region.latitude,
            lon: region.longitude,
            alt: 0.,
        };
        for score in &scores[..2] {
            assert!(horizontal_distance(center, score) <= region.enclosing_radius_m);
        }
    }

    #[test]
    fn compact_region_handles_the_date_line_without_using_the_far_side_of_earth() {
        let first = Point {
            lat: 0.,
            lon: 179.995,
            alt: 0.,
        };
        let scores = [
            LocationScore {
                latitude: 0.,
                longitude: 179.995,
                log_likelihood_ratio: 40.,
            },
            LocationScore {
                latitude: 0.,
                longitude: -179.995,
                log_likelihood_ratio: 38.,
            },
        ];
        let region = enclosing_region(first, 40., &scores).unwrap();
        assert!(region.longitude.abs() > 179.999);
        assert!((500. ..600.).contains(&region.enclosing_radius_m));
    }

    #[test]
    fn enclosing_region_rejects_wide_missing_and_invalid_evidence() {
        let first = Point {
            lat: 41.,
            lon: -87.,
            alt: 0.,
        };
        let mut scores = [
            LocationScore {
                latitude: 41.,
                longitude: -87.,
                log_likelihood_ratio: 40.,
            },
            LocationScore {
                latitude: 41.03,
                longitude: -87.,
                log_likelihood_ratio: 38.,
            },
        ];
        assert!(enclosing_region(first, 40., &scores).is_none());
        assert!(enclosing_region(first, 40., &[]).is_none());
        scores[1].latitude = f64::NAN;
        assert!(enclosing_region(first, 40., &scores).is_none());
        scores[1].latitude = 41.;
        scores[1].log_likelihood_ratio = f64::NAN;
        assert!(enclosing_region(first, 40., &scores).is_none());
    }

    #[test]
    fn older_timing_reports_do_not_invent_a_region() {
        let mut value = serde_json::to_value(Assessment::pending(vec![])).unwrap();
        value.as_object_mut().unwrap().remove("region");
        let old: Assessment = serde_json::from_value(value).unwrap();
        assert!(old.region.is_none());
        assert_eq!(old.screen, Screen::InsufficientEvidence);
    }
}
