//! Whole-activity controls for investigating partial Doppler matches.
//! These controls do not establish exchangeability or a location probability.
use crate::audio::Event;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

mod replay;
pub use replay::{SearchBinding, bind_search, replay_observations};

const CONTROL_COUNT: usize = 19;
const MIN_ACTIVITY_GAP_S: f64 = 30.;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub start_s: f64,
    pub end_s: f64,
    pub event_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Permutation {
    pub seed: u64,
    /// Original block indices in their new chronological order.
    pub order: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub duration_s: f64,
    pub minimum_activity_gap_s: f64,
    pub blocks: Vec<Block>,
    pub controls: Vec<Permutation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventShift {
    pub original_index: usize,
    pub new_index: usize,
    pub block_index: usize,
    pub seconds: f64,
}

fn activity_bounds(event: &Event, duration: f64) -> Result<(f64, f64)> {
    let mut start = event.activity_start_s();
    let mut end = event.activity_end_s();
    ensure!(
        start.is_finite() && end.is_finite() && start <= end,
        "Invalid activity interval"
    );
    for (index, variant) in event.variants().enumerate() {
        ensure!(
            index == 0 || variant.alternatives.is_empty(),
            "Nested observation alternatives are unsupported"
        );
        ensure!(
            variant.start_s.is_finite()
                && variant.end_s.is_finite()
                && variant.cpa_s.is_finite()
                && variant.start_s <= variant.end_s,
            "Invalid observation interval"
        );
        ensure!(
            variant.activity_start_s().is_finite()
                && variant.activity_end_s().is_finite()
                && variant.activity_start_s() <= variant.activity_end_s(),
            "Invalid alternative activity interval"
        );
        start = start.min(variant.start_s).min(variant.activity_start_s());
        end = end.max(variant.end_s).max(variant.activity_end_s());
        for point in &variant.ridge {
            ensure!(point.time_s.is_finite(), "Invalid ridge observation time");
            start = start.min(point.time_s);
            end = end.max(point.time_s);
        }
    }
    ensure!(
        start >= 0. && end <= duration,
        "Observation extends outside the recording"
    );
    Ok((start, end))
}

fn activity_blocks(events: &[Event], duration: f64) -> Result<Vec<Block>> {
    ensure!(
        duration.is_finite() && duration > 0.,
        "Recording duration must be positive and finite"
    );
    let mut intervals = events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            activity_bounds(event, duration).map(|(start, end)| (start, end, index))
        })
        .collect::<Result<Vec<_>>>()?;
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
    let mut groups: Vec<Block> = vec![];
    for (start, end, index) in intervals {
        if let Some(previous) = groups.last_mut()
            && start - previous.end_s < MIN_ACTIVITY_GAP_S
        {
            previous.end_s = previous.end_s.max(end);
            previous.event_indices.push(index);
        } else {
            groups.push(Block {
                start_s: start,
                end_s: end,
                event_indices: vec![index],
            });
        }
    }
    ensure!(
        groups.len() >= 5,
        "Doppler controls need at least five activity blocks with 30-second gaps between detections"
    );
    // Cut between detected activities. Every block contains complete activity, including
    // connected fragments, overlapping detections and all retained alternatives.
    let cuts: Vec<_> = groups
        .windows(2)
        .map(|w| (w[0].end_s + w[1].start_s) * 0.5)
        .collect();
    for (index, block) in groups.iter_mut().enumerate() {
        block.start_s = index.checked_sub(1).map_or(0., |i| cuts[i]);
        block.end_s = cuts.get(index).copied().unwrap_or(duration);
    }
    Ok(groups)
}

fn shuffle(size: usize, mut state: u64) -> Vec<usize> {
    let mut order: Vec<_> = (0..size).collect();
    for i in (1..size).rev() {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        order.swap(i, (z % (i as u64 + 1)) as usize);
    }
    order
}

fn is_rotation(order: &[usize]) -> bool {
    order
        .iter()
        .enumerate()
        .all(|(i, &value)| value == (order[0] + i) % order.len())
}

pub fn plan(events: &[Event], duration: f64) -> Result<Plan> {
    let blocks = activity_blocks(events, duration)?;
    let mut used = HashSet::new();
    let mut controls = vec![];
    for seed in 202611090..202621090 {
        let order = shuffle(blocks.len(), seed);
        // An unknown recording clock can absorb a common shift. Do not use
        // identity or cyclic block orders as evidence against the original fit.
        let mut canonical = order.clone();
        canonical.rotate_left(order.iter().position(|&i| i == 0).unwrap());
        if !is_rotation(&order) && used.insert(canonical) {
            controls.push(Permutation { seed, order });
        }
        if controls.len() == CONTROL_COUNT {
            return Ok(Plan {
                duration_s: duration,
                minimum_activity_gap_s: MIN_ACTIVITY_GAP_S,
                blocks,
                controls,
            });
        }
    }
    anyhow::bail!("Could not construct 19 distinct whole-activity controls")
}

