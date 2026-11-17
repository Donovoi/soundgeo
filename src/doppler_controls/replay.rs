//! Bind diagnostic searches to the completed production date/source plan.
use crate::{
    AnalysisMethod, Control, Progress, Report, audio::Event, date_checkpoint, date_search,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchBinding {
    schema: u32,
    report_sha256: String,
    dates: Vec<String>,
    windows: Vec<(String, String)>,
    batched: bool,
    sources: Vec<Vec<PathBuf>>,
    roots: Vec<PathBuf>,
    inputs: Vec<date_checkpoint::Object>,
}

impl SearchBinding {
    /// Plans, results and optional checkpoints must not mutate their own inputs.
    pub fn check_output_path(&self, path: &Path) -> Result<()> {
        let absolute = std::path::absolute(path)?;
        let target = if absolute.exists() {
            absolute.canonicalize()?
        } else {
            absolute
                .parent()
                .context("Output has no parent")?
                .canonicalize()?
                .join(absolute.file_name().context("Output has no filename")?)
        };
        for root in &self.roots {
            ensure!(
                target != *root && !(root.is_dir() && target.starts_with(root)),
                "Keep control plans, results and checkpoints outside bound input folders"
            );
        }
        Ok(())
    }
}

fn canonical_sources(sources: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    sources
        .iter()
        .map(|p| p.canonicalize().map_err(Into::into))
        .collect()
}

/// Bind the *current* verified input bytes and resolved scope to a complete report.
/// Old reports do not establish what raw input bytes existed at their creation.
pub fn bind_search(report: &Report, control: &Control) -> Result<SearchBinding> {
    let control = &control.with_offline(true);
    let job = &report.job;
    job.validate()?;
    ensure!(
        job.start_utc.is_none(),
        "Controls require an unknown recording clock"
    );
    ensure!(
        job.archive
            .as_ref()
            .is_none_or(|a| a.date_from.is_empty() && a.date_to.is_none()),
        "Controls require no supplied recording date"
    );
    ensure!(
        report.timing.is_none() && job.analysis_method == AnalysisMethod::Doppler,
        "Expected a Doppler report"
    );
    ensure!(
        report.duration_s.is_finite() && report.duration_s > 0.,
        "Invalid report duration"
    );
    ensure!(
        !report.reference_sources.is_empty() && !report.search_method.is_empty(),
        "Controls require a completed reference search"
    );
    ensure!(
        report.doppler_partitions.iter().all(|p| p.complete),
        "Controls cannot use incomplete proposal partitions"
    );
    let mut dates = vec![];
    let mut windows = vec![];
    let mut sources = vec![];
    let mut batched = false;
    if let Some(request) = &job.archive {
        dates = request.resolve_dates(control, &|_, _| {})?.0;
        if let Some(coverage) = &report.date_search {
            ensure!(
                coverage.complete && coverage.dates == dates,
                "Prepared date plan differs from the completed baseline"
            );
            windows = date_search::batches(&dates)?;
            ensure!(
                coverage.completed_batches == windows.len(),
                "Baseline date batches are incomplete"
            );
            ensure!(
                dates.len() > 1 || coverage.provider_discovery.is_some(),
                "Baseline date batching cannot be reproduced"
            );
            batched = true;
            for (from, to) in &windows {
                let mut batch = request.clone();
                batch.date_from = from.clone();
                batch.date_to = Some(to.clone());
                sources.push(batch.ensure_for_bounds(job.bounds, control, &|_, _| {})?);
            }
        } else {
            ensure!(
                dates.len() == 1,
                "Multi-date baseline has no complete date coverage"
            );
            sources.push(request.ensure_for_bounds(job.bounds, control, &|_, _| {})?);
        }
    } else {
        ensure!(
            report.date_search.is_none(),
            "Imported baseline has unexpected date coverage"
        );
        sources.push(vec![job.tracks.clone()]);
    }
    let all_sources: Vec<_> = sources.iter().flatten().cloned().collect();
    ensure!(
        canonical_sources(&all_sources)? == canonical_sources(&report.reference_sources)?,
        "Prepared reference sources differ from the completed baseline"
    );
    let mut roots = if job.archive.is_some() {
        date_checkpoint::reference_roots(&all_sources)?
    } else {
        all_sources.clone()
    };
    roots.extend([std::env::current_exe()?, job.media.clone()]);
    roots.extend(job.terrain.iter().cloned());
    roots.extend(job.type_catalog.iter().cloned());
    if let Some(ffmpeg) = &job.ffmpeg
        && ffmpeg.is_file()
    {
        roots.push(ffmpeg.clone());
    }
    for source in &all_sources {
        if let Some(parent) = source.parent() {
            let catalog = parent.join("aircraft-types.json");
            if catalog.is_file() {
                roots.push(catalog);
            }
        }
    }
    roots = roots
        .iter()
        .map(|p| p.canonicalize())
        .collect::<std::io::Result<_>>()?;
    roots.sort();
    roots.dedup();
    let inputs = date_checkpoint::inventory(&roots, control)?;
    Ok(SearchBinding {
        schema: 1,
        report_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(report)?)),
        dates,
        windows,
        batched,
        sources,
        roots,
        inputs,
    })
}

