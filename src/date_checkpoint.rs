//! Optional recovery of complete offline date batches. Partial searches are never reused.
use crate::{Control, Job, Report, timing};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const SCHEMA: u32 = 2;

// Every value has an explicit type, so legitimate strings and objects cannot be
// mistaken for floating-point markers. Only checkpoint payloads use this format;
// normal reports and reference-file number parsing retain their existing behavior.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExactValue {
    Null,
    Bool(bool),
    String(String),
    Signed(i64),
    Unsigned(u64),
    FloatBits(String),
    Array(Vec<ExactValue>),
    Object(BTreeMap<String, ExactValue>),
}

impl ExactValue {
    fn from_value(value: serde_json::Value) -> Result<Self> {
        use serde_json::Value;
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(value),
            Value::String(value) => Self::String(value),
            Value::Number(value) if value.is_f64() => {
                Self::FloatBits(format!("{:016x}", value.as_f64().unwrap().to_bits()))
            }
            Value::Number(value) => match value.as_u64() {
                Some(value) => Self::Unsigned(value),
                None => Self::Signed(value.as_i64().context("Invalid checkpoint integer")?),
            },
            Value::Array(values) => Self::Array(
                values
                    .into_iter()
                    .map(Self::from_value)
                    .collect::<Result<_>>()?,
            ),
            Value::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, Self::from_value(value)?)))
                    .collect::<Result<_>>()?,
            ),
        })
    }

    fn into_value(self) -> Result<serde_json::Value> {
        use serde_json::Value;
        Ok(match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::String(value) => Value::String(value),
            Self::Signed(value) => Value::Number(value.into()),
            Self::Unsigned(value) => Value::Number(value.into()),
            Self::FloatBits(bits) => {
                ensure!(
                    bits.len() == 16,
                    "Invalid checkpoint floating-point bit pattern"
                );
                let value = f64::from_bits(
                    u64::from_str_radix(&bits, 16)
                        .context("Invalid checkpoint floating-point bit pattern")?,
                );
                Value::Number(
                    serde_json::Number::from_f64(value)
                        .context("Nonfinite floating-point value in checkpoint")?,
                )
            }
            Self::Array(values) => Value::Array(
                values
                    .into_iter()
                    .map(Self::into_value)
                    .collect::<Result<_>>()?,
            ),
            Self::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, value.into_value()?)))
                    .collect::<Result<_>>()?,
            ),
        })
    }
}

fn encode_report(report: &Report) -> Result<String> {
    Ok(serde_json::to_string(&ExactValue::from_value(
        serde_json::to_value(report)?,
    )?)?)
}

