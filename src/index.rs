//! Time/ECEF buckets prune flight comparisons; observer seeds follow flight
//! corridors at a fixed metre spacing, independent of the country's area.
use crate::{
    Control, Job, Progress,
    audio::Event,
    geo::{Bounds, Point, Terrain, Vec3},
    model::Track,
    search::{Seed, complete_seed, feature_error, within_time},
};
use anyhow::{Result, bail};
use rayon::prelude::*;
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
    queries: Mutex<HashMap<QueryKey, Vec<usize>>>,
}
impl FlightIndex {
    pub fn new(tracks: &[Track], control: &Control) -> Result<Self> {
        let mut buckets: HashMap<Key, Vec<usize>> = HashMap::new();
        for (i, track) in tracks.iter().enumerate() {
            control.check()?;
            for s in &track.samples {
                let bucket = buckets.entry(key(s.xyz, s.time)).or_default();
                if bucket.last() != Some(&i) {
                    bucket.push(i);
                }
            }
        }
        Ok(Self {
            buckets: Arc::new(buckets),
            queries: Mutex::new(HashMap::new()),
        })
    }
    fn fork_queries(&self) -> Self {
        Self {
            buckets: Arc::clone(&self.buckets),
            queries: Mutex::new(HashMap::new()),
        }
    }
    pub fn nearby(&self, p: Point, arrival: f64, radius: f64, c: f64) -> Vec<usize> {
        let (_, x, y, z) = key(p.ecef(), arrival);
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
            return cached;
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
        let found: Vec<_> = found.into_iter().copied().collect();
        if let Ok(mut cache) = self.queries.lock() {
            // Neighboring metre-scale hypotheses share the same coarse buckets.
            // Cache the exact query result, with bounded memory; no pruning changes.
            if cache.len() >= 8192 {
                cache.clear();
            }
            cache.insert(query, found.clone());
        }
        found
    }
}

pub(crate) fn corridor_seeds(
    job: &Job,
    events: &[Event],
    tracks: &[Track],
    bounds: Bounds,
    terrain: &mut Terrain,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<(Vec<Seed>, usize)> {
    let index = FlightIndex::new(tracks, control)?;
    let counter = AtomicUsize::new(0);
    let mut seeds = vec![];
    let anchors = crate::audio::anchor_indices(events);
    // Keep disk-backed terrain queries serial so their tile cache stays shared.
    // Without tiles, each worker can evaluate the bundled geoid independently.
    let prototype = terrain.empty_cache_clone();
    let pool = if !terrain.has_tiles() {
        Some(
            rayon::ThreadPoolBuilder::new()
                .num_threads(std::thread::available_parallelism().map_or(1, |n| n.get().min(4)))
                .build()?,
        )
    } else {
        None
    };
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
                        )
                    })
                    .collect()
            };
            // Merge in track order, making parallel and serial runs reproducible.
            for result in results {
                for seed in result? {
                    seeds.push(seed);
                    if seeds.len() >= 8192 {
                        seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
                        seeds.truncate(2048);
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
) -> Result<Vec<Seed>> {
    control.check()?;
    let c = job.sound_speed();
    let first = &events[anchor];
    let mut seeds = vec![];
    for pair in track.samples.windows(2) {
        let dt = pair[1].time - pair[0].time;
        if dt <= 0. || dt > 60. {
            continue;
        }
        let speed = (pair[1].xyz - pair[0].xyz).norm() / dt;
        if speed < 25. || (speed / first.speed_mps).ln().abs() > 0.8 {
            continue;
        }
        let latmid = (pair[0].point.lat + pair[1].point.lat) * 0.5;
        let lon_m = 111320. * latmid.to_radians().cos().abs().max(0.05);
        let east = (pair[1].point.lon - pair[0].point.lon) * lon_m;
        let north = (pair[1].point.lat - pair[0].point.lat) * 111320.;
        let length = east.hypot(north);
        if length < 1. {
            continue;
        }
        let steps = (length / job.grid_m).ceil().max(1.) as usize;
        for step in 0..steps {
            let f = step as f64 / steps as f64;
            let a = pair[0].point;
            let b = pair[1].point;
            let plane = Point {
                lat: a.lat + (b.lat - a.lat) * f,
                lon: a.lon + (b.lon - a.lon) * f,
                alt: a.alt + (b.alt - a.alt) * f,
            };
            let emit = pair[0].time + dt * f;
            // No clip in the requested known-time window can hear this anchor.
            if let Some(t) = job.start_utc
                && (emit + first.slant_m / c - first.cpa_s - t).abs() > job.time_tolerance_s + 120.
            {
                continue;
            }
            let anchor_ground = terrain.elevation(plane);
            for scale in [0.5, 0.75, 1., 1.5, 2.] {
                let slant = (first.slant_m * scale).min(40000.);
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
                    let examined = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    if examined > job.max_hypotheses {
                        bail!(
                            "Indexed search exceeded the {} hypothesis budget; no partial location is reported. Narrow the region/date range, increase spacing, or raise the budget.",
                            job.max_hypotheses
                        );
                    }
                    if examined.is_multiple_of(256) {
                        control.check()?;
                    }
                    let expected = emit + slant / c;
                    let Some((arrival, range, actual_speed)) =
                        track.cpa_near(point.ecef(), c, Some((expected - 30., expected + 30.)))
                    else {
                        continue;
                    };
                    let start = arrival - first.cpa_s;
                    if !within_time(job, start) {
                        continue;
                    }
                    let score = feature_error(first, range, actual_speed);
                    if score > 12. {
                        continue;
                    }
                    if let Some(seed) =
                        complete_seed(point, start, anchor, id, score, events, |wanted, event| {
                            let radius = (event.slant_m * 2.5).clamp(5000., 40000.);
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
                            seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
                            seeds.truncate(2048);
                        }
                    }
                }
            }
        }
    }
    Ok(seeds)
}
