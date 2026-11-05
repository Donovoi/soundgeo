use crate::{Control, geo::Vec3, model::Track};
use anyhow::{Result, ensure};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub(super) struct Passage {
    pub time: f64,
    pub track: usize,
}

struct Segment {
    start: Vec3,
    velocity: Vec3,
    speed2: f64,
    midpoint: Vec3,
    time: f64,
    duration: f64,
    track: usize,
}
struct Bucket {
    radius: f64,
    cells: HashMap<(i64, i64, i64), Vec<usize>>,
}
impl Bucket {
    fn cell(&self, p: Vec3) -> (i64, i64, i64) {
        (
            (p.0 / self.radius).floor() as i64,
            (p.1 / self.radius).floor() as i64,
            (p.2 / self.radius).floor() as i64,
        )
    }
}

pub(super) struct Reference {
    segments: Vec<Segment>,
    buckets: Vec<Bucket>,
    range: f64,
    sound_speed: f64,
    pub start_day: f64,
    pub end_day: f64,
}
impl Reference {
    pub fn new(tracks: &[Track], range: f64, sound_speed: f64, control: &Control) -> Result<Self> {
        ensure!(
            range.is_finite() && range > 0. && sound_speed.is_finite() && sound_speed > 0.,
            "Invalid passage geometry options"
        );
        let limits = [250., 500., 1000., 2000., 4000., 8000., 16000.];
        let mut result = Self {
            segments: vec![],
            buckets: limits
                .iter()
                .map(|limit| Bucket {
                    radius: range + limit,
                    cells: HashMap::new(),
                })
                .collect(),
            range,
            sound_speed,
            start_day: f64::INFINITY,
            end_day: f64::NEG_INFINITY,
        };
        for (track, path) in tracks.iter().enumerate() {
            control.check()?;
            for pair in path.samples.windows(2) {
                let duration = pair[1].time - pair[0].time;
                if !(0. < duration && duration <= 60.) {
                    continue;
                }
                let difference = pair[1].xyz - pair[0].xyz;
                let velocity = difference * (1. / duration);
                let speed2 = velocity.dot(velocity);
                if !(625. ..(sound_speed * 0.9).powi(2)).contains(&speed2) {
                    continue;
                }
                let half = difference.norm() * 0.5;
                let bucket = limits
                    .iter()
                    .position(|limit| half <= *limit)
                    .ok_or_else(|| anyhow::anyhow!("Flight segment exceeds timing-index limits"))?;
                let midpoint = (pair[0].xyz + pair[1].xyz) * 0.5;
                let cell = result.buckets[bucket].cell(midpoint);
                result.buckets[bucket]
                    .cells
                    .entry(cell)
                    .or_default()
                    .push(result.segments.len());
                result.segments.push(Segment {
                    start: pair[0].xyz,
                    velocity,
                    speed2,
                    midpoint,
                    time: pair[0].time,
                    duration,
                    track,
                });
                let day = (pair[0].time / 86400.).floor() * 86400.;
                result.start_day = result.start_day.min(day);
                result.end_day = result.end_day.max(day + 86400.);
            }
        }
        Ok(result)
    }

    pub fn passages(&self, observer: Vec3) -> Vec<Passage> {
        let mut nearest: HashMap<usize, (f64, usize, f64)> = HashMap::new();
        for bucket in &self.buckets {
            let (cx, cy, cz) = bucket.cell(observer);
            for x in cx - 1..=cx + 1 {
                for y in cy - 1..=cy + 1 {
                    for z in cz - 1..=cz + 1 {
                        let Some(indices) = bucket.cells.get(&(x, y, z)) else {
                            continue;
                        };
                        for &index in indices {
                            let segment = &self.segments[index];
                            if (segment.midpoint - observer).norm() > bucket.radius {
                                continue;
                            }
                            let relative = segment.start - observer;
                            let u = (-relative.dot(segment.velocity) / segment.speed2)
                                .clamp(0., segment.duration);
                            let distance = (relative + segment.velocity * u).norm();
                            if distance > self.range {
                                continue;
                            }
                            let old = nearest.entry(segment.track).or_insert((
                                f64::INFINITY,
                                usize::MAX,
                                0.,
                            ));
                            // The original sample order resolves an equal-distance endpoint.
                            if distance < old.0 || (distance == old.0 && index < old.1) {
                                *old = (
                                    distance,
                                    index,
                                    segment.time + u + distance / self.sound_speed,
                                );
                            }
                        }
                    }
                }
            }
        }
        let mut result: Vec<_> = nearest
            .into_iter()
            .map(|(track, (_, _, time))| Passage { time, track })
            .collect();
        result.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.track.cmp(&b.track)));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{aircraft::AircraftKind, geo::Point, model::Sample};
    #[test]
    fn indexed_passage_matches_all_segments_and_preserves_sound_delay() {
        let observer = Point {
            lat: 41.,
            lon: -87.,
            alt: 100.,
        }
        .ecef();
        let mut tracks = vec![];
        for i in 0..70 {
            let shift = (i as f64 - 35.) * 100.;
            let samples = (0..12)
                .map(|j| {
                    let mut sample = Sample::new(
                        10000. + j as f64 * 5.,
                        Point {
                            lat: 41.,
                            lon: -87.,
                            alt: 100.,
                        },
                    );
                    sample.xyz = observer + Vec3(shift, j as f64 * 250. - 1300., 50.);
                    sample
                })
                .collect();
            tracks.push(Track {
                id: i.to_string(),
                samples,
                kind: AircraftKind::Unknown,
            });
        }
        let reference = Reference::new(&tracks, 500., 340., &Control::default()).unwrap();
        let indexed = reference.passages(observer);
        assert_eq!(indexed.len(), 9);
        for passage in indexed {
            let distance = (((passage.track as f64 - 35.) * 100.).powi(2) + 2500.).sqrt();
            assert!((passage.time - (10026. + distance / 340.)).abs() < 1e-9);
        }
        let control = Control::default();
        control.stop();
        assert!(Reference::new(&tracks, 500., 340., &control).is_err());
    }
}
