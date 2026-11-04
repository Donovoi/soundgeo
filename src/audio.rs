use crate::{Control, Job, Progress, model::straight_factor};
use anyhow::{Context, Result, bail};
use rustfft::{FftPlanner, num_complex::Complex};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const SAMPLE_RATE: usize = 8000;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RidgePoint {
    pub time_s: f64,
    pub hz: f64,
    pub contrast_db: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub start_s: f64,
    pub end_s: f64,
    pub cpa_s: f64,
    pub base_hz: f64,
    pub speed_mps: f64,
    pub slant_m: f64,
    pub fit_rmse_hz: f64,
    pub contrast_db: f64,
    pub ridge: Vec<RidgePoint>,
    /// Full activity interval when adjacent fragments were counted as one sound.
    /// Matching still uses only the representative fitted ridge above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_span_s: Option<[f64; 2]>,
}
impl Event {
    pub fn activity_start_s(&self) -> f64 {
        self.activity_span_s.map_or(self.start_s, |span| span[0])
    }
    pub fn activity_end_s(&self) -> f64 {
        self.activity_span_s.map_or(self.end_s, |span| span[1])
    }
}

/// Residual of a freely fitted straight frequency drift, evaluated on the same
/// samples as a physical model. Centered online sums avoid cancellation for
/// short events late in a long recording.
pub(crate) fn linear_baseline_rmse(points: impl Iterator<Item = (f64, f64)>) -> f64 {
    let (mut n, mut mx, mut my, mut xx, mut xy, mut yy) = (0., 0., 0., 0., 0., 0.);
    for (x, y) in points {
        n += 1.;
        let dx = x - mx;
        let dy = y - my;
        mx += dx / n;
        my += dy / n;
        xx += dx * (x - mx);
        xy += dx * (y - my);
        yy += dy * (y - my);
    }
    if n < 3. || xx <= 0. {
        return 0.;
    }
    ((yy - xy * xy / xx).max(0.) / n).sqrt()
}

pub fn ffmpeg_path(job: &Job) -> PathBuf {
    if let Some(path) = &job.ffmpeg {
        return path.clone();
    }
    if let Ok(exe) = std::env::current_exe() {
        let local = exe.with_file_name(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        if local.is_file() {
            return local;
        }
    }
    PathBuf::from("ffmpeg")
}
pub fn quiet_command(path: impl AsRef<std::ffi::OsStr>) -> Command {
    let cmd = Command::new(path);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(0x08000000);
        cmd
    };
    cmd
}
pub fn decode(job: &Job, control: &Control) -> Result<Vec<f32>> {
    let temp = tempfile::tempdir()?;
    let wav = temp.path().join("audio.wav");
    let log = temp.path().join("ffmpeg.log");
    let stderr = std::fs::File::create(&log)?;
    // Decode 1 sample beyond the cap so long recordings are rejected, never silently truncated.
    let mut child=quiet_command(ffmpeg_path(job)).args(["-nostdin","-hide_banner","-loglevel","error","-y","-i"])
        .arg(&job.media).args(["-map","0:a:0","-vn","-ac","1","-ar","8000","-t","7200.001","-c:a","pcm_f32le"])
        .arg(&wav).stdin(Stdio::null()).stdout(Stdio::null()).stderr(stderr).spawn()
        .context("FFmpeg could not start. Install FFmpeg, place it beside SoundGeo, or set its path in Advanced settings.")?;
    let start = Instant::now();
    let status = loop {
        if let Err(e) = control.check() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        if start.elapsed() > Duration::from_secs(600) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Audio extraction exceeded 10 minutes.");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(60));
    };
    if !status.success() {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        bail!(
            "Could not extract an audio stream: {}",
            text.chars().take(1000).collect::<String>()
        );
    }
    let mut reader = hound::WavReader::open(wav)?;
    let samples: Vec<f32> = reader
        .samples::<f32>()
        .collect::<std::result::Result<_, _>>()?;
    if samples.len() > SAMPLE_RATE * 7200 {
        bail!(
            "Recordings longer than two hours must be split into shorter clips; keep their relative offsets when interpreting results."
        );
    }
    if samples.len() < SAMPLE_RATE * 20 {
        bail!("Use at least 20 seconds of audio to resolve a flyover curve.");
    }
    if samples.iter().any(|x| !x.is_finite()) {
        bail!("Audio contains invalid sample values.");
    }
    Ok(samples)
}