fn shift(event: &mut Event, seconds: f64) {
    event.start_s += seconds;
    event.end_s += seconds;
    event.cpa_s += seconds;
    for point in &mut event.ridge {
        point.time_s += seconds;
    }
    if let Some(span) = &mut event.activity_span_s {
        span[0] += seconds;
        span[1] += seconds;
    }
    for alternative in &mut event.alternatives {
        shift(alternative, seconds);
    }
}

fn same_time(a: f64, b: f64) -> bool {
    a.is_finite()
        && b.is_finite()
        && (a - b).abs() <= 4. * f64::EPSILON * a.abs().max(b.abs()).max(1.)
}

pub fn apply(
    events: &[Event],
    duration_s: f64,
    saved: &Plan,
    index: usize,
) -> Result<(Vec<Event>, Vec<EventShift>)> {
    // JSON decoding can round a block boundary by one floating-point step.
    // Always derive the actual shifts from the source report again.
    let plan = self::plan(events, duration_s)?;
    ensure!(
        same_time(saved.duration_s, plan.duration_s)
            && saved.minimum_activity_gap_s == plan.minimum_activity_gap_s
            && saved.controls == plan.controls
            && saved.blocks.len() == plan.blocks.len()
            && saved.blocks.iter().zip(&plan.blocks).all(|(a, b)| {
                a.event_indices == b.event_indices
                    && same_time(a.start_s, b.start_s)
                    && same_time(a.end_s, b.end_s)
            }),
        "Control plan differs from the source activity blocks"
    );
    let permutation = plan
        .controls
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Control index must be between 0 and 18"))?;
    let mut output = vec![];
    let mut cursor = 0.;
    for &block_index in &permutation.order {
        let block = &plan.blocks[block_index];
        let seconds = cursor - block.start_s;
        for &original_index in &block.event_indices {
            let mut event = events[original_index].clone();
            shift(&mut event, seconds);
            output.push((
                event,
                EventShift {
                    original_index,
                    new_index: 0,
                    block_index,
                    seconds,
                },
            ));
        }
        cursor += block.end_s - block.start_s;
    }
    output.sort_by(|a, b| {
        a.0.start_s
            .total_cmp(&b.0.start_s)
            .then_with(|| a.1.original_index.cmp(&b.1.original_index))
    });
    for (index, (_, mapping)) in output.iter_mut().enumerate() {
        mapping.new_index = index;
    }
    Ok(output.into_iter().unzip())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::RidgePoint;

    fn event(start: f64, end: f64) -> Event {
        Event {
            start_s: start,
            end_s: end,
            cpa_s: (start + end) / 2.,
            base_hz: 100.,
            speed_mps: 70.,
            slant_m: 1000.,
            fit_rmse_hz: 0.1,
            contrast_db: 12.,
            ridge: vec![
                RidgePoint {
                    time_s: start,
                    hz: 101.,
                    contrast_db: 10.,
                },
                RidgePoint {
                    time_s: end,
                    hz: 99.,
                    contrast_db: 15.,
                },
            ],
            activity_span_s: None,
            alternatives: vec![],
        }
    }
    fn fixture() -> Vec<Event> {
        vec![
            event(20.1, 40.2),
            event(160.3, 180.4),
            event(320.5, 340.6),
            event(500.7, 520.8),
            event(700.7, 720.8),
        ]
    }

    #[test]
    fn whole_curves_and_overlapping_activity_survive_every_control() {
        let mut events = fixture();
        events[0].activity_span_s = Some([10., 90.]);
        events[0].alternatives.push(event(50., 80.));
        events.push(event(75., 100.));
        let original = serde_json::to_value(&events).unwrap();
        let plan = plan(&events, 900.75).unwrap();
        let plan: Plan = serde_json::from_slice(&serde_json::to_vec(&plan).unwrap()).unwrap();
        assert_eq!(plan.blocks.len(), 5);
        for index in 0..CONTROL_COUNT {
            let (changed, shifts) = apply(&events, 900.75, &plan, index).unwrap();
            assert_eq!(changed.len(), events.len());
            let a = shifts.iter().find(|s| s.original_index == 0).unwrap();
            let b = shifts.iter().find(|s| s.original_index == 5).unwrap();
            assert_eq!(a.seconds, b.seconds);
            for mapping in shifts {
                let new = &changed[mapping.new_index];
                let old = &events[mapping.original_index];
                assert!(new.activity_start_s() >= -1e-9 && new.activity_end_s() <= 900.75 + 1e-9);
                assert!((new.cpa_s - old.cpa_s - mapping.seconds).abs() < 1e-9);
                assert_eq!(new.variants().count(), old.variants().count());
                for (new, old) in new.variants().zip(old.variants()) {
                    assert_eq!(new.ridge.len(), old.ridge.len());
                    assert_eq!(new.base_hz, old.base_hz);
                    assert_eq!(new.fit_rmse_hz, old.fit_rmse_hz);
                    for (a, b) in new.ridge.iter().zip(&old.ridge) {
                        assert_eq!(a.hz, b.hz);
                        assert_eq!(a.contrast_db, b.contrast_db);
                        assert!((a.time_s - b.time_s - mapping.seconds).abs() < 1e-9);
                    }
                }
            }
        }
        assert_eq!(serde_json::to_value(&events).unwrap(), original);
    }

    #[test]
    fn distinct_controls_change_more_than_an_unknown_clock_offset() {
        let events = fixture();
        let plan = plan(&events, 900.).unwrap();
        assert_eq!(plan, self::plan(&events, 900.).unwrap());
        assert_eq!(plan.controls.len(), 19);
        assert_eq!(
            plan.controls
                .iter()
                .map(|c| &c.order)
                .collect::<HashSet<_>>()
                .len(),
            19
        );
        assert!(plan.controls.iter().all(|c| !is_rotation(&c.order)));
        for index in 0..19 {
            let (_, shifts) = apply(&events, 900., &plan, index).unwrap();
            assert!(
                shifts
                    .iter()
                    .any(|s| (s.seconds - shifts[0].seconds).abs() > 1.)
            );
        }
    }

    #[test]
    fn controls_remain_distinct_after_clock_rotation() {
        let events = fixture();
        let plan = plan(&events, 800.).unwrap();
        for (index, a) in plan.controls.iter().enumerate() {
            for b in &plan.controls[index + 1..] {
                assert!(
                    !(0..a.order.len()).any(|offset| {
                        a.order
                            .iter()
                            .enumerate()
                            .all(|(i, value)| *value == b.order[(i + offset) % b.order.len()])
                    }),
                    "Two controls differ only by cyclic rotation"
                );
            }
        }
    }

    #[test]
    fn saved_real_activity_plan_can_be_reloaded() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/doppler-control-activities.json"
        ))
        .unwrap();
        let duration = fixture["duration_s"].as_f64().unwrap();
        let events: Vec<_> = fixture["activities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| event(v[0].as_f64().unwrap(), v[1].as_f64().unwrap()))
            .collect();
        let original = plan(&events, duration).unwrap();
        assert_eq!(original.blocks.len(), 15);
        let saved: Plan = serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
        for index in 0..CONTROL_COUNT {
            let original = apply(&events, duration, &original, index).unwrap();
            let reloaded = apply(&events, duration, &saved, index).unwrap();
            assert_eq!(
                serde_json::to_value(&original).unwrap(),
                serde_json::to_value(&reloaded).unwrap()
            );
        }
    }

    #[test]
    fn insufficient_blocks_and_invalid_or_changed_inputs_are_rejected() {
        let mut events = fixture();
        let plan = plan(&events, 900.).unwrap();
        assert!(self::plan(&events[..4], 900.).is_err());
        assert!(self::plan(&events, f64::NAN).is_err());
        assert!(apply(&events, 900., &plan, 19).is_err());
        let changed_duration = self::plan(&events, 901.).unwrap();
        assert!(apply(&events, 900., &changed_duration, 0).is_err());
        let mut changed_boundary = plan.clone();
        changed_boundary.blocks[0].end_s += 1.;
        assert!(apply(&events, 900., &changed_boundary, 0).is_err());
        assert!(apply(&events, f64::NAN, &plan, 0).is_err());
        events[0].alternatives.push(event(45., 140.));
        assert!(self::plan(&events, 900.).is_err());
        assert!(apply(&events, 900., &plan, 0).is_err());
        events = fixture();
        events[0].ridge[0].time_s = -1.;
        assert!(self::plan(&events, 900.).is_err());
    }
}
