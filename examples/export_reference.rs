//! Export every retained point in a regional full-day archive for repeatable benchmarks.
use anyhow::{Context, Result, bail};
use soundgeo::aircraft::{AircraftFilter, AircraftKind};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let config = args
        .get(1)
        .context("Expected job JSON and output CSV paths")?;
    let output = args.get(2).context("Expected output CSV path")?;
    let mut job: soundgeo::Job = serde_json::from_reader(std::fs::File::open(config)?)?;
    if job.start_utc.is_some() {
        bail!("Benchmark reference export must not filter by recording clock");
    }
    job.aircraft_filter = AircraftFilter::All;
    if job.type_catalog.is_none() {
        job.type_catalog = job
            .tracks
            .parent()
            .map(|p| p.join("aircraft-types.json"))
            .filter(|p| p.is_file());
    }
    let dataset = soundgeo::data::load_for_job(&job, 0., &soundgeo::Control::default())?;
    let mut writer = csv::Writer::from_path(output)?;
    writer.write_record([
        "aircraft",
        "time",
        "lat",
        "lon",
        "altitude_m",
        "type_description",
    ])?;
    let mut count = 0;
    for track in &dataset.tracks {
        let description = match track.kind {
            AircraftKind::Propeller => "L1P",
            AircraftKind::Jet => "L2J",
            AircraftKind::Rotorcraft => "H1T",
            _ => "",
        };
        for sample in &track.samples {
            writer.write_record([
                track.id.clone(),
                sample.time.to_string(),
                sample.point.lat.to_string(),
                sample.point.lon.to_string(),
                sample.point.alt.to_string(),
                description.into(),
            ])?;
            count += 1;
        }
    }
    writer.flush()?;
    eprintln!(
        "Exported {} segments / {count} points; no clock filtering",
        dataset.tracks.len()
    );
    for warning in dataset.warnings {
        eprintln!("{warning}");
    }
    Ok(())
}