pub fn detect(
    samples: &[f32],
    job: &Job,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Vec<Event>> {
    let mut found = detect_scale(samples, job, control, progress, false)?;
    if job.fast_flyovers {
        found.extend(detect_scale(samples, job, control, progress, true)?);
    }
    found.sort_by(|a, b| event_quality(b).total_cmp(&event_quality(a)));
    let mut events: Vec<Event> = vec![];
    for e in found {
        if events
            .iter()
            .any(|p| e.start_s < p.end_s && p.start_s < e.end_s)
        {
            continue;
        }
        events.push(e);
    }
    let mut events = collapse_continuations(events);
    events.sort_by(|a, b| event_quality(b).total_cmp(&event_quality(a)));
    events.truncate(24);
    events.sort_by(|a, b| a.cpa_s.total_cmp(&b.cpa_s));
    Ok(events)
}

fn collapse_continuations(mut events: Vec<Event>) -> Vec<Event> {
    events.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    let mut groups: Vec<Event> = Vec::new();
    let mut previous: Option<(f64, f64, bool)> = None;
    for event in events {
        let slow = event.end_s - event.start_s >= 20.;
        let continues =
            previous
                .zip(event.ridge.first())
                .is_some_and(|((end, hz, was_slow), first)| {
                    slow && was_slow
                        && (0. ..=5.).contains(&(event.start_s - end))
                        && (first.hz - hz).abs() <= 2_f64.max(hz * 0.01)
                });
        // Compare the next fragment with this fragment, not the representative.
        // A quiet middle fragment can connect two stronger outer fragments.
        previous = event.ridge.last().map(|p| (event.end_s, p.hz, slow));
        if continues {
            let representative = groups.last_mut().unwrap();
            let span = [representative.activity_start_s(), event.activity_end_s()];
            if event_quality(&event) > event_quality(representative) {
                *representative = event;
            }
            representative.activity_span_s = Some(span);
        } else {
            groups.push(event);
        }
    }
    groups
}

fn event_quality(e: &Event) -> f64 {
    let duration = e.end_s - e.start_s;
    // A long one-sided fragment can fit very well with an incorrect CPA and
    // slant range. Prefer evidence on both sides when choosing overlapping
    // slow curves; brief jet detections retain their original ranking.
    let balance = if duration >= 20. {
        (2. * (e.cpa_s - e.start_s).min(e.end_s - e.cpa_s) / duration).clamp(0.05, 1.)
    } else {
        1.
    };
    balance * e.contrast_db * duration.min(20.).sqrt() / (1. + 100. * e.fit_rmse_hz / e.base_hz)
}

/// Bound the expensive initial search without throwing away quieter evidence.
/// Every retained event may corroborate a seed from these strongest eight.
pub(crate) fn anchor_indices(events: &[Event]) -> Vec<usize> {
    let mut indices: Vec<_> = (0..events.len()).collect();
    indices.sort_by(|&a, &b| event_quality(&events[b]).total_cmp(&event_quality(&events[a])));
    indices.truncate(8);
    indices.sort_unstable();
    indices
}

fn detect_scale(
    samples: &[f32],
    job: &Job,
    control: &Control,
    progress: &Progress<'_>,
    fast: bool,
) -> Result<Vec<Event>> {
    let fft_size = if fast { 1024 } else { 16384 };
    let hop = if fast { 200 } else { 4000 };
    let (min_hz, max_hz) = if fast {
        (500., 3900.)
    } else {
        (job.min_hz, job.max_hz)
    };
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);
    let low = (min_hz * fft_size as f64 / SAMPLE_RATE as f64).ceil() as usize;
    let high = (max_hz * fft_size as f64 / SAMPLE_RATE as f64).floor() as usize;
    let bins = high - low + 1;
    let frames = (samples.len() - fft_size) / hop + 1;
    let win: Vec<f32> = (0..fft_size)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (fft_size - 1) as f32).cos())
        .collect();
    let mut spectrum = Vec::with_capacity(frames);
    let mut buffer = vec![Complex::default(); fft_size];
    for frame in 0..frames {
        if frame % 100 == 0 {
            control.check()?;
            progress(
                if fast {
                    "Scanning short high-frequency flyovers"
                } else {
                    "Building the low-frequency spectrogram"
                },
                if fast { 0.24 } else { 0.10 } + 0.06 * frame as f32 / frames as f32,
            );
        }
        for i in 0..fft_size {
            buffer[i] = Complex::new(samples[frame * hop + i] * win[i], 0.);
        }
        fft.process(&mut buffer);
        let values: Vec<f32> = buffer[low..=high]
            .iter()
            .map(|v| 10. * v.norm_sqr().max(1e-20).log10())
            .collect();
        // Wind/jet roar has a steep, changing spectral slope. A single median
        // across the whole band mistakes bass for a tone and hides fan ridges.
        let mut neighbors = Vec::with_capacity(25);
        let mut contrast = Vec::with_capacity(bins);
        for (b, &value) in values.iter().enumerate() {
            neighbors.clear();
            neighbors.extend_from_slice(&values[b.saturating_sub(12)..(b + 13).min(bins)]);
            let middle = neighbors.len() / 2;
            neighbors.select_nth_unstable_by(middle, f32::total_cmp);
            contrast.push(value - neighbors[middle]);
        }
        spectrum.push(contrast);
    }
    // Short jets need both 10- and 20-second windows. Slow propeller curves
    // need 90 seconds. Half-window overlap retains passes near boundaries.
    let lengths = if fast { vec![800usize, 400] } else { vec![180] };
    let mut windows = vec![];
    for length in lengths {
        let window = length.min(frames);
        let mut starts: Vec<usize> = (0..frames.saturating_sub(window) + 1)
            .step_by(window / 2)
            .collect();
        if starts.last().copied() != Some(frames - window) {
            starts.push(frames - window);
        }
        windows.extend(starts.into_iter().map(|start| (start, window)));
    }
    let bands = if fast {
        vec![
            (
                low,
                (1900. * fft_size as f64 / SAMPLE_RATE as f64).floor() as usize,
                false,
            ),
            (
                (1400. * fft_size as f64 / SAMPLE_RATE as f64).ceil() as usize,
                high,
                true,
            ),
        ]
    } else {
        vec![(low, high, false)]
    };
    let mut found = vec![];
    for (i, &(start, window)) in windows.iter().enumerate() {
        control.check()?;
        progress(
            "Tracing and fitting Doppler sweeps",
            if fast { 0.30 } else { 0.16 }
                + if fast { 0.04 } else { 0.08 } * i as f32 / windows.len() as f32,
        );
        for &(band_low, band_high, require_harmonic) in &bands {
            // Separate bands keep high-frequency noise from consuming the
            // original low-band ridge budget. The extension needs corroboration.
            let mut patch: Vec<Vec<f32>> = spectrum[start..start + window]
                .iter()
                .map(|row| row[band_low - low..=band_high - low].to_vec())
                .collect();
            for _ in 0..4 {
                let ridge = trace(&patch, band_low, start, fft_size, hop);
                if let Some(event) = fit(&ridge, job, fast)
                    && (!require_harmonic
                        || harmonic_support(&event, &spectrum, low, fft_size, hop))
                {
                    let event = if fast {
                        event
                    } else {
                        extend_slow_event(&event, &spectrum, low, fft_size, hop, job)
                            .unwrap_or(event)
                    };
                    found.push(event);
                }
                for (frame, p) in patch.iter_mut().zip(&ridge) {
                    let b = (p.hz * fft_size as f64 / SAMPLE_RATE as f64).round() as isize
                        - band_low as isize;
                    for j in b - 5..=b + 5 {
                        if j >= 0 && (j as usize) < frame.len() {
                            frame[j as usize] = -20.;
                        }
                    }
                }
            }
        }
    }
    Ok(found)
}

