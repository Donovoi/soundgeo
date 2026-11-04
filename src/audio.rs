use crate::{Control, Job, Progress, model::straight_factor};
use anyhow::{Context, Result, bail};
use rustfft::{FftPlanner, num_complex::Complex};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const SAMPLE_RATE: usize = 4000;
const FFT: usize = 8192;
const HOP: usize = 2000;
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
        .arg(&job.media).args(["-map","0:a:0","-vn","-ac","1","-ar","4000","-t","7200.001","-c:a","pcm_f32le"])
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
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT);
    let low = (job.min_hz * FFT as f64 / SAMPLE_RATE as f64).ceil() as usize;
    let high = (job.max_hz * FFT as f64 / SAMPLE_RATE as f64).floor() as usize;
    let bins = high - low + 1;
    let frames = (samples.len() - FFT) / HOP + 1;
    let win: Vec<f32> = (0..FFT)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (FFT - 1) as f32).cos())
        .collect();
    let mut spectrum = Vec::with_capacity(frames);
    let mut buffer = vec![Complex::default(); FFT];
    for frame in 0..frames {
        if frame % 100 == 0 {
            control.check()?;
            progress(
                "Building the low-frequency spectrogram",
                0.10 + 0.10 * frame as f32 / frames as f32,
            );
        }
        for i in 0..FFT {
            buffer[i] = Complex::new(samples[frame * HOP + i] * win[i], 0.);
        }
        fft.process(&mut buffer);
        let values: Vec<f32> = buffer[low..=high]
            .iter()
            .map(|v| 10. * v.norm_sqr().max(1e-20).log10())
            .collect();
        let mut sorted = values.clone();
        sorted.sort_by(f32::total_cmp);
        let noise = sorted[bins / 2];
        spectrum.push(values.iter().map(|v| v - noise).collect::<Vec<_>>());
    }
    // 90-second windows overlap by 45 seconds, so complete flyovers survive boundaries.
    let window = 180usize.min(frames);
    let step = 90;
    let mut starts: Vec<usize> = (0..frames.saturating_sub(window) + 1)
        .step_by(step)
        .collect();
    if starts.last().copied() != Some(frames - window) {
        starts.push(frames - window);
    }
    let mut found = vec![];
    for (i, &start) in starts.iter().enumerate() {
        control.check()?;
        progress(
            "Tracing and fitting Doppler sweeps",
            0.20 + 0.14 * i as f32 / starts.len() as f32,
        );
        let mut patch = spectrum[start..start + window].to_vec();
        // Several ridges let a strong stationary hum coexist with a quiet aircraft tone.
        for _ in 0..4 {
            let ridge = trace(&patch, low, start);
            if let Some(event) = fit(&ridge, job) {
                found.push(event);
            }
            for (frame, p) in patch.iter_mut().zip(&ridge) {
                let b = (p.hz * FFT as f64 / SAMPLE_RATE as f64).round() as isize - low as isize;
                for j in b - 5..=b + 5 {
                    if j >= 0 && (j as usize) < bins {
                        frame[j as usize] = -20.;
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| {
        (b.contrast_db / (1. + b.fit_rmse_hz)).total_cmp(&(a.contrast_db / (1. + a.fit_rmse_hz)))
    });
    let mut events: Vec<Event> = vec![];
    for e in found {
        // Overlapping/harmonic ridges count as one event, not independent evidence.
        if events
            .iter()
            .any(|p| e.start_s < p.end_s && p.start_s < e.end_s)
        {
            continue;
        }
        events.push(e);
        if events.len() == 8 {
            break;
        }
    }
    events.sort_by(|a, b| a.cpa_s.total_cmp(&b.cpa_s));
    Ok(events)
}

fn trace(spec: &[Vec<f32>], low: usize, start: usize) -> Vec<RidgePoint> {
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
                let score = prev[old as usize] - 0.35 * (d as f32).abs();
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
                time_s: ((start + t) * HOP + FFT / 2) as f64 / SAMPLE_RATE as f64,
                hz: (low as f64 + b as f64 + delta as f64) * SAMPLE_RATE as f64 / FFT as f64,
                contrast_db: spec[t][b] as f64,
            }
        })
        .collect()
}

fn fit(points: &[RidgePoint], job: &Job) -> Option<Event> {
    let mut contrasts: Vec<_> = points.iter().map(|p| p.contrast_db).collect();
    contrasts.sort_by(f64::total_cmp);
    // A ridge continues through noise after the plane fades. Trim by sustained
    // contrast relative to the actual tone, not isolated high noise bins.
    let cutoff = job.min_snr_db.max(contrasts[contrasts.len() * 3 / 4] - 18.);
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
        || ridge.last()?.time_s - ridge[0].time_s < 20.
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
    if head - tail < 4. || (head - tail) / head < 0.035 {
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
    for v in [35., 55., 80., 110., 150., 200., 250.] {
        for d in [300., 700., 1500., 3000., 6000., 12000.] {
            for offset in [-20., -10., 0., 10., 20.] {
                let t = middle + offset;
                let (s, b) = score(t, v, d);
                if s < best.0 {
                    best = (s, t, v, d, b);
                }
            }
        }
    }
    let mut steps = [10., 25., 1000.];
    for _ in 0..60 {
        let mut improved = false;
        for axis in 0..3 {
            for sign in [-1., 1.] {
                let mut p = [best.1, best.2, best.3];
                p[axis] += sign * steps[axis];
                if p[0] < time_start + 3.
                    || p[0] > time_end - 3.
                    || !(25. ..=260.).contains(&p[1])
                    || !(100. ..=20000.).contains(&p[2])
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
    if rmse > 1.5_f64.max(mean * 0.008) || best.0 / variance > 0.12 || contrast < job.min_snr_db {
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
    })
}
