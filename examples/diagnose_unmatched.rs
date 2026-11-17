//! Diagnose a saved result at its inferred point/clock, without a new search.
use anyhow::{Context, Result, ensure};
use serde_json::json;
use soundgeo::{Control, Report, search::diagnostic};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let input = args
        .get(1)
        .context("Expected report, NEW output, optional reference-source subset")?;
    let output = args.get(2).context("Expected NEW output path")?;
    let report: Report = serde_json::from_reader(BufReader::new(File::open(input)?))?;
    ensure!(report.job.start_utc.is_none(), "Requires an inferred clock");
    let candidate = report.candidates.first().context("No inferred candidate")?;
    let sources: Vec<PathBuf> = if args.len() > 3 {
        args[3..].iter().map(PathBuf::from).collect()
    } else {
        report.reference_sources.clone()
    };
    ensure!(
        !sources.is_empty() && sources.iter().all(|s| report.reference_sources.contains(s)),
        "Sources must be recorded in the original report"
    );
    eprintln!("Loading {} recorded reference sources", sources.len());
    let dataset = soundgeo::data::load_for_job_sources(
        &report.job,
        &sources,
        report.duration_s,
        &Control {
            offline: true,
            ..Default::default()
        },
    )?;
    let speed = report.job.sound_speed();
    let mut maximum_rmse_difference: f64 = 0.;
    for m in &candidate.matches {
        let index = m.event_index.context("Unindexed match")?;
        let event = report
            .events
            .get(index)
            .and_then(|e| e.variant(m.event_variant))
            .context("Unknown observation")?;
        let track = dataset
            .tracks
            .iter()
            .find(|t| t.id == m.aircraft)
            .context("Matched flight absent from selected sources")?;
        let result = diagnostic::pair(candidate, event, track, speed, index, m.event_variant);
        let fitted = result.fit.context("Original match no longer fits")?;
        maximum_rmse_difference = maximum_rmse_difference.max((fitted.rmse_hz - m.rmse_hz).abs());
        ensure!(
            (fitted.rmse_hz - m.rmse_hz).abs() < 1e-6
                && (fitted.coverage - m.coverage).abs() < 1e-12
                && (fitted.fitted_tone_hz - m.fitted_tone_hz).abs() < 1e-6,
            "Original match changed: {index}"
        );
    }
    let mut events = vec![];
    for (index, event) in report.events.iter().enumerate() {
        if candidate
            .matches
            .iter()
            .any(|m| m.event_index == Some(index))
        {
            continue;
        }
        eprintln!("Diagnosing sound {}", index + 1);
        let mut rows = vec![];
        let mut zero_coverage = 0;
        for (variant, observation) in event.variants().enumerate() {
            for track in &dataset.tracks {
                let mut row =
                    diagnostic::pair(candidate, observation, track, speed, index, variant);
                if row.coverage == 0. {
                    zero_coverage += 1;
                    continue;
                }
                // Numeric pair evidence is sufficient; retain original ridges in
                // the report instead of duplicating thousands of modeled curves.
                if let Some(fit) = &mut row.fit {
                    fit.modeled_hz.clear();
                }
                rows.push(row);
            }
        }
        rows.sort_by(|a, b| {
            let error = |r: &diagnostic::PairDiagnostic| {
                r.fit
                    .as_ref()
                    .map_or(f64::INFINITY, |f| f.rmse_hz / f.fitted_tone_hz)
            };
            error(a)
                .total_cmp(&error(b))
                .then(a.aircraft.cmp(&b.aircraft))
                .then(a.event_variant.cmp(&b.event_variant))
        });
        events.push(json!({"event_index":index, "sound_number":index+1,
            "start_s":event.start_s, "end_s":event.end_s, "variants":event.variants().count(),
            "zero_coverage_pairs":zero_coverage, "pairs_with_coverage":rows}));
    }
    let result = json!({
        "scope":"Conditional diagnostics at the saved app-inferred point and clock; no new broad-area search or independent location acceptance.",
        "source_report":input, "selected_reference_sources":sources, "track_count":dataset.tracks.len(),
        "reference_warnings":dataset.warnings, "candidate":candidate,
        "reproduced_matches":candidate.matches.len(), "maximum_original_rmse_difference_hz":maximum_rmse_difference,
        "assessment":soundgeo::doppler_assessment::assess(&report), "unmatched_events":events,
        "limitations":"Final-point coarse gates do not reconstruct proposal pruning. A passing absent pair identifies a search/assignment omission, not its exact historical stage. Rejected curves alone do not identify the physical sound source."
    });
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(BufWriter::new(file), &result)?;
    Ok(())
}