fn extend_slow_event(
    event: &Event,
    spectrum: &[Vec<f32>],
    low: usize,
    fft_size: usize,
    hop: usize,
    job: &Job,
) -> Option<Event> {
    if event.end_s - event.start_s < 45. {
        return None;
    }
    // Use the initial curve to follow a tone through a longer unmasked patch.
    // A second unconstrained wide-band scan can select a louder stationary tone.
    let center =
        ((event.cpa_s * SAMPLE_RATE as f64 - fft_size as f64 * 0.5) / hop as f64).round() as usize;
    let half = 90 * SAMPLE_RATE / hop;
    let start = center.saturating_sub(half);
    let end = (center + half).min(spectrum.len());
    if end <= start || end - start < 180 {
        return None;
    }
    let width = (event.base_hz * 0.04).max(4.);
    let patch: Vec<Vec<f32>> = spectrum[start..end]
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let t = ((start + i) * hop + fft_size / 2) as f64 / SAMPLE_RATE as f64;
            let predicted = event.base_hz
                * straight_factor(
                    t,
                    event.cpa_s,
                    event.speed_mps,
                    event.slant_m,
                    job.sound_speed(),
                );
            row.iter()
                .enumerate()
                .map(|(bin, &value)| {
                    let hz = (low + bin) as f64 * SAMPLE_RATE as f64 / fft_size as f64;
                    if (hz - predicted).abs() <= width {
                        value
                    } else {
                        -50.
                    }
                })
                .collect()
        })
        .collect();
    let ridge = trace(&patch, low, start, fft_size, hop);
    let refined = fit(&ridge, job, false)?;
    if refined.end_s - refined.start_s < event.end_s - event.start_s + 20.
        || refined.fit_rmse_hz / refined.base_hz > 1.25 * event.fit_rmse_hz / event.base_hz + 0.001
        || refined.contrast_db < event.contrast_db - 6.
    {
        return None;
    }
    Some(refined)
}