/// Replay supplied observations through the production post-detection pipeline.
/// This does not decode audio or modify acceptance rules. The example runner
/// supplies only validated whole-activity permutations; identity is useful for
/// checking that a baseline and a control use the same search implementation.
pub fn replay_observations(
    source: &Report,
    binding: &SearchBinding,
    events: Vec<Event>,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Report> {
    let control = &control.with_offline(true);
    ensure!(
        *binding == bind_search(source, control)?,
        "Control executable, report, date plan or inputs changed; preserve this plan and create a new one"
    );
    ensure!(
        events.len() == source.events.len(),
        "Control observations changed in count"
    );
    if let Some(path) = &control.date_checkpoints {
        ensure!(
            binding.batched && binding.dates.len() > 1,
            "Control checkpoints require a multi-date baseline"
        );
        binding.check_output_path(path)?;
    }
    let mut job = source.job.clone();
    job.offline = true;
    let resolved = job.archive.as_ref().map(|_| {
        (
            binding.dates.clone(),
            source
                .date_search
                .as_ref()
                .and_then(|c| c.provider_discovery.clone()),
        )
    });
    let report = crate::analyze_observations(
        &job,
        source.duration_s,
        events,
        None,
        resolved,
        control,
        progress,
    )?;
    ensure!(
        *binding == bind_search(source, control)?,
        "Control inputs or date plan changed during search; no result was saved"
    );
    ensure!(
        report.doppler_partitions.iter().all(|p| p.complete),
        "Control search returned incomplete proposal partitions"
    );
    if binding.batched {
        ensure!(
            report.date_search.as_ref().is_some_and(|c| c.complete
                && c.dates == binding.dates
                && c.completed_batches == binding.windows.len()),
            "Control date search is incomplete"
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        archive::{ArchiveProvider, ArchiveRequest},
        audio::RidgePoint,
        demo,
    };
    use std::sync::Mutex;

    #[test]
    fn nonidentity_control_replays_exact_overlapping_date_windows() {
        let root = tempfile::tempdir().unwrap();
        let mut job = demo::create(root.path()).unwrap();
        job.offline = true;
        job.grid_m = 5000.;
        job.search_mode = crate::search::SearchMode::RegionalGrid;
        let cache = root.path().join("cache");
        for (day, shift) in [
            ("2026-09-08", 0.),
            ("2026-09-09", 86400.),
            ("2026-09-10", 172800.),
        ] {
            let folder = cache.join(format!("webtrak-cda-{day}"));
            std::fs::create_dir_all(&folder).unwrap();
            let mut csv = "aircraft,time,lat,lon,altitude_m\n".to_string();
            for track in demo::tracks() {
                for sample in track.samples {
                    csv.push_str(&format!(
                        "{},{},{},{},{}\n",
                        track.id,
                        sample.time + shift,
                        sample.point.lat,
                        sample.point.lon,
                        sample.point.alt
                    ));
                }
            }
            std::fs::write(folder.join("flights.csv"), &csv).unwrap();
            std::fs::write(
                folder.join("aircraft-types.json"),
                br#"{"C172":["Cessna","L1P","L"]}"#,
            )
            .unwrap();
            std::fs::write(folder.join("provider-manifest.json"), serde_json::to_vec(&serde_json::json!({
                "format":1,"date":day,"provider":"webtrak_chicago","complete":true,
                "chunks":2880,"rows":262,"offline_chunks":0,
                "csv_sha256":format!("{:x}", Sha256::digest(csv.as_bytes())),
                "source":"https://webtrak-us02.emsbk.com/WebTrak/cda/data/","altitude":"Synthetic WGS84"
            })).unwrap()).unwrap();
        }
        job.archive = Some(ArchiveRequest {
            date_from: String::new(),
            date_to: None,
            cache,
            provider: ArchiveProvider::WebtrakChicago,
        });
        let events = [20., 160., 320., 500., 700.]
            .into_iter()
            .map(|start| Event {
                start_s: start,
                end_s: start + 20.,
                cpa_s: start + 10.,
                base_hz: 100.,
                speed_mps: 70.,
                slant_m: 1000.,
                fit_rmse_hz: 0.1,
                contrast_db: 12.,
                ridge: vec![
                    RidgePoint {
                        time_s: start,
                        hz: 101.,
                        contrast_db: 10.,
                    },
                    RidgePoint {
                        time_s: start + 20.,
                        hz: 99.,
                        contrast_db: 12.,
                    },
                ],
                activity_span_s: None,
                alternatives: vec![],
            })
            .collect();
        let control = Control::default().with_offline(true);
        let baseline =
            crate::analyze_observations(&job, 900., events, None, None, &control, &|_, _| {})
                .unwrap();
        let binding = bind_search(&baseline, &control).unwrap();
        assert!(baseline.track_count > 0 && baseline.hypotheses > 0);
        assert_eq!(
            binding.windows,
            [
                ("2026-09-08".into(), "2026-09-09".into()),
                ("2026-09-09".into(), "2026-09-10".into())
            ]
        );
        let plan = crate::doppler_controls::plan(&baseline.events, baseline.duration_s).unwrap();
        let (shuffled, shifts) =
            crate::doppler_controls::apply(&baseline.events, baseline.duration_s, &plan, 0)
                .unwrap();
        assert!(shifts.iter().any(|s| s.seconds != 0.));
        let messages = Mutex::new(vec![]);
        let checkpoints = root.path().join("control-00-checkpoints");
        let result = replay_observations(
            &baseline,
            &binding,
            shuffled.clone(),
            &control.with_date_checkpoints(checkpoints.clone()),
            &|m, _| messages.lock().unwrap().push(m.to_string()),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&result.events).unwrap(),
            serde_json::to_value(shuffled).unwrap()
        );
        assert_eq!(result.date_search.as_ref().unwrap().completed_batches, 2);
        assert_eq!(result.reference_sources, baseline.reference_sources);
        for (i, (from, to)) in binding.windows.iter().enumerate() {
            let prefix = format!("Dates {from} to {to} ({}/2):", i + 1);
            assert!(
                messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.starts_with(&prefix))
            );
            assert!(checkpoints.join(format!("batch-{i:04}.json")).is_file());
        }
        // An identity replay cannot consume checkpoints created for permuted events.
        assert!(
            replay_observations(
                &baseline,
                &binding,
                baseline.events.clone(),
                &control.with_date_checkpoints(checkpoints),
                &|_, _| {}
            )
            .is_err()
        );
    }
}
