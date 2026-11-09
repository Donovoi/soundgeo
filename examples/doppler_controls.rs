//! Plan and run full-search controls from a saved unknown-time Doppler report.
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use soundgeo::{Control, Report, doppler_controls, geo::Terrain};
use std::path::{Path, PathBuf};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Freeze whole-activity permutations without running a location search.
    Plan { report: PathBuf, output: PathBuf },
    /// Rerun the original full search for one saved control, entirely offline.
    Run {
        report: PathBuf,
        plan: PathBuf,
        #[arg(value_parser = clap::value_parser!(u8).range(0..=18))]
        index: u8,
        output: PathBuf,
    },
}

#[derive(Serialize, Deserialize)]
struct SavedPlan {
    schema: u32,
    source_report_sha256: String,
    changes_location_acceptance: bool,
    calibrated_location_probability: bool,
    plan: doppler_controls::Plan,
}

fn load(path: &Path) -> Result<(Report, String)> {
    let bytes = std::fs::read(path)?;
    let report: Report = serde_json::from_slice(&bytes)?;
    ensure!(
        report.job.start_utc.is_none(),
        "Controls require an unknown recording clock"
    );
    ensure!(
        report
            .job
            .archive
            .as_ref()
            .is_none_or(|a| a.date_from.is_empty() && a.date_to.is_none()),
        "Controls require no supplied recording date"
    );
    ensure!(
        report.timing.is_none() && report.job.analysis_method == soundgeo::AnalysisMethod::Doppler,
        "Expected a Doppler report"
    );
    Ok((report, format!("{:x}", Sha256::digest(bytes))))
}

fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(std::fs::File::create_new(path)?, value)?;
    Ok(())
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::Plan { report, output } => {
            let (report, source_report_sha256) = load(&report)?;
            let plan = doppler_controls::plan(&report.events, report.duration_s)?;
            save(
                &output,
                &SavedPlan {
                    schema: 1,
                    source_report_sha256,
                    changes_location_acceptance: false,
                    calibrated_location_probability: false,
                    plan,
                },
            )?;
        }
        Command::Run {
            report,
            plan,
            index,
            output,
        } => {
            ensure!(
                !output.exists(),
                "Output already exists; preserve the earlier result"
            );
            let (report, report_hash) = load(&report)?;
            let saved: SavedPlan = serde_json::from_slice(&std::fs::read(&plan)?)?;
            ensure!(
                saved.schema == 1
                    && saved.source_report_sha256 == report_hash
                    && !saved.changes_location_acceptance
                    && !saved.calibrated_location_probability,
                "Saved control plan does not match the source report"
            );
            let (events, shifts) = doppler_controls::apply(
                &report.events,
                report.duration_s,
                &saved.plan,
                index.into(),
            )?;
            let mut job = report.job.clone();
            job.offline = true;
            let control = Control::default().with_offline(true);
            let data = soundgeo::data::load_for_job_sources(
                &job,
                &report.reference_sources,
                report.duration_s,
                &control,
            )?;
            let mut terrain =
                Terrain::with_datum(job.terrain.clone(), job.ground_m, job.ground_datum);
            let search = soundgeo::search::locate(
                &job,
                &events,
                &data.tracks,
                &mut terrain,
                &control,
                &|message, fraction| eprintln!("{:3.0}% {message}", fraction * 100.),
            )?;
            save(
                &output,
                &serde_json::json!({
                    "schema":1,"source_report_sha256":report_hash,"control_index":index,
                    "permutation":saved.plan.controls[index as usize],"shifts":shifts,"events":events,
                    "job":job,"duration_s":report.duration_s,"track_count":data.tracks.len(),
                    "candidates":search.candidates,"hypotheses":search.hypotheses,"search_method":search.method,
                    "search_grid_m":search.coarse_grid_m,"warnings":search.warnings,
                    "proposal_partition":search.partition,
                    "changes_location_acceptance":false,"calibrated_location_probability":false,
                    "scope":"Full search of reordered complete sound activities, with original bounds, flight data and resource limits. The control supplies no candidate position or recording clock."
                }),
            )?;
        }
    }
    Ok(())
}
