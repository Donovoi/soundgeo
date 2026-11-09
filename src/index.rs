//! Time/ECEF buckets prune flight comparisons; observer seeds follow flight
//! corridors at a fixed metre spacing, independent of the country's area.
use crate::{
    Control, Job, Progress,
    audio::Event,
    geo::{Bounds, Point, Terrain, Vec3},
    model::Track,
    search::{
        FEATURE_ERROR_LIMIT, Seed, complete_seed, feature_error, feature_range_factor,
        score_and_prune_seeds, within_time,
    },
};
use anyhow::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
const CELL: f64 = 20000.;
type Key = (i64, i32, i32, i32);
type QueryKey = (i64, i64, i32, i32, i32, i32);
#[derive(Clone, Copy)]
struct Envelope {
    min: Vec3,
    max: Vec3,
}
impl Envelope {
    fn point(p: Vec3) -> Self {
        Self { min: p, max: p }
    }
    fn include(&mut self, p: Vec3) {
        self.min = Vec3(
            self.min.0.min(p.0),
            self.min.1.min(p.1),
            self.min.2.min(p.2),
        );
        self.max = Vec3(
            self.max.0.max(p.0),
            self.max.1.max(p.1),
            self.max.2.max(p.2),
        );
    }
    fn distance_squared(self, p: Vec3) -> f64 {
        let dx = (self.min.0 - p.0).max(0.).max(p.0 - self.max.0);
        let dy = (self.min.1 - p.1).max(0.).max(p.1 - self.max.1);
        let dz = (self.min.2 - p.2).max(0.).max(p.2 - self.max.2);
        dx * dx + dy * dy + dz * dz
    }
}
#[derive(Clone, Copy)]
struct QueryTrack {
    id: usize,
    envelope: Envelope,
}
fn key(xyz: Vec3, time: f64) -> Key {
    (
        (time / 60.).floor() as i64,
        (xyz.0 / CELL).floor() as i32,
        (xyz.1 / CELL).floor() as i32,
        (xyz.2 / CELL).floor() as i32,
    )
}
pub struct FlightIndex {
    buckets: Arc<HashMap<Key, Vec<usize>>>,
    envelopes: Arc<Vec<Vec<(i64, Envelope)>>>,
    queries: Mutex<HashMap<QueryKey, Arc<Vec<QueryTrack>>>>,
}
impl FlightIndex {
    pub fn new(tracks: &[Track], control: &Control) -> Result<Self> {
        let mut buckets: HashMap<Key, Vec<usize>> = HashMap::new();
        let mut envelopes = Vec::with_capacity(tracks.len());
        for (i, track) in tracks.iter().enumerate() {
            control.check()?;
            for s in &track.samples {
                let bucket = buckets.entry(key(s.xyz, s.time)).or_default();
                if bucket.last() != Some(&i) {
                    bucket.push(i);
                }
            }
            let mut minutes: Vec<(i64, Envelope)> = Vec::new();
            for pair in track.samples.windows(2) {
                let dt = pair[1].time - pair[0].time;
                if dt <= 0. || dt > 60. {
                    continue;
                }
                // Both endpoints bound the entire interpolated ECEF segment.
                // Include them in every crossed minute, including a boundary.
                for minute in
                    (pair[0].time / 60.).floor() as i64..=(pair[1].time / 60.).floor() as i64
                {
                    if minutes.last().is_none_or(|last| last.0 != minute) {
                        minutes.push((minute, Envelope::point(pair[0].xyz)));
                    }
                    let envelope = &mut minutes.last_mut().unwrap().1;
                    envelope.include(pair[0].xyz);
                    envelope.include(pair[1].xyz);
                }
            }
            envelopes.push(minutes);
        }
        Ok(Self {
            buckets: Arc::new(buckets),
            envelopes: Arc::new(envelopes),
            queries: Mutex::new(HashMap::new()),
        })
    }
    fn fork_queries(&self) -> Self {
        Self {
            buckets: Arc::clone(&self.buckets),
            envelopes: Arc::clone(&self.envelopes),
            queries: Mutex::new(HashMap::new()),
        }
    }
    pub fn nearby(&self, p: Point, arrival: f64, radius: f64, c: f64) -> Vec<usize> {
        let xyz = p.ecef();
        let (_, x, y, z) = key(xyz, arrival);
        let select = |hits: &[QueryTrack]| -> Vec<usize> {
            let squared = (radius + 1e-6).powi(2);
            hits.iter()
                .filter(|hit| hit.envelope.distance_squared(xyz) <= squared)
                .map(|hit| hit.id)
                .collect()
        };
        // A segment may cross the query radius between fixes up to 60 s apart.
        // One extra cell covers the importer's 280 m/s maximum segment speed.
        let reach = (radius / CELL).ceil() as i32 + 1;
        // Pad for track interpolation, acoustic delay and approximate CPA measurement.
        let from = ((arrival - radius / c - 65.) / 60.).floor() as i64;
        let to = ((arrival + 65.) / 60.).floor() as i64;
        let query = (from, to, x, y, z, reach);
        if let Some(cached) = self
            .queries
            .lock()
            .ok()
            .and_then(|cache| cache.get(&query).cloned())
        {
            return select(&cached);
        }
        let mut found = BTreeSet::new();
        for t in from..=to {
            for dx in -reach..=reach {
                for dy in -reach..=reach {
                    for dz in -reach..=reach {
                        if let Some(ids) = self.buckets.get(&(t, x + dx, y + dy, z + dz)) {
                            found.extend(ids);
                        }
                    }
                }
            }
        }
        let found: Vec<_> = found
            .into_iter()
            .copied()
            .filter_map(|id| {
                let minutes = &self.envelopes[id];
                let start = minutes.partition_point(|(minute, _)| *minute < from);
                let mut covered = minutes[start..]
                    .iter()
                    .take_while(|(minute, _)| *minute <= to);
                let mut envelope = covered.next()?.1;
                for (_, next) in covered {
                    envelope.include(next.min);
                    envelope.include(next.max);
                }
                Some(QueryTrack { id, envelope })
            })
            .collect();
        let found = Arc::new(found);
        if let Ok(mut cache) = self.queries.lock() {
            // Neighboring metre-scale hypotheses share the same coarse buckets.
            // Cache unfiltered envelopes. Exact observer/radius filtering must
            // happen again when another point shares these coarse buckets.
            if cache.len() >= 8192 {
                cache.clear();
            }
            cache.insert(query, Arc::clone(&found));
        }
        select(&found)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EmissionWindow {
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalBatch {
    /// Inclusive flight timestamp; this does not constrain the recording clock.
    pub emission_from_unix: f64,
    /// Exclusive flight timestamp.
    pub emission_to_unix: f64,
    pub hypotheses: usize,
}

impl ProposalBatch {
    pub(crate) fn window(&self) -> EmissionWindow {
        EmissionWindow {
            start: self.emission_from_unix,
            end: self.emission_to_unix,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalPartition {
    pub complete: bool,
    pub max_hypotheses_per_batch: usize,
    pub batches: Vec<ProposalBatch>,
}

#[derive(Debug)]
pub(crate) struct HypothesisBudgetExceeded {
    pub limit: usize,
    pub before_matching: bool,
}

impl std::fmt::Display for HypothesisBudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Indexed search exceeds the {} hypothesis budget; no partial location is reported.",
            self.limit
        )?;
        if self.before_matching {
            write!(f, " The budget check stopped before curve matching.")?;
        }
        Ok(())
    }
}

impl std::error::Error for HypothesisBudgetExceeded {}

fn corridor_pool(job: &Job, terrain: &Terrain) -> Result<Option<rayon::ThreadPool>> {
    if terrain.has_tiles() {
        return Ok(None);
    }
    Ok(Some(
        rayon::ThreadPoolBuilder::new()
            .num_threads(
                std::thread::available_parallelism()
                    .map_or(1, |n| n.get().min(job.search_threads.unwrap_or(4).max(1))),
            )
            .build()?,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn corridor_seeds(
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    bounds: Bounds,
    terrain: &mut Terrain,
    control: &Control,
    progress: &Progress<'_>,
    window: Option<EmissionWindow>,
) -> Result<(Vec<Seed>, usize)> {
    let counter = AtomicUsize::new(0);
    let mut seeds = vec![];
    let anchors = crate::audio::anchor_indices(events);
    // Keep disk-backed terrain queries serial so their tile cache stays shared.
    // Without tiles, each worker can evaluate the bundled geoid independently.
    let prototype = terrain.empty_cache_clone();
    let pool = corridor_pool(job, terrain)?;
    // Count the same spatial proposals before expensive curve matching. A run
    // that cannot fit its budget must not spend hours collecting partial seeds.
    check_corridor_budget(
        job,
        events,
        tracks,
        &anchors,
        bounds,
        terrain,
        pool.as_ref(),
        control,
        progress,
        window,
    )?;
    progress(
        &format!(
            "Flight-corridor workers: {}",
            pool.as_ref().map_or(1, |p| p.current_num_threads())
        ),
        0.42,
    );
    let index = FlightIndex::new(tracks, control)?;
    for (ordinal, &anchor) in anchors.iter().enumerate() {
        for (batch_no, batch) in tracks.chunks(32).enumerate() {
            control.check()?;
            let first_id = batch_no * 32;
            progress(
                &format!(
                    "Indexed flight corridors: anchor {}/{}, track {}/{}, {} hypotheses",
                    ordinal + 1,
                    anchors.len(),
                    first_id + 1,
                    tracks.len(),
                    counter.load(Ordering::Relaxed)
                ),
                0.42 + 0.30 * (ordinal * tracks.len() + first_id) as f32
                    / (anchors.len() * tracks.len()) as f32,
            );
            let results: Vec<Result<Vec<Seed>>> = if let Some(pool) = &pool {
                pool.install(|| {
                    batch
                        .par_iter()
                        .enumerate()
                        .map(|(offset, track)| {
                            let local_index = index.fork_queries();
                            let mut local_terrain = prototype.empty_cache_clone();
                            track_seeds(
                                job,
                                events,
                                tracks,
                                track,
                                first_id + offset,
                                anchor,
                                bounds,
                                &mut local_terrain,
                                &local_index,
                                &counter,
                                control,
                                window,
                            )
                        })
                        .collect()
                })
            } else {
                batch
                    .iter()
                    .enumerate()
                    .map(|(offset, track)| {
                        track_seeds(
                            job,
                            events,
                            tracks,
                            track,
                            first_id + offset,
                            anchor,
                            bounds,
                            terrain,
                            &index,
                            &counter,
                            control,
                            window,
                        )
                    })
                    .collect()
            };
            // Merge in track order, making parallel and serial runs reproducible.
            for result in results {
                for seed in result? {
                    seeds.push(seed);
                    if seeds.len() >= 8192 {
                        let mut prune = || {
                            score_and_prune_seeds(&mut seeds, 2048, job, events, tracks, control)
                        };
                        if let Some(pool) = &pool {
                            pool.install(prune)?;
                        } else {
                            prune()?;
                        }
                    }
                }
            }
        }
    }
    let examined = counter.load(Ordering::Relaxed);
    if pool.is_some() && examined > 0 {
        terrain.missing += 1;
    }
    Ok((seeds, examined))
}

#[allow(clippy::too_many_arguments)]
fn track_seeds(
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    track: &Track,
    id: usize,
    anchor: usize,
    bounds: Bounds,
    terrain: &mut Terrain,
    index: &FlightIndex,
    counter: &AtomicUsize,
    control: &Control,
    window: Option<EmissionWindow>,
) -> Result<Vec<Seed>> {
    let c = job.sound_speed();
    let range_factor = feature_range_factor();
    let mut seeds = vec![];
    for_each_corridor_point(
        job,
        &events[anchor],
        track,
        bounds,
        terrain,
        control,
        window,
        |first, point, expected| {
            let examined = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if examined > job.max_hypotheses {
                return Err(HypothesisBudgetExceeded {
                    limit: job.max_hypotheses,
                    before_matching: false,
                }
                .into());
            }
            if examined.is_multiple_of(256) {
                control.check()?;
            }
            let Some((arrival, range, actual_speed)) =
                track.cpa_near(point.ecef(), c, Some((expected - 30., expected + 30.)))
            else {
                return Ok(());
            };
            let start = arrival - first.cpa_s;
            if !within_time(job, start) {
                return Ok(());
            }
            let score = feature_error(first, range, actual_speed);
            if score > FEATURE_ERROR_LIMIT {
                return Ok(());
            }
            if let Some(seed) =
                complete_seed(point, start, anchor, id, score, events, |wanted, event| {
                    let radius = (event.slant_m.max(100.) * range_factor).clamp(5000., 40000.);
                    index
                        .nearby(point, wanted, radius, c)
                        .into_iter()
                        .filter_map(|j| {
                            let (t, r, v) = tracks[j].cpa_near(
                                point.ecef(),
                                c,
                                Some((wanted - 45., wanted + 45.)),
                            )?;
                            Some((j, t, r, v))
                        })
                        .collect()
                })
            {
                seeds.push(seed);
                // Bound each worker's retained hypotheses as well as
                // the global beam, even for unusually long tracks.
                if seeds.len() >= 8192 {
                    score_and_prune_seeds(&mut seeds, 2048, job, events, tracks, control)?;
                }
            }
            Ok(())
        },
    )?;
    Ok(seeds)
}

// Keep geometry in one place so the budget pass counts exactly what matching
// will examine, including alternate ridges, time gates and terrain heights.
#[allow(clippy::too_many_arguments)]
fn for_each_corridor_point(
    job: &Job,
    event: &Event,
    track: &Track,
    bounds: Bounds,
    terrain: &mut Terrain,
    control: &Control,
    window: Option<EmissionWindow>,
    mut visit: impl FnMut(&Event, Point, f64) -> Result<()>,
) -> Result<()> {
    control.check()?;
    let c = job.sound_speed();
    for first in event.variants() {
        let range_factor = feature_range_factor();
        // The straight-flight audio summary can underestimate range on turns or
        // partial curves. Sample the range interval already allowed by the feature
        // gate; the previous 0.5..2 interval could reject all valid proposals.
        let mut slants: Vec<_> = [
            1. / range_factor,
            0.125,
            0.25,
            0.5,
            0.75,
            1.,
            1.5,
            2.,
            3.,
            5.,
            8.,
            range_factor,
        ]
        .into_iter()
        .map(|scale| (first.slant_m.max(100.) * scale).min(40000.))
        .collect();
        slants.sort_by(f64::total_cmp);
        slants.dedup_by(|a, b| (*a - *b).abs() < 1.);
        // Carry spacing along each continuous eligible path. Starting again at
        // every fix makes dense recordings produce extra search hypotheses.
        let mut next_distance = 0.;
        for pair in track.samples.windows(2) {
            control.check()?;
            let dt = pair[1].time - pair[0].time;
            if dt <= 0. || dt > 60. {
                next_distance = 0.;
                continue;
            }
            let speed = (pair[1].xyz - pair[0].xyz).norm() / dt;
            if speed < 25. || (speed / first.speed_mps).ln().abs() > 0.8 {
                next_distance = 0.;
                continue;
            }
            let latmid = (pair[0].point.lat + pair[1].point.lat) * 0.5;
            let lon_m = 111320. * latmid.to_radians().cos().abs().max(0.05);
            let east = (pair[1].point.lon - pair[0].point.lon) * lon_m;
            let north = (pair[1].point.lat - pair[0].point.lat) * 111320.;
            let length = east.hypot(north);
            if length < 1. {
                next_distance = 0.;
                continue;
            }
            while next_distance < length {
                let f = next_distance / length;
                next_distance += job.grid_m;
                let emit = pair[0].time + dt * f;
                // Advance path spacing before filtering so every original
                // proposal belongs to exactly one half-open flight-time batch.
                if window.is_some_and(|w| emit < w.start || emit >= w.end) {
                    continue;
                }
                let a = pair[0].point;
                let b = pair[1].point;
                let plane = Point {
                    lat: a.lat + (b.lat - a.lat) * f,
                    lon: a.lon + (b.lon - a.lon) * f,
                    alt: a.alt + (b.alt - a.alt) * f,
                };
                // No clip in the requested known-time window can hear this anchor.
                if let Some(t) = job.start_utc
                    && (emit + first.slant_m / c - first.cpa_s - t).abs()
                        > job.time_tolerance_s + 120.
                {
                    continue;
                }
                let anchor_ground = terrain.elevation(plane);
                for &slant in &slants {
                    let height = plane.alt - anchor_ground;
                    if slant < height.abs() {
                        continue;
                    }
                    for side in [-1., 1.] {
                        let mut ground = anchor_ground;
                        let mut point = plane;
                        for _ in 0..2 {
                            let cross = (slant * slant - (plane.alt - ground).powi(2))
                                .max(0.)
                                .sqrt();
                            point = Point {
                                lat: plane.lat + side * east / length * cross / 111320.,
                                lon: plane.lon - side * north / length * cross / lon_m,
                                alt: ground,
                            };
                            if !bounds.contains(point) {
                                break;
                            }
                            ground = terrain.elevation(point);
                            point.alt = ground;
                        }
                        if !bounds.contains(point) {
                            continue;
                        }
                        visit(first, point, emit + slant / c)?;
                    }
                }
            }
            next_distance = (next_distance - length).max(0.);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn check_corridor_budget(
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    anchors: &[usize],
    bounds: Bounds,
    terrain: &Terrain,
    pool: Option<&rayon::ThreadPool>,
    control: &Control,
    progress: &Progress<'_>,
    window: Option<EmissionWindow>,
) -> Result<usize> {
    let counter = AtomicUsize::new(0);
    // Do not change the caller's missing-tile count or populate its cache.
    let mut budget_terrain = terrain.empty_cache_clone();
    let count = |_: &Event, _: Point, _: f64| -> Result<()> {
        let examined = counter.fetch_add(1, Ordering::Relaxed) + 1;
        if examined > job.max_hypotheses {
            return Err(HypothesisBudgetExceeded {
                limit: job.max_hypotheses,
                before_matching: true,
            }
            .into());
        }
        if examined.is_multiple_of(256) {
            control.check()?;
        }
        Ok(())
    };
    for (ordinal, &anchor) in anchors.iter().enumerate() {
        for (batch_no, batch) in tracks.chunks(32).enumerate() {
            control.check()?;
            progress(
                &format!(
                    "Checking search budget: anchor {}/{}, track {}/{}, {} hypotheses",
                    ordinal + 1,
                    anchors.len(),
                    batch_no * 32 + 1,
                    tracks.len(),
                    counter.load(Ordering::Relaxed)
                ),
                0.42,
            );
            if let Some(pool) = pool {
                pool.install(|| {
                    batch.par_iter().try_for_each(|track| {
                        let mut local_terrain = terrain.empty_cache_clone();
                        for_each_corridor_point(
                            job,
                            &events[anchor],
                            track,
                            bounds,
                            &mut local_terrain,
                            control,
                            window,
                            count,
                        )
                    })
                })?;
            } else {
                for track in batch {
                    for_each_corridor_point(
                        job,
                        &events[anchor],
                        track,
                        bounds,
                        &mut budget_terrain,
                        control,
                        window,
                        count,
                    )?;
                }
            }
        }
    }
    progress(
        &format!(
            "Search budget fits: {} hypotheses",
            counter.load(Ordering::Relaxed)
        ),
        0.42,
    );
    Ok(counter.load(Ordering::Relaxed))
}

/// Plan the complete partition before any expensive matching starts. Every
/// batch retains the complete reference set for associations and refinement.
#[allow(clippy::too_many_arguments)]
pub(crate) fn partition_proposals(
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    bounds: Bounds,
    terrain: &Terrain,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<ProposalPartition> {
    let mut start = f64::INFINITY;
    let mut end = f64::NEG_INFINITY;
    for sample in tracks.iter().flat_map(|t| &t.samples) {
        start = start.min(sample.time);
        end = end.max(sample.time);
    }
    end = end.next_up();
    anyhow::ensure!(
        start.is_finite() && end.is_finite() && start < end,
        "Flight timestamps cannot define a finite proposal partition"
    );
    let pool = corridor_pool(job, terrain)?;
    let anchors = crate::audio::anchor_indices(events);
    let mut pending = vec![EmissionWindow { start, end }];
    let mut batches = vec![];
    while let Some(window) = pending.pop() {
        control.check()?;
        match check_corridor_budget(
            job,
            events,
            tracks,
            &anchors,
            bounds,
            terrain,
            pool.as_ref(),
            control,
            progress,
            Some(window),
        ) {
            Ok(hypotheses) => batches.push(ProposalBatch {
                emission_from_unix: window.start,
                emission_to_unix: window.end,
                hypotheses,
            }),
            Err(error)
                if error
                    .downcast_ref::<HypothesisBudgetExceeded>()
                    .is_some_and(|e| e.before_matching) =>
            {
                let middle = window.start + (window.end - window.start) * 0.5;
                if window.end - window.start <= 1.
                    || middle <= window.start
                    || middle >= window.end
                    || batches.len() + pending.len() + 2 > 4096
                {
                    return Err(error.context("Automatic proposal partitioning stopped before curve matching at its minimum interval or batch limit. Increase the hypothesis budget; no partial location is reported"));
                }
                // Stack order processes the full timeline from left to right.
                pending.push(EmissionWindow {
                    start: middle,
                    end: window.end,
                });
                pending.push(EmissionWindow {
                    start: window.start,
                    end: middle,
                });
            }
            Err(error) => return Err(error),
        }
    }
    Ok(ProposalPartition {
        complete: true,
        max_hypotheses_per_batch: job.max_hypotheses,
        batches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        aircraft::AircraftKind,
        demo::{EPOCH, TRUTH},
        model::Sample,
    };

    fn point(east: f64, north: f64, height: f64) -> Point {
        Point {
            lat: TRUTH.lat + north / 111320.,
            lon: TRUTH.lon + east / (111320. * TRUTH.lat.to_radians().cos()),
            alt: TRUTH.alt + height,
        }
    }

    fn track(id: &str, fixes: &[(f64, f64, f64, f64)]) -> Track {
        Track {
            id: id.into(),
            kind: AircraftKind::Unknown,
            samples: fixes
                .iter()
                .map(|&(t, east, north, height)| Sample::new(EPOCH + t, point(east, north, height)))
                .collect(),
        }
    }

    fn corridor_proposals(track: &Track) -> Vec<(Point, f64)> {
        corridor_proposals_in_window(track, None)
    }

    fn corridor_proposals_in_window(
        track: &Track,
        window: Option<EmissionWindow>,
    ) -> Vec<(Point, f64)> {
        let job = Job {
            grid_m: 200.,
            ..Default::default()
        };
        let event = Event {
            start_s: 0.,
            end_s: 20.,
            cpa_s: 10.,
            base_hz: 100.,
            speed_mps: 50.,
            slant_m: 500.,
            fit_rmse_hz: 0.,
            contrast_db: 20.,
            ridge: vec![],
            activity_span_s: None,
            alternatives: vec![],
        };
        let bounds = Bounds {
            south: TRUTH.lat - 1.,
            north: TRUTH.lat + 1.,
            west: TRUTH.lon - 1.,
            east: TRUTH.lon + 1.,
        };
        let mut result = vec![];
        for_each_corridor_point(
            &job,
            &event,
            track,
            bounds,
            &mut Terrain::new(None, TRUTH.alt),
            &Control::default(),
            window,
            |_, point, arrival| {
                result.push((point, arrival));
                Ok(())
            },
        )
        .unwrap();
        result
    }

    fn corridor_centers(proposals: &[(Point, f64)]) -> BTreeSet<(i64, i64)> {
        // The two sides of each constant-height corridor share the same plane point.
        let (pairs, remainder) = proposals.as_chunks::<2>();
        assert!(remainder.is_empty());
        pairs
            .iter()
            .map(|pair| {
                let lat = (pair[0].0.lat + pair[1].0.lat) * 0.5;
                let lon = (pair[0].0.lon + pair[1].0.lon) * 0.5;
                (
                    ((lon - TRUTH.lon) * 111320. * TRUTH.lat.to_radians().cos()).round() as i64,
                    ((lat - TRUTH.lat) * 111320.).round() as i64,
                )
            })
            .collect()
    }

    #[test]
    fn flight_time_partitions_preserve_every_proposal_at_boundaries_and_gaps() {
        let flight = track(
            "turn-and-gap",
            &[
                (0., 0., 0., 100.),
                (3., 150., 0., 100.),
                (7., 150., 200., 100.),
                (202., 1000., 200., 100.),
                (206., 1200., 200., 100.),
                (210., 1400., 200., 100.),
            ],
        );
        let whole = corridor_proposals(&flight);
        let boundaries = [
            EPOCH,
            EPOCH + 4.,
            EPOCH + 202.,
            EPOCH + 206.,
            (EPOCH + 210.).next_up(),
        ];
        let parts: Vec<_> = boundaries
            .windows(2)
            .flat_map(|b| {
                corridor_proposals_in_window(
                    &flight,
                    Some(EmissionWindow {
                        start: b[0],
                        end: b[1],
                    }),
                )
            })
            .collect();
        assert!(!whole.is_empty());
        assert_eq!(whole.len(), parts.len());
        for ((expected, expected_time), (actual, actual_time)) in whole.iter().zip(parts) {
            assert_eq!(expected.lat.to_bits(), actual.lat.to_bits());
            assert_eq!(expected.lon.to_bits(), actual.lon.to_bits());
            assert_eq!(expected.alt.to_bits(), actual.alt.to_bits());
            assert_eq!(expected_time.to_bits(), actual_time.to_bits());
        }
    }

    #[test]
    fn automatic_partition_plan_covers_all_proposals_with_bounded_batches() {
        let tracks = vec![
            track("first", &[(0., 0., 0., 100.), (60., 3000., 0., 100.)]),
            track(
                "later",
                &[(86400., 0., 0., 100.), (86460., 3000., 0., 100.)],
            ),
        ];
        let events = vec![Event {
            start_s: 0.,
            end_s: 20.,
            cpa_s: 10.,
            base_hz: 100.,
            speed_mps: 50.,
            slant_m: 500.,
            fit_rmse_hz: 0.,
            contrast_db: 20.,
            ridge: vec![],
            activity_span_s: None,
            alternatives: vec![],
        }];
        let bounds = Bounds {
            south: TRUTH.lat - 1.,
            north: TRUTH.lat + 1.,
            west: TRUTH.lon - 1.,
            east: TRUTH.lon + 1.,
        };
        let mut job = Job {
            grid_m: 200.,
            max_hypotheses: 100000,
            ..Default::default()
        };
        let terrain = Terrain::new(None, TRUTH.alt);
        let control = Control::default();
        let count = check_corridor_budget(
            &job,
            &events,
            &tracks,
            &[0],
            bounds,
            &terrain,
            None,
            &control,
            &|_, _| {},
            None,
        )
        .unwrap();
        assert!(count > 200);
        job.max_hypotheses = count / 2;
        let before = serde_json::to_value(&job).unwrap();
        let partition = partition_proposals(
            &job,
            &events,
            &tracks,
            bounds,
            &terrain,
            &control,
            &|_, _| {},
        )
        .unwrap();
        assert!(partition.complete && partition.batches.len() >= 2);
        assert_eq!(
            partition
                .batches
                .iter()
                .map(|b| b.hypotheses)
                .sum::<usize>(),
            count
        );
        assert!(
            partition
                .batches
                .iter()
                .all(|b| b.hypotheses <= job.max_hypotheses)
        );
        assert_eq!(partition.batches[0].emission_from_unix, EPOCH);
        assert_eq!(
            partition.batches.last().unwrap().emission_to_unix,
            (EPOCH + 86460.).next_up()
        );
        assert!(
            partition
                .batches
                .windows(2)
                .all(|b| b[0].emission_to_unix == b[1].emission_from_unix)
        );
        assert_eq!(serde_json::to_value(&job).unwrap(), before);
        assert_eq!(terrain.missing, 0);
        let cancelled = Control::default();
        cancelled.stop();
        let error = partition_proposals(
            &job,
            &events,
            &tracks,
            bounds,
            &terrain,
            &cancelled,
            &|_, _| {},
        )
        .unwrap_err();
        assert!(!error.is::<HypothesisBudgetExceeded>());
    }

    #[test]
    fn corridor_spacing_is_independent_of_position_cadence() {
        let sparse: Vec<_> = (0..=3)
            .map(|i| (i as f64 * 6., i as f64 * 300., 0., 100.))
            .collect();
        let dense: Vec<_> = (0..=9)
            .map(|i| (i as f64 * 2., i as f64 * 100., 0., 100.))
            .collect();
        let sparse = corridor_proposals(&track("sparse", &sparse));
        let dense = corridor_proposals(&track("dense", &dense));
        let expected = BTreeSet::from([(0, 0), (200, 0), (400, 0), (600, 0), (800, 0)]);
        assert_eq!(corridor_centers(&sparse), expected);
        assert_eq!(corridor_centers(&dense), expected);
        assert_eq!(sparse.len(), dense.len());
        for (a, b) in sparse.iter().zip(&dense) {
            assert!(a.0.distance(b.0) < 1e-4);
            assert!((a.1 - b.1).abs() < 1e-6);
        }
    }

    #[test]
    fn corridor_spacing_follows_turns_along_the_recorded_path() {
        let proposals = corridor_proposals(&track(
            "turn",
            &[
                (0., 0., 0., 100.),
                (3., 150., 0., 100.),
                (7., 150., 200., 100.),
            ],
        ));
        assert_eq!(
            corridor_centers(&proposals),
            BTreeSet::from([(0, 0), (150, 50)])
        );
    }

    #[test]
    fn corridor_spacing_restarts_after_gaps_and_ineligible_motion() {
        let proposals = corridor_proposals(&track(
            "interrupted",
            &[
                (0., 0., 0., 100.),
                (2., 100., 0., 100.),
                (202., 1000., 0., 100.),
                (204., 1100., 0., 100.),
                (214., 1150., 0., 100.),
                (216., 1250., 0., 100.),
            ],
        ));
        assert_eq!(
            corridor_centers(&proposals),
            BTreeSet::from([(0, 0), (1000, 0), (1150, 0)])
        );
    }

    #[test]
    fn envelopes_keep_interpolated_crossings_and_all_in_range_cpas() {
        let tracks = vec![
            // Both fixes are outside a 1 km query, crossing a minute boundary.
            track(
                "crossing",
                &[(30., -6000., 0., 500.), (90., 6000., 0., 500.)],
            ),
            track(
                "far",
                &[(30., 12000., 0., 500.), (90., 12000., 1000., 500.)],
            ),
            // No valid segment spans this data gap.
            track("gap", &[(30., -6000., 0., 500.), (91., 6000., 0., 500.)]),
            track(
                "turn",
                &[
                    (0., -5000., -3000., 2000.),
                    (60., 1000., 0., 1000.),
                    (120., 6000., -2000., 1500.),
                    (180., 1000., -6000., 1000.),
                ],
            ),
        ];
        let index = FlightIndex::new(&tracks, &Control::default()).unwrap();
        assert_eq!(index.nearby(TRUTH, EPOCH + 60., 1000., 343.), vec![0]);
        let mut checked = 0;
        for east in [-8000., 0., 8000.] {
            for north in [-8000., 0., 8000.] {
                let observer = point(east, north, 0.);
                for time in [0., 30., 59., 60., 90., 180., 600.] {
                    for radius in [500., 1000., 5000., 15000., 40000.] {
                        for c in [300., 343.] {
                            let arrival = EPOCH + time;
                            let hits = index.nearby(observer, arrival, radius, c);
                            for (id, track) in tracks.iter().enumerate() {
                                if let Some((_, range, _)) = track.cpa_near(
                                    observer.ecef(),
                                    c,
                                    Some((arrival - 45., arrival + 45.)),
                                ) && range <= radius
                                {
                                    assert!(
                                        hits.contains(&id),
                                        "missed {id} at {east},{north}, {time}, {radius}, {c}"
                                    );
                                    checked += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 100);
    }

    #[test]
    fn cached_envelopes_are_filtered_for_each_exact_observer_and_radius() {
        let tracks = vec![track(
            "overhead",
            &[(30., -100., 0., 1000.), (90., 100., 0., 1000.)],
        )];
        let index = FlightIndex::new(&tracks, &Control::default()).unwrap();
        let arrival = EPOCH + 60.;
        let raised = point(0., 0., 700.);
        assert_eq!(key(TRUTH.ecef(), arrival), key(raised.ecef(), arrival));
        assert!(index.nearby(TRUTH, arrival, 500., 343.).is_empty());
        assert_eq!(index.nearby(raised, arrival, 500., 343.), vec![0]);
        assert_eq!(index.nearby(TRUTH, arrival, 1500., 343.), vec![0]);
        assert!(index.nearby(TRUTH, arrival, 500., 343.).is_empty());
        assert_eq!(index.queries.lock().unwrap().len(), 1);
    }
}
