//! Research command. Scores are not calibrated location probabilities.
//! Optional audio-only peak JSON permits independent negative-control checks.
use anyhow::{Context, Result, ensure};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    ensure!(
        (3..=5).contains(&args.len()),
        "Expected job JSON, output JSON, and optional peak-array JSON plus duration"
    );
    let job: soundgeo::Job = serde_json::from_reader(std::fs::File::open(&args[1])?)?;
    job.validate()?;
    let control = soundgeo::Control::default();
    let (peaks, duration) = if args.len() == 5 {
        let peaks: Vec<f64> = serde_json::from_reader(std::fs::File::open(&args[3])?)?;
        let duration = args[4]
            .to_str()
            .context("Invalid duration")?
            .parse::<f64>()?;
        (peaks, duration)
    } else {
        ensure!(args.len() == 3, "Peak overrides require a duration");
        let samples = soundgeo::audio::decode(&job, &control)?;
        let peaks = soundgeo::timing::detect(&samples, &control)?
            .iter()
            .map(|p| p.time_s)
            .collect();
        (
            peaks,
            samples.len() as f64 / soundgeo::audio::SAMPLE_RATE as f64,
        )
    };
    let sources = if let Some(archive) = &job.archive {
        archive.ensure_for_bounds(job.bounds, &control, &|m, _| eprintln!("{m}"))?
    } else {
        vec![job.tracks.clone()]
    };
    let data = soundgeo::data::load_for_job_sources(&job, &sources, duration, &control)?;
    eprintln!(
        "{} peaks, {} track segments",
        peaks.len(),
        data.tracks.len()
    );
    let start = std::time::Instant::now();
    let result = soundgeo::timing::locate(
        &job,
        &peaks,
        duration,
        &data.tracks,
        &job.timing_options,
        &control,
        &|m, _| eprintln!("{m}"),
    )?;
    let report = serde_json::json!({"not_acceptance":true,"job":job,"peaks":peaks,"duration_s":duration,"reference_sources":sources,"warnings":data.warnings,"elapsed_s":start.elapsed().as_secs_f64(),"result":result});
    let output = std::path::Path::new(&args[2]);
    ensure!(
        !output.exists(),
        "Research output already exists; preserve it and choose a new path"
    );
    serde_json::to_writer_pretty(std::fs::File::create(output)?, &report)?;
    Ok(())
}
