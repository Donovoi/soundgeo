//! Reuse decoded regional tracks after verifying the source bytes and parser.
use super::{Dataset, Filter, Parts, source_paths};
use crate::{
    Control, Job,
    aircraft::AircraftKind,
    geo::Point,
    model::{Sample, Track},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) struct PreparedCache {
    path: PathBuf,
    key: String,
}

#[derive(Serialize, Deserialize)]
struct StoredTrack {
    id: String,
    kind: AircraftKind,
    // Integer bit patterns preserve every floating-point source value exactly.
    samples: Vec<[u64; 4]>,
}
#[derive(Serialize, Deserialize)]
struct Payload {
    tracks: Vec<StoredTrack>,
    warnings: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct Stored {
    key: String,
    payload_sha256: String,
    payload: Payload,
}

fn hash_file(path: &Path, control: &Control) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 262144];
    loop {
        control.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

impl PreparedCache {
    pub(super) fn for_job(
        job: &Job,
        sources: &[PathBuf],
        filter: &Filter,
        control: &Control,
    ) -> Result<Option<Self>> {
        let Some(archive) = &job.archive else {
            return Ok(None);
        };
        if job.start_utc.is_some() {
            return Ok(None);
        }
        let mut paths = Vec::new();
        for path in source_paths(sources) {
            if path.to_string_lossy().ends_with(".tar.aa") {
                paths.extend(Parts::new(&path)?.paths);
            } else {
                paths.push(path);
            }
        }
        paths.sort();
        paths.dedup();
        let mut objects = Vec::new();
        for path in paths {
            objects.push((path.clone(), hash_file(&path, control)?));
        }
        let catalog = job
            .type_catalog
            .as_ref()
            .map(|path| hash_file(path, control))
            .transpose()?;
        let settings = serde_json::to_vec(&(
            filter.bounds,
            filter.aircraft,
            filter.minimum_positions,
            objects,
            catalog,
        ))?;
        let mut digest = Sha256::new();
        digest.update(b"soundgeo-decoded-tracks-v1");
        for source in [
            include_str!("data.rs"),
            include_str!("data_cache.rs"),
            include_str!("model.rs"),
            include_str!("aircraft.rs"),
            include_str!("geo.rs"),
        ] {
            digest.update(source.as_bytes());
        }
        digest.update(settings);
        let key = format!("{:x}", digest.finalize());
        Ok(Some(Self {
            path: archive
                .cache
                .join("decoded-tracks")
                .join(format!("{key}.sgcache")),
            key,
        }))
    }

    pub(super) fn load(&self) -> Result<Option<Dataset>> {
        if !self.path.is_file() || self.path.metadata()?.len() > 256 * 1024 * 1024 {
            return Ok(None);
        }
        let stored: Stored =
            serde_json::from_reader(std::io::BufReader::new(File::open(&self.path)?))?;
        if stored.key != self.key
            || format!("{:x}", Sha256::digest(serde_json::to_vec(&stored.payload)?))
                != stored.payload_sha256
        {
            return Ok(None);
        }
        let mut tracks = Vec::new();
        for track in stored.payload.tracks {
            let mut samples = Vec::new();
            for values in track.samples {
                let [time, lat, lon, alt] = values.map(f64::from_bits);
                let point = Point { lat, lon, alt };
                ensure!(
                    time.is_finite() && (0. ..=4102444800.).contains(&time) && point.valid(),
                    "Invalid prepared sample"
                );
                samples.push(Sample::new(time, point));
            }
            tracks.push(Track {
                id: track.id,
                kind: track.kind,
                samples,
            });
        }
        Ok(Some(Dataset {
            tracks,
            warnings: stored.payload.warnings,
        }))
    }

    pub(super) fn save(&self, dataset: &Dataset, control: &Control) -> Result<()> {
        control.check()?;
        if dataset
            .tracks
            .iter()
            .map(|t| t.samples.len())
            .sum::<usize>()
            > 1_000_000
            || self.load().ok().flatten().is_some()
        {
            return Ok(());
        }
        let payload = Payload {
            tracks: dataset
                .tracks
                .iter()
                .map(|track| StoredTrack {
                    id: track.id.clone(),
                    kind: track.kind,
                    samples: track
                        .samples
                        .iter()
                        .map(|s| {
                            [
                                s.time.to_bits(),
                                s.point.lat.to_bits(),
                                s.point.lon.to_bits(),
                                s.point.alt.to_bits(),
                            ]
                        })
                        .collect(),
                })
                .collect(),
            warnings: dataset.warnings.clone(),
        };
        let stored = Stored {
            key: self.key.clone(),
            payload_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&payload)?)),
            payload,
        };
        let parent = self.path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut temp, &stored)?;
        temp.flush()?;
        control.check()?;
        if self.path.exists() {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            std::fs::rename(
                &self.path,
                self.path.with_extension(format!("rejected-{nonce}")),
            )?;
        }
        temp.persist(&self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisMethod, archive::ArchiveRequest};

    #[test]
    fn cached_tracks_preserve_bits_invalidate_sources_and_repair_corruption() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("flights.csv");
        let csv = "aircraft,time,lat,lon,altitude_m\na,1700000000,61.179633,-149.9719665,123.456789123\na,1700000002,61.179933,-149.9719665,125.456789123\na,1700000004,61.180233,-149.9719665,127.456789123\n";
        std::fs::write(&source, csv).unwrap();
        let mut job = Job {
            tracks: source.clone(),
            archive: Some(ArchiveRequest {
                date_from: String::new(),
                date_to: None,
                cache: root.path().join("cache"),
                provider: Default::default(),
            }),
            analysis_method: AnalysisMethod::PassageTiming,
            smooth_tracks: false,
            ..Default::default()
        };
        let control = Control::default().with_offline(true);
        let cold = super::super::load_for_job(&job, 600., &control).unwrap();
        let cache = std::fs::read_dir(root.path().join("cache/decoded-tracks"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let compare = |a: &Dataset, b: &Dataset| {
            assert_eq!(a.warnings, b.warnings);
            assert_eq!(a.tracks.len(), b.tracks.len());
            for (a, b) in a.tracks.iter().zip(&b.tracks) {
                assert_eq!(a.id, b.id);
                assert_eq!(a.kind, b.kind);
                assert_eq!(a.samples.len(), b.samples.len());
                for (a, b) in a.samples.iter().zip(&b.samples) {
                    assert_eq!(
                        [
                            a.time.to_bits(),
                            a.point.lat.to_bits(),
                            a.point.lon.to_bits(),
                            a.point.alt.to_bits(),
                            a.xyz.0.to_bits(),
                            a.xyz.1.to_bits(),
                            a.xyz.2.to_bits()
                        ],
                        [
                            b.time.to_bits(),
                            b.point.lat.to_bits(),
                            b.point.lon.to_bits(),
                            b.point.alt.to_bits(),
                            b.xyz.0.to_bits(),
                            b.xyz.1.to_bits(),
                            b.xyz.2.to_bits()
                        ]
                    );
                }
            }
        };
        compare(
            &cold,
            &super::super::load_for_job(&job, 1200., &control).unwrap(),
        );
        let mut warm_smoothed = job.clone();
        warm_smoothed.smooth_tracks = true;
        let mut uncached_smoothed = warm_smoothed.clone();
        uncached_smoothed.archive = None;
        compare(
            &super::super::load_for_job(&warm_smoothed, 600., &control).unwrap(),
            &super::super::load_for_job(&uncached_smoothed, 600., &control).unwrap(),
        );
        let mut tampered: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
        tampered["payload"]["tracks"][0]["samples"][0][3] = serde_json::json!(999_f64.to_bits());
        std::fs::write(&cache, serde_json::to_vec(&tampered).unwrap()).unwrap();
        compare(
            &cold,
            &super::super::load_for_job(&job, 600., &control).unwrap(),
        );
        std::fs::write(&cache, b"broken prepared cache").unwrap();
        compare(
            &cold,
            &super::super::load_for_job(&job, 600., &control).unwrap(),
        );
        assert!(
            std::fs::read_dir(cache.parent().unwrap())
                .unwrap()
                .any(|p| p
                    .unwrap()
                    .path()
                    .extension()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("rejected-"))
        );
        std::fs::write(&source, csv.replace("123.456789123", "124.456789123")).unwrap();
        let changed = super::super::load_for_job(&job, 600., &control).unwrap();
        assert_ne!(
            cold.tracks[0].samples[0].point.alt.to_bits(),
            changed.tracks[0].samples[0].point.alt.to_bits()
        );
        let mut other_region = job.clone();
        other_region.bounds = Some(crate::geo::Bounds {
            south: 40.,
            north: 41.,
            west: -88.,
            east: -87.,
        });
        assert!(super::super::load_for_job(&other_region, 600., &control).is_err());
        let mut known_clock = job.clone();
        known_clock.start_utc = Some(1700100000.);
        assert!(super::super::load_for_job(&known_clock, 600., &control).is_err());
        job.analysis_method = AnalysisMethod::Doppler;
        assert!(super::super::load_for_job(&job, 600., &control).is_err());
        std::fs::remove_file(source).unwrap();
        job.analysis_method = AnalysisMethod::PassageTiming;
        assert!(super::super::load_for_job(&job, 600., &control).is_err());
    }
}
