//! Diagnostic utility: cargo run --release --example inspect_audio -- job.json events.json
use anyhow::{Context, Result};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let config = args
        .get(1)
        .context("Expected job JSON and output JSON paths")?;
    let output = args.get(2).context("Expected output JSON path")?;
    let job: soundgeo::Job = serde_json::from_reader(std::fs::File::open(config)?)?;
    let control = soundgeo::Control::default();
    let samples = soundgeo::audio::decode(&job, &control)?;
    let events = soundgeo::audio::detect(&samples, &job, &control, &|_, _| {})?;
    serde_json::to_writer_pretty(std::fs::File::create(output)?, &events)?;
    eprintln!("Detected {} events", events.len());
    Ok(())
}
