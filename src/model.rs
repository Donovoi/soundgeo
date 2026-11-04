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

impl Track {
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
        let dt = s[hi].time - s[lo].time;
        if dt <= 0. || dt > 60. {
            return None;
        }
        let vel = (s[hi].xyz - s[lo].xyz) * (1. / dt);
        if vel.norm() >= c * 0.9 {
            return None;
        }
        let mut a = 0.;
        let mut b = dt;
        for _ in 0..22 {
            let m = (a + b) * 0.5;
            let t = s[lo].time + m + (s[lo].xyz + vel * m - observer).norm() / c;
            if t < arrival {
                a = m;
            } else {
                b = m;
            }
        }
        let r = s[lo].xyz + vel * ((a + b) * 0.5) - observer;
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
