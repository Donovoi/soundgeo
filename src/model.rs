use crate::geo::{Point, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub time: f64,
    pub point: Point,
    #[serde(skip)]
    pub xyz: Vec3,
}
impl Sample {
    pub fn new(time: f64, point: Point) -> Self {
        Self {
            time,
            point,
            xyz: point.ecef(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Track {
    pub id: String,
    pub samples: Vec<Sample>,
    pub kind: crate::aircraft::AircraftKind,
}

/// Arrival times depend on the observer and sound speed, not the recording clock.
/// Borrow the immutable track so cached geometry cannot outlive or mutate it.
pub(crate) struct PreparedTrack<'a> {
    track: &'a Track,
    observer: Vec3,
    sound_speed: f64,
    arrivals: Vec<f64>,
    segments: Vec<std::cell::Cell<Option<CachedSegment>>>,
}

#[derive(Clone, Copy)]
struct CachedSegment {
    index: usize,
    geometry: Option<PreparedSegment>,
}

/// Invariants of one segment at one observer. Initialize only segments that
/// the clock profile visits; long tracks need not prepare unrelated geometry.
#[derive(Clone, Copy)]
struct PreparedSegment {
    start: f64,
    duration: f64,
    velocity: Vec3,
    relative: Vec3,
    quadratic: f64,
    projection: f64,
    squared_delay: f64,
}

impl PreparedSegment {
    fn new(track: &Track, observer: Vec3, c: f64, lo: usize, hi: usize) -> Option<Self> {
        let s = &track.samples;
        let dt = s[hi].time - s[lo].time;
        if dt <= 0. || dt > 60. {
            return None;
        }
        let vel = (s[hi].xyz - s[lo].xyz) * (1. / dt);
        if vel.norm() >= c * 0.9 {
            return None;
        }
        let r0 = s[lo].xyz - observer;
        let c2 = c * c;
        Some(Self {
            start: s[lo].time,
            duration: dt,
            velocity: vel,
            relative: r0,
            quadratic: 1. - vel.dot(vel) / c2,
            projection: r0.dot(vel) / c2,
            squared_delay: r0.dot(r0) / c2,
        })
    }

    fn factor(&self, arrival: f64, c: f64) -> f64 {
        // Keep the unprepared calculation's floating-point operation order.
        let q = arrival - self.start;
        let k = q + self.projection;
        let discriminant = (k * k - self.quadratic * (q * q - self.squared_delay)).max(0.);
        let emission = ((k - discriminant.sqrt()) / self.quadratic).clamp(0., self.duration);
        let r = self.relative + self.velocity * emission;
        let radial = self.velocity.dot(r) / r.norm().max(1.);
        c / (c + radial)
    }
}

impl PreparedTrack<'_> {
    pub(crate) fn factor(&self, arrival: f64) -> Option<f64> {
        let times = &self.arrivals;
        if times.len() < 2 || arrival < times[0] || arrival > times[times.len() - 1] {
            return None;
        }
        let mut lo = 0;
        let mut hi = times.len() - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if times[mid] < arrival {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        // Bound storage independently of a long track's length. Collisions
        // merely recompute the same immutable geometry, including invalid gaps.
        let slot = &self.segments[lo % self.segments.len()];
        let segment = match slot.get() {
            Some(cached) if cached.index == lo => cached.geometry,
            _ => {
                let segment =
                    PreparedSegment::new(self.track, self.observer, self.sound_speed, lo, hi);
                slot.set(Some(CachedSegment {
                    index: lo,
                    geometry: segment,
                }));
                segment
            }
        };
        segment.map(|segment| segment.factor(arrival, self.sound_speed))
    }
}

impl Track {
    pub(crate) fn prepare(&self, observer: Vec3, sound_speed: f64) -> PreparedTrack<'_> {
        PreparedTrack {
            track: self,
            observer,
            sound_speed,
            arrivals: self
                .samples
                .iter()
                .map(|s| s.time + (s.xyz - observer).norm() / sound_speed)
                .collect(),
            segments: (0..self.samples.len().saturating_sub(1).min(64))
                .map(|_| std::cell::Cell::new(None))
                .collect(),
        }
    }
    /// Centered local linear regression suppresses quantized position noise
    /// before differentiating. Never adds points, extends a track or joins gaps.
    pub fn smooth_positions(&mut self) {
        let original = &self.samples;
        let mut smoothed = original.clone();
        for (i, sample) in original.iter().enumerate() {
            let from = original.partition_point(|s| s.time < sample.time - 5.);
            let to = original.partition_point(|s| s.time <= sample.time + 5.);
            if from == i || to <= i + 1 || to - from < 3 {
                continue;
            }
            let mut sx = 0.;
            let mut sxx = 0.;
            let mut sy = Vec3::default();
            let mut sxy = Vec3::default();
            for neighbor in &original[from..to] {
                let x = neighbor.time - sample.time;
                // Fit in local geodetic coordinates; unwrap longitude at the seam.
                let y = Vec3(
                    neighbor.point.lat - sample.point.lat,
                    (neighbor.point.lon - sample.point.lon + 180.).rem_euclid(360.) - 180.,
                    neighbor.point.alt - sample.point.alt,
                );
                sx += x;
                sxx += x * x;
                sy = sy + y;
                sxy = sxy + y * x;
            }
            let n = (to - from) as f64;
            let denominator = n * sxx - sx * sx;
            if denominator <= 1e-9 {
                continue;
            }
            let adjustment = (sy * sxx - sxy * sx) * (1. / denominator);
            smoothed[i] = Sample::new(
                sample.time,
                Point {
                    lat: sample.point.lat + adjustment.0,
                    lon: (sample.point.lon + adjustment.1 + 180.).rem_euclid(360.) - 180.,
                    alt: sample.point.alt + adjustment.2,
                },
            );
        }
        self.samples = smoothed;
    }
    /// Solve arrival(t_emit)=t_emit+range(t_emit)/c, then use velocity at emission.
    /// No extrapolation across track boundaries or missing-data gaps.
    pub fn factor(&self, observer: Vec3, arrival: f64, c: f64) -> Option<f64> {
        let s = &self.samples;
        let arrival_at = |i: usize| s[i].time + (s[i].xyz - observer).norm() / c;
        if s.len() < 2 || arrival < arrival_at(0) || arrival > arrival_at(s.len() - 1) {
            return None;
        }
        let mut lo = 0;
        let mut hi = s.len() - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if arrival_at(mid) < arrival {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        self.factor_in_segment(observer, arrival, c, lo, hi)
    }
    fn factor_in_segment(
        &self,
        observer: Vec3,
        arrival: f64,
        c: f64,
        lo: usize,
        hi: usize,
    ) -> Option<f64> {
        let s = &self.samples;
        let dt = s[hi].time - s[lo].time;
        if dt <= 0. || dt > 60. {
            return None;
        }
        let vel = (s[hi].xyz - s[lo].xyz) * (1. / dt);
        if vel.norm() >= c * 0.9 {
            return None;
        }
        // Exact retarded time on this constant-velocity segment. The earlier
        // quadratic root is the causal solution (emission precedes arrival).
        // This replaces 22 square-root bisection steps per ridge sample.
        let r0 = s[lo].xyz - observer;
        let q = arrival - s[lo].time;
        let c2 = c * c;
        let a = 1. - vel.dot(vel) / c2;
        let k = q + r0.dot(vel) / c2;
        let discriminant = (k * k - a * (q * q - r0.dot(r0) / c2)).max(0.);
        let emission = ((k - discriminant.sqrt()) / a).clamp(0., dt);
        let r = r0 + vel * emission;
        let radial = vel.dot(r) / r.norm().max(1.);
        Some(c / (c + radial))
    }
    pub fn cpa(&self, observer: Vec3, c: f64) -> Option<(f64, f64, f64)> {
        self.cpa_near(observer, c, None)
    }
    pub fn cpa_near(
        &self,
        observer: Vec3,
        c: f64,
        window: Option<(f64, f64)>,
    ) -> Option<(f64, f64, f64)> {
        let mut best = None;
        let mut min = f64::INFINITY;
        // Acoustic candidates are limited to 40 km (<150 s travel time).
        // Index the emission window before examining segments, including neighbors.
        let (from, to) = if let Some((a, b)) = window {
            (
                self.samples
                    .partition_point(|s| s.time < a - 150.)
                    .saturating_sub(1),
                (self.samples.partition_point(|s| s.time <= b) + 1).min(self.samples.len()),
            )
        } else {
            (0, self.samples.len())
        };
        if from >= to {
            return None;
        }
        for pair in self.samples[from..to].windows(2) {
            let dt = pair[1].time - pair[0].time;
            if dt <= 0. || dt > 60. {
                continue;
            }
            let delta = pair[1].xyz - pair[0].xyz;
            let f =
                ((observer - pair[0].xyz).dot(delta) / delta.dot(delta).max(1e-9)).clamp(0., 1.);
            let range = (pair[0].xyz + delta * f - observer).norm();
            let arrival = pair[0].time + dt * f + range / c;
            if window.is_some_and(|(a, b)| arrival < a || arrival > b) {
                continue;
            }
            if range < min {
                min = range;
                best = Some((pair[0].time + dt * f + range / c, range, delta.norm() / dt));
            }
        }
        best
    }
}

/// Constant-speed straight flyover, parameterized by the RECEIVED CPA time.
pub fn straight_factor(t: f64, cpa: f64, speed: f64, distance: f64, c: f64) -> f64 {
    let q = t - cpa + distance / c;
    let a = 1. - (speed / c).powi(2);
    let emission = (q - ((1. - a) * q * q + a * (distance / c).powi(2)).sqrt()) / a;
    let range = (distance * distance + (speed * emission).powi(2)).sqrt();
    c / (c + speed * speed * emission / range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_arrivals_preserve_exact_factors_and_coverage() {
        let mut tracks = crate::demo::tracks();
        let mut gap = tracks[0].clone();
        let middle = gap.samples.len() / 2;
        for sample in &mut gap.samples[middle..] {
            sample.time += 100.;
        }
        tracks.push(gap);
        let mut short = tracks[0].clone();
        short.samples.truncate(1);
        tracks.push(short.clone());
        short.samples.clear();
        tracks.push(short);
        for track in &tracks {
            for c in [310., 340.39, 360.] {
                for observer in [
                    crate::demo::TRUTH.ecef(),
                    Point {
                        lat: crate::demo::TRUTH.lat + 0.02,
                        lon: crate::demo::TRUTH.lon - 0.01,
                        alt: 750.,
                    }
                    .ecef(),
                ] {
                    let prepared = track.prepare(observer, c);
                    let mut arrivals = vec![crate::demo::EPOCH - 100., crate::demo::EPOCH + 2000.];
                    for sample in &track.samples {
                        let time = sample.time + (sample.xyz - observer).norm() / c;
                        arrivals.extend([time - 0.01, time, time + 0.01]);
                    }
                    for pair in track.samples.windows(2) {
                        let midpoint = (pair[0].time + pair[1].time) / 2.;
                        arrivals.push(midpoint + (pair[0].xyz - observer).norm() / c);
                    }
                    for arrival in arrivals {
                        assert_eq!(
                            prepared.factor(arrival).map(f64::to_bits),
                            track.factor(observer, arrival, c).map(f64::to_bits),
                            "Prepared geometry changed a boundary, coverage gap or Doppler value"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn prepared_segments_keep_exact_results_after_cache_collisions() {
        let truth = crate::demo::TRUTH;
        let track = Track {
            id: "CACHE_COLLISIONS".into(),
            kind: crate::aircraft::AircraftKind::Propeller,
            samples: (0..260)
                .map(|i| {
                    // A missing interval and an invalid speed share cache slots
                    // with valid geometry, and must still return no prediction.
                    let time = crate::demo::EPOCH + 2. * i as f64 + if i >= 65 { 75. } else { 0. };
                    let east = 100. * i as f64 + if i >= 129 { 1200. } else { 0. };
                    Sample::new(
                        time,
                        Point {
                            lat: truth.lat + 600. / 111320.,
                            lon: truth.lon + east / (111320. * truth.lat.to_radians().cos()),
                            alt: 1200.,
                        },
                    )
                })
                .collect(),
        };
        for c in [310., 340.39, 360.] {
            for observer in [
                truth.ecef(),
                Point {
                    lat: truth.lat + 0.01,
                    ..truth
                }
                .ecef(),
            ] {
                let prepared = track.prepare(observer, c);
                for round in 0..3 {
                    for offset in 0..64 {
                        for group in 0..4 {
                            let index = offset + 64 * ((group + round) % 4);
                            let a = prepared.arrivals[index];
                            let b = prepared.arrivals[index + 1];
                            for fraction in [0.1, 0.5, 0.9] {
                                let arrival = a + fraction * (b - a);
                                let expected = track.factor(observer, arrival, c);
                                assert_eq!(
                                    prepared.factor(arrival).map(f64::to_bits),
                                    expected.map(f64::to_bits),
                                    "A replaced cache entry changed segment {index}"
                                );
                                if index == 64 || index == 128 {
                                    assert!(expected.is_none());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
