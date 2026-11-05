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
) -> Result<Vec<Seed>> {
    control.check()?;
    let c = job.sound_speed();
    let mut seeds = vec![];
    for first in events[anchor].variants() {
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
                        if score > FEATURE_ERROR_LIMIT {
                            continue;
                        }
                        if let Some(seed) = complete_seed(
                            point,
                            start,
                            anchor,
                            id,
                            score,
                            events,
                            |wanted, event| {
                                let radius =
                                    (event.slant_m.max(100.) * range_factor).clamp(5000., 40000.);
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
                            },
                        ) {
                            seeds.push(seed);
                            // Bound each worker's retained hypotheses as well as
                            // the global beam, even for unusually long tracks.
                            if seeds.len() >= 8192 {
                                score_and_prune_seeds(
                                    &mut seeds, 2048, job, events, tracks, control,
                                )?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(seeds)
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
