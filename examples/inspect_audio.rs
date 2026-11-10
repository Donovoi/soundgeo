//! Diagnostic utility: inspect_audio job.json events.json [--all-observations]
use anyhow::{Context, Result, bail};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let config = args
        .get(1)
        .context("Expected job JSON and output JSON paths")?;
    let output = args.get(2).context("Expected output JSON path")?;
    let all_observations = match args.get(3) {
        None => false,
        Some(flag) if flag == "--all-observations" && args.len() == 4 => true,
        _ => bail!("Expected inspect_audio job.json events.json [--all-observations]"),
    };
    let job: soundgeo::Job = serde_json::from_reader(std::fs::File::open(config)?)?;
    let control = soundgeo::Control::default();
    let samples = soundgeo::audio::decode(&job, &control)?;
    let events = if all_observations {
        soundgeo::audio::detect_all(&samples, &job, &control, &|_, _| {})?
    } else {
        soundgeo::audio::detect(&samples, &job, &control, &|_, _| {})?
    };
    serde_json::to_writer_pretty(std::fs::File::create(output)?, &events)?;
    eprintln!("Detected {} events", events.len());
    Ok(())
}