fn decode_report(payload: &str) -> Result<Report> {
    let encoded: ExactValue = serde_json::from_str(payload)?;
    // Deserializing from Value transfers f64 directly, without a decimal parser.
    Ok(serde_json::from_value(encoded.into_value()?)?)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Object {
    path: PathBuf,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    key: String,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    schema: u32,
    key: String,
    index: usize,
    batch_key: String,
    complete: bool,
    inputs: Vec<Object>,
    report_sha256: String,
    // Hash the original serialized bytes, avoiding a checksum dependent on parsing.
    report: String,
}

pub(crate) struct Checkpoints {
    root: PathBuf,
    key: String,
    shared_paths: Vec<PathBuf>,
    shared: Vec<Object>,
    // Every reused or newly completed batch is checked again before returning a report.
    completed: Vec<(Vec<PathBuf>, Vec<Object>)>,
}

fn hash_file(path: &Path, control: &Control) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("Could not read checkpoint input {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 262144];
    loop {
        control.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

// Keep roots, rather than just a list of existing files, so added terrain/reference
// files invalidate a checkpoint too. Traversal errors cannot silently omit inputs.
fn inventory(roots: &[PathBuf], control: &Control) -> Result<Vec<Object>> {
    let mut paths = BTreeSet::new();
    for root in roots {
        control.check()?;
        if root.is_dir() {
            for entry in walkdir::WalkDir::new(root).follow_links(false) {
                let entry = entry?;
                ensure!(
                    !entry.file_type().is_symlink(),
                    "Checkpoint inputs cannot contain symbolic links: {}",
                    entry.path().display()
                );
                if entry.file_type().is_file() {
                    paths.insert(entry.path().canonicalize()?);
                }
            }
        } else {
            paths.insert(root.canonicalize()?);
        }
    }
    paths
        .into_iter()
        .map(|path| {
            Ok(Object {
                sha256: hash_file(&path, control)?,
                path,
            })
        })
        .collect()
}

fn write_new(path: &Path, value: &impl Serialize) -> Result<()> {
    let (mut temporary, destination) = crate::atomic_file::prepare(path)?;
    serde_json::to_writer(temporary.as_file_mut(), value)?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist_noclobber(destination).with_context(|| {
        format!(
            "Could not publish checkpoint {}; another writer may be using this folder",
            path.display()
        )
    })?;
    Ok(())
}

impl Checkpoints {
    pub(crate) fn open(
        root: &Path,
        job: &Job,
        template: &Report,
        peaks: &Option<Vec<timing::Peak>>,
        windows: &[(String, String)],
        control: &Control,
    ) -> Result<Self> {
        ensure!(
            control.offline,
            "Date checkpoints require offline analysis of prepared archives"
        );
        // Preparing data online is intentionally separate from recovery. The input
        // files below must already exist and stay unchanged for the entire search.
        let mut shared_paths = vec![std::env::current_exe()?, job.media.clone()];
        if let Some(path) = &job.type_catalog {
            shared_paths.push(path.clone());
        }
        if let Some(path) = &job.terrain {
            shared_paths.push(path.clone());
        }
        if let Some(path) = &job.ffmpeg
            && path.is_file()
        {
            shared_paths.push(path.clone());
        }
        let shared = inventory(&shared_paths, control)?;
        let key = digest(&serde_json::to_vec(&(
            SCHEMA,
            job,
            template.duration_s.to_bits(),
            &template.events,
            peaks,
            windows,
            &shared,
        ))?);
        let manifest_path = root.join("manifest.json");
        if root.exists() {
            ensure!(root.is_dir(), "Date checkpoint path must be a folder");
            if manifest_path.exists() {
                let manifest: Manifest = serde_json::from_reader(File::open(&manifest_path)?)
                    .context("Invalid date checkpoint manifest; preserve it and use a new checkpoint folder")?;
                ensure!(
                    manifest.schema == SCHEMA && manifest.key == key,
                    "Date checkpoint executable, plan or inputs changed; preserve this folder and use a new checkpoint folder"
                );
            } else {
                ensure!(
                    std::fs::read_dir(root)?.next().is_none(),
                    "Date checkpoint folder is not empty and has no manifest; use a new checkpoint folder"
                );
                write_new(
                    &manifest_path,
                    &Manifest {
                        schema: SCHEMA,
                        key: key.clone(),
                    },
                )?;
            }
        } else {
            std::fs::create_dir_all(root)?;
            write_new(
                &manifest_path,
                &Manifest {
                    schema: SCHEMA,
                    key: key.clone(),
                },
            )?;
        }
        let root = root.canonicalize()?;
        // A checkpoint written inside an input directory would change its own key.
        for path in &shared_paths {
            if path.is_dir() {
                ensure!(
                    !root.starts_with(path.canonicalize()?),
                    "Keep date checkpoints outside input folders"
                );
            }
        }
        Ok(Self {
            root,
            key,
            shared_paths,
            shared,
            completed: vec![],
        })
    }

    pub(crate) fn prepare_batch(
        &self,
        job: &Job,
        sources: &[PathBuf],
        control: &Control,
    ) -> Result<Batch> {
        self.verify_shared(control)?;
        // Archive providers place the files, split parts, coverage manifests and
        // inferred aircraft catalog in each source's parent folder. Bind that full
        // folder, including metadata used for filtering and report warnings.
        let mut roots = BTreeSet::new();
        for source in sources {
            let root = if source.is_dir() && source.file_name().is_none_or(|name| name != "traces")
            {
                source.as_path()
            } else {
                source
                    .parent()
                    .context("Archive source has no parent folder")?
            }
            .canonicalize()?;
            ensure!(
                !self.root.starts_with(&root),
                "Keep date checkpoints outside reference folders"
            );
            roots.insert(root);
        }
        let roots: Vec<_> = roots.into_iter().collect();
        let inputs = inventory(&roots, control)?;
        let key = digest(&serde_json::to_vec(&(job, sources, &inputs))?);
        let mut prepared = job.clone();
        if prepared.type_catalog.is_none() {
            prepared.type_catalog = sources
                .iter()
                .filter_map(|p| p.parent())
                .map(|p| p.join("aircraft-types.json"))
                .find(|p| p.is_file());
        }
        Ok(Batch {
            roots,
            inputs,
            key,
            job: job.clone(),
            prepared,
        })
    }

    pub(crate) fn load(&self, index: usize, batch: &Batch) -> Result<Option<Report>> {
        let path = self.root.join(format!("batch-{index:04}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let stored: Stored = serde_json::from_reader(File::open(path)?).context(
            "Invalid date batch checkpoint; preserve it and use a new checkpoint folder",
        )?;
        ensure!(
            stored.schema == SCHEMA
                && stored.key == self.key
                && stored.index == index
                && stored.batch_key == batch.key
                && stored.complete,
            "Invalid date batch checkpoint binding; use a new checkpoint folder"
        );
        ensure!(
            stored.inputs == batch.inputs,
            "Date batch reference inputs changed; preserve this folder and use a new checkpoint folder"
        );
        ensure!(
            digest(stored.report.as_bytes()) == stored.report_sha256,
            "Date batch checkpoint checksum failed; preserve it and use a new checkpoint folder"
        );
        let report =
            decode_report(&stored.report).context("Invalid report in date batch checkpoint")?;
        batch.validate_report(&report)?;
        ensure!(
            report.doppler_partitions.iter().all(|p| p.complete),
            "Date batch checkpoint contains an incomplete proposal partition"
        );
        Ok(Some(report))
    }

    pub(crate) fn finish(
        &mut self,
        index: usize,
        batch: Batch,
        report: &Report,
        reused: bool,
        control: &Control,
    ) -> Result<()> {
        self.verify_shared(control)?;
        ensure!(
            inventory(&batch.roots, control)? == batch.inputs,
            "Reference inputs changed during date batch; no checkpoint or final report was saved"
        );
        if !reused {
            batch.validate_report(report)?;
            ensure!(
                report.doppler_partitions.iter().all(|p| p.complete),
                "Cannot checkpoint an incomplete proposal partition"
            );
            let report = encode_report(report)?;
            write_new(
                &self.root.join(format!("batch-{index:04}.json")),
                &Stored {
                    schema: SCHEMA,
                    key: self.key.clone(),
                    index,
                    batch_key: batch.key.clone(),
                    complete: true,
                    inputs: batch.inputs.clone(),
                    report_sha256: digest(report.as_bytes()),
                    report,
                },
            )?;
        }
        self.completed.push((batch.roots, batch.inputs));
        Ok(())
    }

    fn verify_shared(&self, control: &Control) -> Result<()> {
        ensure!(
            inventory(&self.shared_paths, control)? == self.shared,
            "Shared inputs changed during date search; no new checkpoint or final report was saved"
        );
        Ok(())
    }

    pub(crate) fn verify_complete_inputs(&self, control: &Control) -> Result<()> {
        self.verify_shared(control)?;
        for (roots, original) in &self.completed {
            ensure!(
                inventory(roots, control)? == *original,
                "Reference inputs changed during date search; no final report was returned"
            );
        }
        Ok(())
    }
}

pub(crate) struct Batch {
    roots: Vec<PathBuf>,
    inputs: Vec<Object>,
    key: String,
    job: Job,
    prepared: Job,
}

impl Batch {
    fn validate_report(&self, report: &Report) -> Result<()> {
        let actual = serde_json::to_vec(&report.job)?;
        let prepared = serde_json::to_vec(&self.prepared)?;
        let early_empty = report.track_count == 0
            && report.candidate_count() == 0
            && actual == serde_json::to_vec(&self.job)?;
        ensure!(
            actual == prepared || early_empty,
            "Date batch checkpoint report has a different effective job"
        );
        ensure!(
            report.date_search.is_none() && report.media == self.job.media,
            "Date batch checkpoint report has an invalid batch scope"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_encoding_preserves_float_bits_integers_and_literal_marker_values() {
        use serde_json::{Value, json};
        let mut bits = vec![
            0,
            1,
            1u64 << 63,
            f64::MIN_POSITIVE.to_bits(),
            f64::MAX.to_bits(),
            0.1f64.to_bits(),
            0.9228645712621525f64.to_bits(),
        ];
        let mut seed = 0x8127_1839_abc7_132fu64;
        for _ in 0..128 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            if f64::from_bits(seed).is_finite() {
                bits.push(seed);
            }
        }
        let original = json!({
            "floats": bits.iter().map(|bits| f64::from_bits(*bits)).collect::<Vec<_>>(),
            "minimum_integer": i64::MIN, "maximum_integer": u64::MAX,
            "marker_object": {"float_bits":"7ff0000000000000", "object":{"string":"null"}},
            "marker_strings": ["null", "$soundgeo_f64", "float_bits", "0000000000000001"],
            "literal_values": [Value::Null, Value::Bool(true), json!({}), json!([])],
        });
        let wire = serde_json::to_vec(&ExactValue::from_value(original.clone()).unwrap()).unwrap();
        let restored = serde_json::from_slice::<ExactValue>(&wire)
            .unwrap()
            .into_value()
            .unwrap();
        assert_eq!(restored, original);
        assert_eq!(restored["minimum_integer"].as_i64(), Some(i64::MIN));
        assert_eq!(restored["maximum_integer"].as_u64(), Some(u64::MAX));
        for (expected, restored) in bits.iter().zip(restored["floats"].as_array().unwrap()) {
            assert_eq!(restored.as_f64().unwrap().to_bits(), *expected);
        }
        for invalid in [
            "7ff0000000000000",
            "fff0000000000000",
            "7ff8000000000001",
            "invalid",
            "000000000000000z",
        ] {
            assert!(ExactValue::FloatBits(invalid.into()).into_value().is_err());
        }
    }

    fn template(job: &Job) -> Report {
        Report {
            version: env!("CARGO_PKG_VERSION").into(),
            status: "insufficient_evidence".into(),
            media: job.media.clone(),
            job: job.clone(),
            duration_s: 123.45678901234567,
            track_count: 0,
            events: vec![],
            candidates: vec![],
            warnings: vec![],
            search_grid_m: job.grid_m,
            search_method: String::new(),
            hypotheses: 0,
            created_utc: "2026-11-10T00:00:00Z".into(),
            reference_sources: vec![],
            terrain_sources: vec![],
            timing: None,
            date_search: None,
            doppler_partitions: vec![],
        }
    }

    #[test]
    fn checkpoint_binds_terrain_media_and_parent_metadata_and_detects_midrun_changes() {
        let dir = tempfile::tempdir().unwrap();
        let terrain = dir.path().join("terrain");
        let source = dir.path().join("day/traces");
        std::fs::create_dir_all(&terrain).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        let media = dir.path().join("recording.wav");
        std::fs::write(&media, b"original media bytes").unwrap();
        std::fs::write(source.join("trace_full_a.json"), b"original trace").unwrap();
        let metadata = source.parent().unwrap().join("provider-manifest.json");
        std::fs::write(&metadata, b"original metadata").unwrap();
        let job = Job {
            media,
            terrain: Some(terrain.clone()),
            ..Job::default()
        };
        let report = template(&job);
        let windows = vec![("2026-11-09".into(), "2026-11-09".into())];
        let control = Control::default().with_offline(true);
        let root = dir.path().join("checkpoints");
        let open = || Checkpoints::open(&root, &job, &report, &None, &windows, &control);
        let mut store = open().unwrap();
        let sources = vec![source];
        let batch = store.prepare_batch(&job, &sources, &control).unwrap();
        store.finish(0, batch, &report, false, &control).unwrap();
        let original = std::fs::read(root.join("batch-0000.json")).unwrap();
        // A source directory's parent carries coverage metadata used in reports.
        std::fs::write(&metadata, b"changed metadata").unwrap();
        let batch = store.prepare_batch(&job, &sources, &control).unwrap();
        assert!(store.load(0, &batch).is_err());
        assert!(store.verify_complete_inputs(&control).is_err());
        std::fs::write(metadata, b"original metadata").unwrap();
        let tile = terrain.join("N00E000.hgt");
        std::fs::write(&tile, b"new terrain tile").unwrap();
        assert!(
            open().is_err(),
            "Adding a previously missing terrain tile must invalidate recovery"
        );
        std::fs::remove_file(tile).unwrap();
        let batch = store.prepare_batch(&job, &sources, &control).unwrap();
        let tile = terrain.join("N00E000.hgt");
        std::fs::write(&tile, b"terrain changed during calculation").unwrap();
        assert!(store.finish(1, batch, &report, false, &control).is_err());
        assert!(!root.join("batch-0001.json").exists());
        std::fs::remove_file(tile).unwrap();
        let batch = store.prepare_batch(&job, &sources, &control).unwrap();
        assert!(
            store.load(1, &batch).unwrap().is_none(),
            "Restoring shared inputs must not uncover a poisoned record"
        );
        std::fs::write(&job.media, b"changed media bytes").unwrap();
        assert!(open().is_err());
        std::fs::write(&job.media, b"original media bytes").unwrap();
        let batch = store.prepare_batch(&job, &sources, &control).unwrap();
        std::fs::write(sources[0].join("trace_full_new.json"), b"new trace").unwrap();
        assert!(store.finish(1, batch, &report, false, &control).is_err());
        assert!(!root.join("batch-0001.json").exists());
        assert_eq!(
            std::fs::read(root.join("batch-0000.json")).unwrap(),
            original
        );
    }

    #[test]
    fn checkpoint_rejects_incomplete_partition_and_changed_effective_job() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("day");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("flights.csv"), b"reference").unwrap();
        let media = dir.path().join("recording.wav");
        std::fs::write(&media, b"media").unwrap();
        let job = Job {
            media,
            ..Job::default()
        };
        let mut report = template(&job);
        let windows = vec![("2026-11-09".into(), "2026-11-09".into())];
        let control = Control::default().with_offline(true);
        let root = dir.path().join("checkpoints");
        let mut store = Checkpoints::open(&root, &job, &report, &None, &windows, &control).unwrap();
        let sources = vec![source];
        report
            .doppler_partitions
            .push(crate::index::ProposalPartition {
                complete: false,
                max_hypotheses_per_batch: 100,
                batches: vec![],
            });
        let batch = store.prepare_batch(&job, &sources, &control).unwrap();
        assert!(store.finish(0, batch, &report, false, &control).is_err());
        report.doppler_partitions.clear();
        report.job.temperature_c += 1.;
        let batch = store.prepare_batch(&job, &sources, &control).unwrap();
        assert!(store.finish(0, batch, &report, false, &control).is_err());
        assert!(!root.join("batch-0000.json").exists());
    }
}