fn harmonic_support(
    event: &Event,
    spectrum: &[Vec<f32>],
    low: usize,
    fft_size: usize,
    hop: usize,
) -> bool {
    // Independent noise ridges can fit a smooth curve when the band is wide.
    // A real harmonic must share the *whole moving frequency pattern*, not
    // merely intersect a stationary tone at one instant. Inspect unmasked bins.
    [0.5, 1. / 3., 0.25, 2.].into_iter().any(|ratio| {
        let mut contrast = vec![];
        for point in &event.ridge {
            let frame =
                ((point.time_s * SAMPLE_RATE as f64 - fft_size as f64 * 0.5) / hop as f64).round();
            let bin = (point.hz * ratio * fft_size as f64 / SAMPLE_RATE as f64).round() as isize
                - low as isize;
            if frame >= 0.
                && bin >= 0
                && let Some(value) = spectrum
                    .get(frame as usize)
                    .and_then(|row| row.get(bin as usize))
            {
                contrast.push(*value);
            }
        }
        if contrast.is_empty() || contrast.len() * 5 < event.ridge.len() * 4 {
            return false;
        }
        let middle = contrast.len() / 2;
        contrast.select_nth_unstable_by(middle, f32::total_cmp);
        contrast[middle] >= 4.
    })
}

fn trace(
    spec: &[Vec<f32>],
    low: usize,
    start: usize,
    fft_size: usize,
    hop: usize,
) -> Vec<RidgePoint> {
    let n = spec.len();
    let bins = spec[0].len();
    let mut prev = spec[0].clone();
    let mut next = vec![0.; bins];
    let mut back = vec![0i8; n * bins];
    for t in 1..n {
        for b in 0..bins {
            let mut best = f32::NEG_INFINITY;
            let mut delta = 0;
            for d in -1i32..=5 {
                let old = b as i32 + d;
                if old < 0 || old >= bins as i32 {
                    continue;
                }
                let penalty = if fft_size < 2048 { 1.2 } else { 0.35 };
                let score = prev[old as usize] - penalty * (d as f32).abs();
                if score > best {
                    best = score;
                    delta = d as i8;
                }
            }
            next[b] = best + spec[t][b].clamp(-5., 35.);
            back[t * bins + b] = delta;
        }
        std::mem::swap(&mut prev, &mut next);
    }
    let mut b = prev
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    let mut indices = vec![0; n];
    for t in (0..n).rev() {
        indices[t] = b;
        if t > 0 {
            b = (b as isize + back[t * bins + b] as isize) as usize;
        }
    }
    indices
        .iter()
        .enumerate()
        .map(|(t, &b)| {
            // Sub-bin quadratic interpolation in log power.
            let delta = if b > 0 && b + 1 < bins {
                let (a, y, c) = (spec[t][b - 1], spec[t][b], spec[t][b + 1]);
                let den = a - 2. * y + c;
                if den.abs() > 1e-4 {
                    (0.5 * (a - c) / den).clamp(-0.5, 0.5)
                } else {
                    0.
                }
            } else {
                0.
            };
            RidgePoint {
                time_s: ((start + t) * hop + fft_size / 2) as f64 / SAMPLE_RATE as f64,
                hz: (low as f64 + b as f64 + delta as f64) * SAMPLE_RATE as f64 / fft_size as f64,
                contrast_db: spec[t][b] as f64,
            }
        })
        .collect()
}

