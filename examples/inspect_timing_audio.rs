//! Development inspection only; this does not return a location.
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(args.len() == 3, "Expected job JSON and output JSON paths");
    let job: soundgeo::Job = serde_json::from_reader(std::fs::File::open(&args[1])?)?;
    let control = soundgeo::Control::default();
    let samples = soundgeo::audio::decode(&job, &control)?;
    let peaks = soundgeo::timing::detect(&samples, &control)?;
    serde_json::to_writer_pretty(std::fs::File::create(&args[2])?, &peaks)?;
    eprintln!("{} audio-only peaks", peaks.len());
    Ok(())
}
