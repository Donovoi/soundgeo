use crate::{Control, audio::SAMPLE_RATE};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peak {
    pub time_s: f64,
    pub excess_db: f64,
    pub width_s: f64,
}

// Fourth-order Butterworth bandpass, 700..3500 Hz at 8000 Hz. These SOS
// coefficients are fixed by the sample rate and cutoffs, not by validation data.
const SOS: [[f64; 6]; 4] = [
    [
        0.2754132880723043,
        -0.5508265761446086,
        0.2754132880723043,
        1.,
        -1.0865583580236484,
        0.32118233372249133,
    ],
    [1., 2., 1., 1., 1.3159366114545423, 0.4500098064484538],
    [1., -2., 1., 1., -1.4350746753005692, 0.6905865784787869],
    [1., 2., 1., 1., 1.6255825411195965, 0.7633900238734381],
];

/// Extract broadband peaks using audio alone. No flight data enters detection.
pub fn detect(samples: &[f32], control: &Control) -> Result<Vec<Peak>> {
    control.check()?;
    let mut states = [[0.; 2]; 4];
    let frame = SAMPLE_RATE / 10;
    let mut powers = Vec::with_capacity(samples.len() / frame);
    for (index, samples) in samples.chunks_exact(frame).enumerate() {
        if index.is_multiple_of(128) {
            control.check()?;
        }
        let mut energy = 0.;
        for &sample in samples {
            let mut x = f64::from(sample);
            for (sos, state) in SOS.iter().zip(&mut states) {
                let y = sos[0] * x + state[0];
                state[0] = sos[1] * x - sos[4] * y + state[1];
                state[1] = sos[2] * x - sos[5] * y;
                x = y;
            }
            energy += x * x;
        }
        powers.push(energy / frame as f64);
    }
    if powers.len() < 3 {
        return Ok(Vec::new());
    }
    let mut kernel: Vec<_> = (-12..=12)
        .map(|n| (-0.5 * (n as f64 / 3.).powi(2)).exp())
        .collect();
    let total = kernel.iter().sum::<f64>();
    for weight in &mut kernel {
        *weight /= total;
    }
    let db: Vec<_> = (0..powers.len())
        .map(|i| {
            let power = kernel
                .iter()
                .enumerate()
                .map(|(j, weight)| {
                    powers[reflect(i as isize + j as isize - 12, powers.len())] * weight
                })
                .sum::<f64>();
            10. * power.max(1e-20).log10()
        })
        .collect();
    let mut excess = Vec::with_capacity(db.len());
    let mut window = Vec::with_capacity(601);
    for (i, value) in db.iter().enumerate() {
        if i.is_multiple_of(128) {
            control.check()?;
        }
        window.clear();
        window.extend(
            (-300..=300).map(|j| db[(i as isize + j).clamp(0, db.len() as isize - 1) as usize]),
        );
        let median = *window.select_nth_unstable_by(300, f64::total_cmp).1;
        excess.push(value - median);
    }
    Ok(find_peaks(&excess))
}

fn reflect(index: isize, len: usize) -> usize {
    let period = 2 * len as isize;
    let i = index.rem_euclid(period);
    if i < len as isize {
        i as usize
    } else {
        (period - i - 1) as usize
    }
}

fn find_peaks(values: &[f64]) -> Vec<Peak> {
    let mut peaks = Vec::new();
    let mut i = 1;
    while i + 1 < values.len() {
        if values[i] > values[i - 1] {
            let mut right = i;
            while right + 1 < values.len() && values[right + 1] == values[i] {
                right += 1;
            }
            if right + 1 < values.len() && values[right] > values[right + 1] {
                let middle = (i + right) / 2;
                if values[middle] >= 6. {
                    peaks.push(middle);
                }
            }
            i = right + 1;
        } else {
            i += 1;
        }
    }
    // Distance selection precedes prominence and width, matching the reference
    // detector. Keep the strongest peak in each 15-second neighborhood.
    let mut order = peaks.clone();
    order.sort_by(|a, b| values[*b].total_cmp(&values[*a]).then(b.cmp(a)));
    let mut retained = Vec::new();
    for index in order {
        if retained.iter().all(|&other| index.abs_diff(other) >= 150) {
            retained.push(index);
        }
    }
    retained.sort_unstable();
    retained
        .into_iter()
        .filter_map(|index| {
            let height = values[index];
            let (mut left, mut right) = (index, index);
            let (mut left_min, mut right_min) = (height, height);
            while left > 0 && values[left - 1] <= height {
                left -= 1;
                left_min = left_min.min(values[left]);
            }
            while right + 1 < values.len() && values[right + 1] <= height {
                right += 1;
                right_min = right_min.min(values[right]);
            }
            let prominence = height - left_min.max(right_min);
            if prominence < 6. {
                return None;
            }
            let level = height - prominence * 0.5;
            let (mut a, mut b) = (index, index);
            while a > left && values[a] > level {
                a -= 1;
            }
            while b < right && values[b] > level {
                b += 1;
            }
            let left_cross = if values[a] < level {
                a as f64 + (level - values[a]) / (values[a + 1] - values[a])
            } else {
                a as f64
            };
            let right_cross = if values[b] < level {
                b as f64 - (level - values[b]) / (values[b - 1] - values[b])
            } else {
                b as f64
            };
            let width = right_cross - left_cross;
            if !(5. ..=200.).contains(&width) {
                return None;
            }
            Some(Peak {
                time_s: index as f64 * 0.1 + 0.05,
                excess_db: height,
                width_s: width * 0.1,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timed_broadband_bursts_are_detected_without_flight_hints() {
        let samples: Vec<_> = (0..SAMPLE_RATE * 80)
            .map(|i| {
                let t = i as f64 / SAMPLE_RATE as f64;
                let amp = 0.001
                    + [20., 55.]
                        .iter()
                        .map(|center| (-0.5 * ((t - center) / 1.5).powi(2)).exp())
                        .sum::<f64>();
                (amp * (std::f64::consts::TAU * 1500. * t).sin()) as f32
            })
            .collect();
        let peaks = detect(&samples, &Control::default()).unwrap();
        assert_eq!(peaks.len(), 2);
        assert!((peaks[0].time_s - 20.).abs() < 0.2);
        assert!((peaks[1].time_s - 55.).abs() < 0.2);
    }
    #[test]
    fn silence_and_constant_power_are_not_passages_and_cancellation_is_honored() {
        assert!(
            detect(&vec![0.; SAMPLE_RATE * 30], &Control::default())
                .unwrap()
                .is_empty()
        );
        let samples: Vec<_> = (0..SAMPLE_RATE * 30)
            .map(|i| {
                (0.1 * (std::f64::consts::TAU * 1500. * i as f64 / SAMPLE_RATE as f64).sin()) as f32
            })
            .collect();
        assert!(detect(&samples, &Control::default()).unwrap().is_empty());
        let control = Control::default();
        control.stop();
        assert!(detect(&samples, &control).is_err());
    }
}
