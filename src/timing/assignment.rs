//! Maximum-weight one-to-one acoustic/flight assignment, including close peaks.
use super::reference::Passage;

pub(super) fn assign(peaks: &[f64], reference: &[Passage], offset: f64) -> Vec<(usize, usize)> {
    let mut rows = vec![];
    let mut columns = vec![];
    let mut independent = vec![];
    for (i, peak) in peaks.iter().enumerate() {
        let a = reference.partition_point(|r| r.time < offset + peak - 3.);
        let b = reference.partition_point(|r| r.time <= offset + peak + 3.);
        if a == b {
            continue;
        }
        let best = (a..b)
            .min_by(|&j, &k| {
                (reference[j].time - offset - peak)
                    .abs()
                    .total_cmp(&(reference[k].time - offset - peak).abs())
            })
            .unwrap();
        independent.push((i, best));
        rows.push(i);
        columns.extend(a..b);
    }
    let mut used: Vec<_> = independent.iter().map(|p| p.1).collect();
    used.sort_unstable();
    if !used.windows(2).any(|p| p[0] == p[1]) {
        return independent;
    }
    columns.sort_unstable();
    columns.dedup();
    // Dummy columns allow every acoustic peak to remain unmatched at zero weight.
    let costs: Vec<Vec<_>> = rows
        .iter()
        .map(|&i| {
            columns
                .iter()
                .map(|&j| {
                    let delta = reference[j].time - peaks[i] - offset;
                    if delta.abs() <= 3. {
                        -(-0.5 * delta * delta).exp()
                    } else {
                        0.
                    }
                })
                .chain(std::iter::repeat_n(0., rows.len()))
                .collect()
        })
        .collect();
    hungarian(&costs)
        .into_iter()
        .enumerate()
        .filter(|&(i, j)| j < columns.len() && costs[i][j] < 0.)
        .map(|(i, j)| (rows[i], columns[j]))
        .collect()
}

fn hungarian(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    if n == 0 {
        return vec![];
    }
    let m = cost[0].len();
    let (mut u, mut v, mut p, mut way) = (
        vec![0.; n + 1],
        vec![0.; m + 1],
        vec![0usize; m + 1],
        vec![0usize; m + 1],
    );
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0;
        let mut minimum = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let (mut delta, mut j1) = (f64::INFINITY, 0);
            for j in 1..=m {
                if !used[j] {
                    let current = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if current < minimum[j] {
                        minimum[j] = current;
                        way[j] = j0;
                    }
                    if minimum[j] < delta {
                        delta = minimum[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minimum[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut assignment = vec![usize::MAX; n];
    for j in 1..=m {
        if p[j] != 0 {
            assignment[p[j] - 1] = j - 1;
        }
    }
    assignment
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_peaks_need_global_assignment_not_greedy_or_noncrossing_matching() {
        let refs = [
            Passage { time: 2., track: 0 },
            Passage {
                time: 2.5,
                track: 1,
            },
        ];
        assert_eq!(assign(&[0., 0.5], &refs, 0.), vec![(0, 1), (1, 0)]);
        assert_eq!(assign(&[0., 0.5, 20.], &refs, 0.), vec![(0, 1), (1, 0)]);
    }
    #[test]
    fn assignment_matches_exhaustive_independent_oracle() {
        fn enumerate(cost: &[Vec<f64>], row: usize, used: u32, total: f64, best: &mut f64) {
            if row == cost.len() {
                *best = best.min(total);
                return;
            }
            for j in 0..cost[row].len() {
                if used & (1 << j) == 0 {
                    enumerate(cost, row + 1, used | (1 << j), total + cost[row][j], best);
                }
            }
        }
        for seed in 0..40 {
            let cost: Vec<Vec<f64>> = (0..4)
                .map(|i| {
                    (0..6)
                        .map(|j| -(((i * 37 + j * 19 + seed * 13 + i * j * seed) % 101) as f64))
                        .collect()
                })
                .collect();
            let result = hungarian(&cost);
            let score: f64 = result.iter().enumerate().map(|(i, &j)| cost[i][j]).sum();
            let mut best = f64::INFINITY;
            enumerate(&cost, 0, 0, 0., &mut best);
            assert_eq!(score, best);
        }
    }
}
