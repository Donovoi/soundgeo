//! A deterministic fixture, with independent emission-time waveform synthesis.
use crate::{
    Job,
    geo::{Bounds, Point},
    model::{Sample, Track},
};
use anyhow::Result;
use std::path::Path;

pub const TRUTH: Point = Point {
    lat: 35.0,
    lon: -110.0,
    alt: 120.,
};
pub const EPOCH: f64 = 1_788_861_600.; // 2026-09-08 10:00:00 UTC
pub fn tracks() -> Vec<Track> {
    (0..2)
        .map(|plane| {
            let samples = (0..=130)
                .map(|i| {
                    let relative = if plane == 0 {
                        i as f64 - 25.
                    } else {
                        i as f64 + 100.
                    };
                    let dt = relative - if plane == 0 { 35. } else { 160. };
                    let (east, north, alt) = if plane == 0 {
                        (dt * 72., 850. + 0.018 * dt * dt, 1120. + dt * 0.6)
                    } else {
                        (-1100. + 0.025 * dt * dt, dt * 91., 1420. - dt * 0.4)
                    };
                    Sample::new(
                        EPOCH + relative,
                        Point {
                            lat: TRUTH.lat + north / 111320.,
                            lon: TRUTH.lon + east / (111320. * TRUTH.lat.to_radians().cos()),
                            alt,
                        },
                    )
                })
                .collect();
            Track {
                id: format!("SYNTH{}", plane + 1),
                kind: crate::aircraft::AircraftKind::Propeller,
                samples,
            }
        })
        .collect()
}
pub fn create(dir: &Path) -> Result<Job> {
    std::fs::create_dir_all(dir)?;
    let tracks = tracks();
    let csv = dir.join("flights.csv");
    let mut writer = csv::Writer::from_path(&csv)?;
    writer.write_record([
        "aircraft",
        "time",
        "lat",
        "lon",
        "altitude_m",
        "type_description",
    ])?;
    for track in &tracks {
        for s in &track.samples {
            writer.write_record([
                track.id.clone(),
                s.time.to_string(),
                s.point.lat.to_string(),
                s.point.lon.to_string(),
                s.point.alt.to_string(),
                "L1P".into(),
            ])?;
        }
    }
    writer.flush()?;
    let wav = dir.join("synthetic-flyovers.wav");
    let rate = 8000;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut output = hound::WavWriter::create(&wav, spec)?;
    let mut rng = 0x143256u64;
    let observer = TRUTH.ecef();
    let c = 340.39;
    for i in 0..rate * 230 {
        let time = i as f64 / rate as f64;
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut sample = ((rng >> 32) as u32 as f64 / u32::MAX as f64 - 0.5) * 0.13
            + 0.035 * (std::f64::consts::TAU * 50. * time).sin();
        for (j, track) in tracks.iter().enumerate() {
            let center = if j == 0 { 39. } else { 165. };
            let envelope = (1. - ((time - center) / 40.).powi(2)).max(0.).powi(2);
            if envelope == 0. {
                continue;
            }
            // Independently solve the emission timestamp and synthesize the source phase.
            let mut lo = EPOCH + time - 80.;
            let mut hi = EPOCH + time;
            for _ in 0..28 {
                let mid = (lo + hi) * 0.5;
                let idx = track
                    .samples
                    .partition_point(|s| s.time < mid)
                    .clamp(1, track.samples.len() - 1);
                let a = &track.samples[idx - 1];
                let b = &track.samples[idx];
                let f = ((mid - a.time) / (b.time - a.time)).clamp(0., 1.);
                let xyz = a.xyz + (b.xyz - a.xyz) * f;
                if mid + (xyz - observer).norm() / c < EPOCH + time {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let emission = (lo + hi) * 0.5 - EPOCH;
            let frequency = if j == 0 { 135. } else { 190. };
            sample += envelope
                * (0.24 * (std::f64::consts::TAU * frequency * emission).sin()
                    + 0.08 * (std::f64::consts::TAU * frequency * 2. * emission).sin());
        }
        output.write_sample((sample.clamp(-1., 1.) * i16::MAX as f64) as i16)?;
    }
    output.finalize()?;
    let job = Job {
        media: wav,
        tracks: csv,
        ground_m: TRUTH.alt,
        temperature_c: 15.,
        bounds: Some(Bounds {
            south: 34.975,
            north: 35.025,
            west: -110.035,
            east: -109.965,
        }),
        ..Default::default()
    };
    std::fs::write(dir.join("job.json"), serde_json::to_vec_pretty(&job)?)?;
    std::fs::write(
        dir.join("truth.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"synthetic":true,"latitude":TRUTH.lat,"longitude":TRUTH.lon,"ground_m":TRUTH.alt,"recording_start_unix":EPOCH,"purpose":"Test fixture, not validation on a real recording"}),
        )?,
    )?;
    Ok(job)
}
