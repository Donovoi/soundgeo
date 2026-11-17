//! Plan and run full-search controls from a saved unknown-time Doppler report.
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use soundgeo::{Control, Report, doppler_controls};
use std::path::{Path, PathBuf};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Replay saved observations unchanged through the production search.
    Replay {
        report: PathBuf,
        output: PathBuf,
        #[arg(long)]
        date_checkpoints: Option<PathBuf>,
    },
    /// Freeze whole-activity permutations without running a location search.
    Plan { report: PathBuf, output: PathBuf },
    /// Rerun the original full search for one saved control, entirely offline.
    Run {
        report: PathBuf,
        plan: PathBuf,
        #[arg(value_parser = clap::value_parser!(u8).range(0..=18))]
        index: u8,
        output: PathBuf,
        /// Optional completed-date recovery folder, isolated for this control.
        #[arg(long)]
        date_checkpoints: Option<PathBuf>,
    },
}

#[derive(Serialize, Deserialize)]
struct SavedPlan {
    schema: u32,
    source_report_sha256: String,
    changes_location_acceptance: bool,
    calibrated_location_probability: bool,
    plan: doppler_controls::Plan,
    search: doppler_controls::SearchBinding,
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
        Command::Replay {
            report,
            output,
            date_checkpoints,
        } => {
            ensure!(
                !output.exists(),
                "Output already exists; preserve the earlier result"
            );
            let (report, report_hash) = load(&report)?;
            let search = doppler_controls::bind_search(&report, &Control::default())?;
            search.check_output_path(&output)?;
            let mut control = Control::default().with_offline(true);
            if let Some(path) = date_checkpoints {
                search.check_output_path(&path)?;
                control = control.with_date_checkpoints(path);
            }
            let result = doppler_controls::replay_observations(
                &report,
                &search,
                report.events.clone(),
                &control,
                &|message, fraction| eprintln!("{:3.0}% {message}", fraction * 100.),
            )?;
            save(
                &output,
                &serde_json::json!({
                    "schema":2, "identity_replay":true, "source_report_sha256":report_hash,
                    "search":search, "report":result, "changes_location_acceptance":false,
                    "calibrated_location_probability":false,
                    "scope":"Identity diagnostic from saved JSON observations, not a new independent location result. Compare this replay before interpreting controls against an older baseline."
                }),
            )?;
        }
        Command::Plan { report, output } => {
            let (report, source_report_sha256) = load(&report)?;
            let plan = doppler_controls::plan(&report.events, report.duration_s)?;
            let search = doppler_controls::bind_search(&report, &Control::default())?;
            search.check_output_path(&output)?;
            save(
                &output,
                &SavedPlan {
                    schema: 2,
                    source_report_sha256,
                    changes_location_acceptance: false,
                    calibrated_location_probability: false,
                    plan,
                    search,
                },
            )?;
        }
        Command::Run {
            report,
            plan,
            index,
            output,
            date_checkpoints,
        } => {
            ensure!(
                !output.exists(),
                "Output already exists; preserve the earlier result"
            );
            let (report, report_hash) = load(&report)?;
            let encoded: serde_json::Value = serde_json::from_slice(&std::fs::read(&plan)?)?;
            ensure!(
                encoded["schema"] == 2,
                "Control plan schema changed; preserve this file and create a new schema-2 plan"
            );
            let saved: SavedPlan = serde_json::from_value(encoded)?;
            ensure!(
                saved.schema == 2
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
            saved.search.check_output_path(&output)?;
            let mut control = Control::default().with_offline(true);
            if let Some(path) = date_checkpoints {
                saved.search.check_output_path(&path)?;
                control = control.with_date_checkpoints(path);
            }
            let result = doppler_controls::replay_observations(
                &report,
                &saved.search,
                events,
                &control,
                &|message, fraction| eprintln!("{:3.0}% {message}", fraction * 100.),
            )?;
            save(
                &output,
                &serde_json::json!({
                    "schema":2,"source_report_sha256":report_hash,"control_index":index,
                    "permutation":saved.plan.controls[index as usize],"shifts":shifts,"events":result.events,
                    "job":result.job,"duration_s":result.duration_s,"track_count":result.track_count,
                    "candidates":result.candidates,"hypotheses":result.hypotheses,"search_method":result.search_method,
                    "search_grid_m":result.search_grid_m,"warnings":result.warnings,
                    "doppler_partitions":result.doppler_partitions,"date_search":result.date_search,
                    "report":result,
                    "changes_location_acceptance":false,"calibrated_location_probability":false,
                    "scope":"Production post-observation search of reordered complete activities, with the baseline date windows, source plan, bounds and resource limits. Current raw input bytes are bound when planning; an old report alone does not prove its historical raw input bytes."
                }),
            )?;
        }
    }
    Ok(())
}
