//! Recovery of fully matched and refined proposal batches within an offline date.
use crate::{
    Control, Job, audio::Event, date_checkpoint, index::ProposalPartition, search::SearchResult,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    key: String,
    // Retain the exact plan and input inventory for a separate artifact audit.
    binding: String,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    schema: u32,
    key: String,
    partition_sha256: String,
    index: usize,
    complete: bool,
    payload_sha256: String,
    payload: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Completed {
    pub(crate) result: SearchResult,
    pub(crate) missing_terrain_queries: usize,
}

pub(crate) struct Checkpoints {
    root: PathBuf,
    roots: Vec<PathBuf>,
    inputs: Vec<date_checkpoint::Object>,
    key: String,
    grid_bits: u64,
}

impl Checkpoints {
    // Called before loading reference tracks: a mutation during loading must not
    // bind a result computed from old tracks to newly changed source bytes.
    pub(crate) fn open(
        parent: &Path,
        job: &Job,
        events: &[Event],
        duration_s: f64,
        sources: &[PathBuf],
        control: &Control,
    ) -> Result<Self> {
        ensure!(
            control.offline,
            "Proposal checkpoints require offline analysis"
        );
        let archive = job
            .archive
            .as_ref()
            .context("Proposal checkpoints require a date batch")?;
        ensure!(
            !archive.date_from.is_empty(),
            "Proposal checkpoints require a resolved date batch"
        );
        let date_key = date_checkpoint::digest(&serde_json::to_vec(&(
            &archive.date_from,
            &archive.date_to,
        ))?);
        let root = parent.canonicalize()?.join(format!("proposals-{date_key}"));
        let mut roots = date_checkpoint::reference_roots(sources)?;
        roots.extend([std::env::current_exe()?, job.media.clone()]);
        roots.extend(job.terrain.iter().cloned());
        roots.extend(job.type_catalog.iter().cloned());
        if let Some(path) = &job.ffmpeg
            && path.is_file()
        {
            roots.push(path.clone());
        }
        for input in &roots {
            if input.is_dir() {
                ensure!(
                    !root.starts_with(input.canonicalize()?),
                    "Keep proposal checkpoints outside input folders"
                );
            }
        }
        let inputs = date_checkpoint::inventory(&roots, control)?;
        let binding =
            date_checkpoint::encode_exact(&(SCHEMA, job, events, duration_s, sources, &inputs))?;
        let key = date_checkpoint::digest(binding.as_bytes());
        let manifest_path = root.join("manifest.json");
        if root.exists() {
            ensure!(root.is_dir(), "Proposal checkpoint path must be a folder");
            if manifest_path.exists() {
                let saved: Manifest = serde_json::from_reader(File::open(&manifest_path)?)
                    .context("Invalid proposal checkpoint manifest; preserve this folder")?;
                ensure!(
                    saved.schema == SCHEMA
                        && saved.key == key
                        && date_checkpoint::digest(saved.binding.as_bytes()) == key,
                    "Proposal checkpoint executable, plan, observations or inputs changed; preserve this folder and use a new checkpoint folder"
                );
            } else {
                ensure!(
                    std::fs::read_dir(&root)?.next().is_none(),
                    "Proposal checkpoint folder has no manifest; preserve this folder"
                );
                date_checkpoint::write_new(
                    &manifest_path,
                    &Manifest {
                        schema: SCHEMA,
                        key: key.clone(),
                        binding: binding.clone(),
                    },
                )?;
            }
        } else {
            std::fs::create_dir(&root)?;
            date_checkpoint::write_new(
                &manifest_path,
                &Manifest {
                    schema: SCHEMA,
                    key: key.clone(),
                    binding,
                },
            )?;
        }
        Ok(Self {
            root,
            roots,
            inputs,
            key,
            grid_bits: job.grid_m.to_bits(),
        })
    }

    pub(crate) fn verify(&self, control: &Control) -> Result<()> {
        ensure!(
            date_checkpoint::inventory(&self.roots, control)? == self.inputs,
            "Proposal checkpoint inputs changed during search; no checkpoint or final report was saved"
        );
        Ok(())
    }

    fn path(&self, index: usize) -> PathBuf {
        self.root.join(format!("proposal-{index:04}.json"))
    }

    fn validate(
        &self,
        index: usize,
        plan: &ProposalPartition,
        result: &SearchResult,
    ) -> Result<()> {
        ensure!(plan.complete, "Cannot reuse an incomplete proposal plan");
        let batch = plan
            .batches
            .iter()
            .filter(|batch| batch.hypotheses > 0)
            .nth(index)
            .context("Proposal checkpoint index is outside the plan")?;
        ensure!(
            result.hypotheses == batch.hypotheses
                && result.coarse_grid_m.to_bits() == self.grid_bits
                && result.method == "indexed_flight_corridors"
                && result.partition.is_none(),
            "Proposal checkpoint result does not match its complete planned batch"
        );
        Ok(())
    }

    pub(crate) fn load(
        &self,
        index: usize,
        plan: &ProposalPartition,
        control: &Control,
    ) -> Result<Option<Completed>> {
        let path = self.path(index);
        if !path.exists() {
            return Ok(None);
        }
        self.verify(control)?;
        let stored: Stored = serde_json::from_reader(File::open(path)?)
            .context("Invalid proposal checkpoint; preserve this folder")?;
        let plan_hash = date_checkpoint::digest(date_checkpoint::encode_exact(plan)?.as_bytes());
        ensure!(
            stored.schema == SCHEMA
                && stored.key == self.key
                && stored.index == index
                && stored.partition_sha256 == plan_hash
                && stored.complete,
            "Invalid proposal checkpoint binding; preserve this folder"
        );
        ensure!(
            date_checkpoint::digest(stored.payload.as_bytes()) == stored.payload_sha256,
            "Proposal checkpoint checksum failed; preserve this folder"
        );
        let completed: Completed = date_checkpoint::decode_exact(&stored.payload)?;
        self.validate(index, plan, &completed.result)?;
        Ok(Some(completed))
    }

    pub(crate) fn save(
        &self,
        index: usize,
        plan: &ProposalPartition,
        completed: &Completed,
        control: &Control,
    ) -> Result<()> {
        self.validate(index, plan, &completed.result)?;
        self.verify(control)?;
        let payload = date_checkpoint::encode_exact(completed)?;
        date_checkpoint::write_new(
            &self.path(index),
            &Stored {
                schema: SCHEMA,
                key: self.key.clone(),
                partition_sha256: date_checkpoint::digest(
                    date_checkpoint::encode_exact(plan)?.as_bytes(),
                ),
                index,
                complete: true,
                payload_sha256: date_checkpoint::digest(payload.as_bytes()),
                payload,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        archive::{ArchiveProvider, ArchiveRequest},
        index::ProposalBatch,
        search::Candidate,
    };

    fn fixture(root: &Path) -> (Job, Vec<PathBuf>, PathBuf) {
        let source = root.join("references");
        std::fs::create_dir(&source).unwrap();
        let path = source.join("flights.csv");
        std::fs::write(&path, b"fixed reference bytes").unwrap();
        let media = root.join("media.wav");
        std::fs::write(&media, b"fixed media bytes").unwrap();
        let parent = root.join("checkpoints");
        std::fs::create_dir(&parent).unwrap();
        let job = Job {
            media,
            archive: Some(ArchiveRequest {
                provider: ArchiveProvider::WebtrakChicago,
                cache: source,
                date_from: "2026-09-08".into(),
                date_to: Some("2026-09-08".into()),
            }),
            ..Default::default()
        };
        (job, vec![path], parent)
    }

    fn plan() -> ProposalPartition {
        ProposalPartition {
            complete: true,
            max_hypotheses_per_batch: 100,
            batches: vec![ProposalBatch {
                emission_from_unix: 1.,
                emission_to_unix: 2.,
                hypotheses: 12,
            }],
        }
    }

    fn completed(job: &Job) -> Completed {
        Completed {
            missing_terrain_queries: 17,
            result: SearchResult {
                candidates: vec![Candidate {
                    latitude: -0.0,
                    longitude: 0.9228645712621525,
                    ground_m: f64::MIN_POSITIVE,
                    recording_start_utc: "literal float_bits".into(),
                    recording_start_unix: 123.45678901234567,
                    recording_clock_rate: 1.,
                    relative_error_percent: 0.1,
                    matches: vec![],
                    silence_conflicts: vec![],
                }],
                warnings: vec!["fixed warning".into()],
                coarse_grid_m: job.grid_m,
                hypotheses: 12,
                method: "indexed_flight_corridors".into(),
                partition: None,
            },
        }
    }

    #[test]
    fn proposal_payload_preserves_float_bits_counts_and_rejects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let (job, sources, parent) = fixture(dir.path());
        let control = Control::default().with_offline(true);
        let store = Checkpoints::open(&parent, &job, &[], 123., &sources, &control).unwrap();
        let plan = plan();
        let completed = completed(&job);
        store.save(0, &plan, &completed, &control).unwrap();
        let restored = store.load(0, &plan, &control).unwrap().unwrap();
        assert_eq!(
            date_checkpoint::encode_exact(&restored).unwrap(),
            date_checkpoint::encode_exact(&completed).unwrap()
        );
        assert_eq!(
            restored.result.candidates[0].latitude.to_bits(),
            (-0.0f64).to_bits()
        );
        assert_eq!(restored.missing_terrain_queries, 17);
        let path = store.path(0);
        let original = std::fs::read(&path).unwrap();
        for (field, value) in [
            ("complete", serde_json::json!(false)),
            ("index", serde_json::json!(1)),
            ("partition_sha256", serde_json::json!("wrong plan")),
            ("payload", serde_json::json!("{}")),
        ] {
            let mut damaged: serde_json::Value = serde_json::from_slice(&original).unwrap();
            damaged[field] = value;
            let bytes = serde_json::to_vec(&damaged).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(store.load(0, &plan, &control).is_err(), "{field}");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                bytes,
                "Rejected evidence must be preserved"
            );
        }
        std::fs::write(&path, &original).unwrap();
        let mut changed = plan.clone();
        changed.batches[0].emission_to_unix = 3.;
        assert!(store.load(0, &changed, &control).is_err());
        changed.complete = false;
        assert!(store.save(1, &changed, &completed, &control).is_err());
        assert!(!store.path(1).exists());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn proposal_checkpoints_reject_input_changes_before_publish_and_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let (job, sources, parent) = fixture(dir.path());
        let control = Control::default().with_offline(true);
        let store = Checkpoints::open(&parent, &job, &[], 123., &sources, &control).unwrap();
        std::fs::write(&sources[0], b"changed before publish").unwrap();
        assert!(store.save(0, &plan(), &completed(&job), &control).is_err());
        assert!(!store.path(0).exists());
        std::fs::write(&sources[0], b"fixed reference bytes").unwrap();
        store.save(0, &plan(), &completed(&job), &control).unwrap();
        let original = std::fs::read(store.path(0)).unwrap();
        let added = sources[0].parent().unwrap().join("new-metadata.json");
        std::fs::write(&added, b"{}").unwrap();
        assert!(store.load(0, &plan(), &control).is_err());
        assert!(Checkpoints::open(&parent, &job, &[], 123., &sources, &control).is_err());
        std::fs::remove_file(added).unwrap();
        assert!(store.load(0, &plan(), &control).unwrap().is_some());
        assert_eq!(std::fs::read(store.path(0)).unwrap(), original);
    }

    #[test]
    fn proposal_manifest_binds_duration_settings_and_offline_scope() {
        let dir = tempfile::tempdir().unwrap();
        let (job, sources, parent) = fixture(dir.path());
        let control = Control::default().with_offline(true);
        Checkpoints::open(&parent, &job, &[], 123., &sources, &control).unwrap();
        assert!(Checkpoints::open(&parent, &job, &[], 124., &sources, &control).is_err());
        let mut changed = job.clone();
        changed.temperature_c += 1.;
        assert!(Checkpoints::open(&parent, &changed, &[], 123., &sources, &control).is_err());
        assert!(
            Checkpoints::open(&parent, &job, &[], 123., &sources, &Control::default()).is_err()
        );
        let store = Checkpoints::open(&parent, &job, &[], 123., &sources, &control).unwrap();
        let path = store.root.join("manifest.json");
        let mut manifest: Manifest = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        manifest.binding.push(' ');
        std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(Checkpoints::open(&parent, &job, &[], 123., &sources, &control).is_err());
    }
}