fn fit(points: &[RidgePoint], job: &Job, fast: bool) -> Option<Event> {
    let minimum_contrast = if fast {
        job.fast_min_snr_db
    } else {
        job.min_snr_db
    };
    let mut contrasts: Vec<_> = points.iter().map(|p| p.contrast_db).collect();
    contrasts.sort_by(f64::total_cmp);
    // A ridge continues through noise after the plane fades. Trim by sustained
    // contrast relative to the actual tone, not isolated high noise bins.
    let cutoff = minimum_contrast.max(contrasts[contrasts.len() * 3 / 4] - 18.);
    let strong: Vec<usize> = (2..points.len() - 2)
        .filter(|&i| {
            let mut local: Vec<_> = points[i - 2..=i + 2]
                .iter()
                .map(|p| p.contrast_db)
                .collect();
            local.sort_by(f64::total_cmp);
            local[2] >= cutoff
        })
        .collect();
    if strong.len() < 40 {
        return None;
    }
    let start = *strong.first()?;
    let end = *strong.last()?;
    let ridge: Vec<_> = points[start..=end]
        .iter()
        .filter(|p| p.contrast_db >= cutoff - 3.)
        .cloned()
        .collect();
    if ridge.len() < 40
        || ridge.last()?.time_s - ridge[0].time_s < if fast { 4. } else { 20. }
        || ridge.len() as f64 / ((end - start + 1) as f64) < 0.7
    {
        return None;
    }
    let count = (ridge.len() / 6).max(3);
    let head = ridge[..count].iter().map(|p| p.hz).sum::<f64>() / count as f64;
    let tail = ridge[ridge.len() - count..]
        .iter()
        .map(|p| p.hz)
        .sum::<f64>()
        / count as f64;
    if head - tail < 4. || (head - tail) / head < if fast { 0.08 } else { 0.035 } {
        return None;
    }
    let time_start = ridge[0].time_s;
    let time_end = ridge.last()?.time_s;
    let middle = (time_start + time_end) * 0.5;
    let c = job.sound_speed();
    let score = |t: f64, v: f64, d: f64| -> (f64, f64) {
        let mut ff = 0.;
        let mut yf = 0.;
        for p in &ridge {
            let f = straight_factor(p.time_s, t, v, d, c);
            ff += f * f;
            yf += p.hz * f;
        }
        let base = yf / ff;
        let mse = ridge
            .iter()
            .map(|p| (p.hz - base * straight_factor(p.time_s, t, v, d, c)).powi(2))
            .sum::<f64>()
            / ridge.len() as f64;
        (mse, base)
    };
    let mut best = (f64::INFINITY, middle, 80., 1500., 0.);
    let distances = if fast {
        vec![50., 100., 200., 400., 800., 1500., 3000.]
    } else {
        vec![300., 700., 1500., 3000., 6000., 12000.]
    };
    let edge = if fast { 0.5 } else { 3. };
    for v in [35., 55., 80., 110., 150., 200., 250.] {
        for &d in &distances {
            for offset in [-2., -1., 0., 1., 2.] {
                let offset = offset * if fast { 2. } else { 10. };
                let t = middle + offset;
                // Initial proposals must obey the same bounds as refinement.
                // Otherwise an already optimal out-of-bounds fit never moves.
                if t < time_start + edge || t > time_end - edge {
                    continue;
                }
                let (s, b) = score(t, v, d);
                if s < best.0 {
                    best = (s, t, v, d, b);
                }
            }
        }
    }
    let mut steps = if fast {
        [2., 25., 200.]
    } else {
        [10., 25., 1000.]
    };
    for _ in 0..60 {
        let mut improved = false;
        for axis in 0..3 {
            for sign in [-1., 1.] {
                let mut p = [best.1, best.2, best.3];
                p[axis] += sign * steps[axis];
                if p[0] < time_start + edge
                    || p[0] > time_end - edge
                    || !(25. ..=260.).contains(&p[1])
                    || !(if fast { 25. } else { 100. }..=20000.).contains(&p[2])
                {
                    continue;
                }
                let (s, b) = score(p[0], p[1], p[2]);
                if s < best.0 {
                    best = (s, p[0], p[1], p[2], b);
                    improved = true;
                }
            }
        }
        if !improved {
            for s in &mut steps {
                *s *= 0.5;
            }
        }
    }
    let rmse = best.0.sqrt();
    let mean = ridge.iter().map(|p| p.hz).sum::<f64>() / ridge.len() as f64;
    let variance = ridge.iter().map(|p| (p.hz - mean).powi(2)).sum::<f64>() / ridge.len() as f64;
    let contrast = ridge.iter().map(|p| p.contrast_db).sum::<f64>() / ridge.len() as f64;
    let linear_rmse = linear_baseline_rmse(ridge.iter().map(|p| (p.time_s, p.hz)));
    if rmse > 1.5_f64.max(mean * if fast { 0.015 } else { 0.008 })
        || best.0 / variance > 0.12
        || linear_rmse <= f64::EPSILON
        || best.0 > 0.5 * linear_rmse.powi(2)
        || contrast < minimum_contrast
    {
        return None;
    }
    Some(Event {
        start_s: time_start,
        end_s: time_end,
        cpa_s: best.1,
        base_hz: best.4,
        speed_mps: best.2,
        slant_m: best.3,
        fit_rmse_hz: rmse,
        contrast_db: contrast,
        ridge,
        activity_span_s: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connected_slow_fragments_count_once_and_preserve_the_activity_interval() {
        let fragment = |start: f64, contrast: f64| Event {
            start_s: start,
            end_s: start + 40.,
            cpa_s: start + 20.,
            base_hz: 190.,
            speed_mps: 80.,
            slant_m: 1200.,
            fit_rmse_hz: 0.5,
            contrast_db: contrast,
            ridge: (0..=80)
                .map(|i| {
                    let t = start + i as f64 * 0.5;
                    RidgePoint {
                        time_s: t,
                        hz: 200. - 0.2 * t,
                        contrast_db: contrast,
                    }
                })
                .collect(),
            activity_span_s: None,
        };
        // The strongest first fragment does not touch the last fragment.
        // Continuity must be transitive through the quieter middle fragment.
        let fragments = vec![fragment(0., 30.), fragment(43., 20.), fragment(86., 25.)];
        let grouped = collapse_continuations(fragments.clone());
        assert_eq!(
            grouped.len(),
            1,
            "A continuous tone is not three independent flyovers"
        );
        assert_eq!(grouped[0].activity_span_s, Some([0., 126.]));
        assert_eq!(grouped[0].start_s, 0.);
        assert_eq!(grouped[0].end_s, 40.);
        assert_eq!(grouped[0].ridge.len(), fragments[0].ridge.len());

        let mut separate = fragments;
        for p in &mut separate[1].ridge {
            p.hz += 40.;
        }
        assert_eq!(
            collapse_continuations(separate).len(),
            3,
            "A frequency reset starts another sound"
        );
        assert_eq!(
            collapse_continuations(vec![fragment(0., 30.), fragment(50., 30.)]).len(),
            2
        );
    }

    #[test]
    fn old_event_json_uses_its_observed_curve_interval() {
        let event: Event = serde_json::from_value(serde_json::json!({
            "start_s": 10., "end_s": 40., "cpa_s": 25., "base_hz": 130.,
            "speed_mps": 80., "slant_m": 1000., "fit_rmse_hz": 0.5,
            "contrast_db": 20., "ridge": []
        }))
        .unwrap();
        assert_eq!(event.activity_start_s(), 10.);
        assert_eq!(event.activity_end_s(), 40.);
        assert!(event.activity_span_s.is_none());
    }
    #[test]
    fn a_partial_curve_cannot_report_closest_approach_outside_its_samples() {
        let job = Job::default();
        let points: Vec<_> = (0..57)
            .map(|i| {
                let time_s = i as f64 * 0.5;
                RidgePoint {
                    time_s,
                    hz: 482. * straight_factor(time_s, -6., 150., 700., job.sound_speed()),
                    contrast_db: 25.,
                }
            })
            .collect();
        if let Some(event) = fit(&points, &job, false) {
            assert!(
                event.cpa_s >= event.start_s + 3. && event.cpa_s <= event.end_s - 3.,
                "Closest approach {} lies outside the supported interval {}..{}",
                event.cpa_s,
                event.start_s,
                event.end_s
            );
        }
    }
    #[test]
    fn a_linear_frequency_drift_is_not_a_resolved_flyover() {
        let points: Vec<_> = (0..161)
            .map(|i| {
                let time_s = i as f64 * 0.5;
                RidgePoint {
                    time_s,
                    hz: 160. - 0.3 * time_s + 0.05 * (i as f64 * 1.7).sin(),
                    contrast_db: 20.,
                }
            })
            .collect();
        assert!(
            fit(&points, &Job::default(), false).is_none(),
            "A descending line alone cannot constrain a Doppler closest approach"
        );
    }
}
