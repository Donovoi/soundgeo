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

struct Visit {
    track: usize,
    enter: f64,
    leave: f64,
    distance: f64,
    segment: usize,
    arrival: f64,
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
        let mut visits = vec![];
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
                            let projection = -relative.dot(segment.velocity) / segment.speed2;
                            let u = projection.clamp(0., segment.duration);
                            let distance = (relative + segment.velocity * u).norm();
                            if distance > self.range {
                                continue;
                            }
                            let line_distance = (relative + segment.velocity * projection).norm();
                            let half = ((self.range.powi(2) - line_distance.powi(2)).max(0.)
                                / segment.speed2)
                                .sqrt();
                            visits.push(Visit {
                                track: segment.track,
                                enter: segment.time + (projection - half).max(0.),
                                leave: segment.time + (projection + half).min(segment.duration),
                                distance,
                                segment: index,
                                arrival: segment.time + u + distance / self.sound_speed,
                            });
                        }
                    }
                }
            }
        }
        visits.sort_by(|a, b| {
            a.track
                .cmp(&b.track)
                .then(a.enter.total_cmp(&b.enter))
                .then(a.segment.cmp(&b.segment))
        });
        let mut result = vec![];
        let mut current: Option<Visit> = None;
        for visit in visits {
            if let Some(old) = current.as_mut()
                && old.track == visit.track
                // Adjacent samples share an entry/exit time; allow floating-point rounding.
                && visit.enter <= old.leave + 1e-6
            {
                old.leave = old.leave.max(visit.leave);
                // The original sample order resolves an equal-distance endpoint.
                if visit.distance < old.distance
                    || (visit.distance == old.distance && visit.segment < old.segment)
                {
                    old.distance = visit.distance;
                    old.segment = visit.segment;
                    old.arrival = visit.arrival;
                }
            } else if let Some(old) = current.replace(visit) {
                result.push(Passage {
                    time: old.arrival,
                    track: old.track,
                });
            }
        }
        if let Some(old) = current {
            result.push(Passage {
                time: old.arrival,
                track: old.track,
            });
        }
        result.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.track.cmp(&b.track)));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{aircraft::AircraftKind, geo::Point, model::Sample};

    fn path(positions: &[(f64, f64, f64)]) -> Track {
        Track {
            id: "returning-aircraft".into(),
            kind: AircraftKind::Unknown,
            samples: positions
                .iter()
                .map(|&(time, x, y)| {
                    let mut sample = Sample::new(
                        time,
                        Point {
                            lat: 0.,
                            lon: 0.,
                            alt: 0.,
                        },
                    );
                    sample.xyz = Vec3(x, y, 100.);
                    sample
                })
                .collect(),
        }
    }

    #[test]
    fn returning_aircraft_keeps_both_passages_with_sound_delay() {
        let tracks = [path(&[
            (0., -1000., 0.),
            (10., 0., 0.),
            (20., 1000., 0.),
            (30., 1000., 1000.),
            (40., 0., 1000.),
            (50., -1000., 1000.),
            (60., -1000., 0.),
            (70., 0., 0.),
            (80., 1000., 0.),
        ])];
        let reference = Reference::new(&tracks, 500., 340., &Control::default()).unwrap();
        let passages = reference.passages(Vec3(0., 0., 0.));
        assert_eq!(passages.len(), 2);
        for (passage, time) in passages.iter().zip([10., 70.]) {
            assert_eq!(passage.track, 0);
            assert!((passage.time - time - 100. / 340.).abs() < 1e-9);
        }
    }

    #[test]
    fn adjacent_segments_inside_range_are_one_passage() {
        let tracks = [path(&[
            (0., -1000., 0.),
            (6., -400., 0.),
            (10., 0., 0.),
            (14., 400., 0.),
            (20., 1000., 0.),
        ])];
        let reference = Reference::new(&tracks, 500., 340., &Control::default()).unwrap();
        let passages = reference.passages(Vec3(0., 0., 0.));
        assert_eq!(passages.len(), 1);
        assert!((passages[0].time - 10. - 100. / 340.).abs() < 1e-9);
    }

    #[test]
    fn missing_positions_do_not_join_passages() {
        let tracks = [path(&[
            (0., -400., 0.),
            (4., 0., 0.),
            (8., 400., 0.),
            (100., -400., 0.),
            (104., 0., 0.),
            (108., 400., 0.),
        ])];
        let reference = Reference::new(&tracks, 500., 340., &Control::default()).unwrap();
        let passages = reference.passages(Vec3(0., 0., 0.));
        assert_eq!(passages.len(), 2);
        assert!((passages[1].time - passages[0].time - 100.).abs() < 1e-9);
    }

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
