use soundgeo::{Control, data};
use std::{io::Write, path::Path};

fn trace(id: &str) -> Vec<u8> {
    let rows: Vec<_> = (0..24)
        .map(|i| serde_json::json!([i * 2, 35., -110. + i as f64 * 0.0005, 4000, 100, 90, 8, 0]))
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "icao": id, "timestamp": 1788861600., "trace": rows
    }))
    .unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap()
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for &(name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, bytes).unwrap();
    }
    builder.into_inner().unwrap()
}

fn verify_complete_trace(path: &Path) {
    let dataset = data::load(path, &Control::default()).unwrap();
    assert_eq!(dataset.tracks.len(), 1);
    assert_eq!(dataset.tracks[0].id, "abc123/0");
    assert_eq!(dataset.tracks[0].samples.len(), 24);
    for (i, sample) in dataset.tracks[0].samples.iter().enumerate() {
        assert_eq!(sample.time, 1788861600. + i as f64 * 2.);
        assert_eq!(sample.point.lon, -110. + i as f64 * 0.0005);
    }
    assert!(
        dataset.warnings.iter().any(|warning| {
            warning == "Unfinished readsb trace files skipped: 2 (.readsb_tmp)."
        })
    );
}

#[test]
fn archived_readsb_temporary_files_are_reported_without_loading_their_positions() {
    let completed = gzip(&trace("abc123"));
    let temporary = gzip(&trace("def456"));
    let bytes = archive(&[
        ("traces/23/trace_full_abc123.json", &completed),
        // The real staging archive contained this empty temporary member.
        ("traces/5f/trace_full_407e5f.json.readsb_tmp", &[]),
        ("traces/56/trace_full_def456.json.readsb_tmp", &temporary),
    ]);
    for split in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(if split { "day.tar.aa" } else { "day.tar" });
        if split {
            let cut = bytes.len() / 2 + 13;
            std::fs::write(&path, &bytes[..cut]).unwrap();
            std::fs::write(dir.path().join("day.tar.ab"), &bytes[cut..]).unwrap();
        } else {
            std::fs::write(&path, &bytes).unwrap();
        }
        verify_complete_trace(&path);
        assert!(!dir.path().join("traces").exists());
        let mut unchanged = std::fs::read(&path).unwrap();
        if split {
            unchanged.extend(std::fs::read(dir.path().join("day.tar.ab")).unwrap());
        }
        assert_eq!(unchanged, bytes);
    }
}

#[test]
fn loose_readsb_temporary_files_are_reported_and_preserved() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("trace_full_abc123.json"), trace("abc123")).unwrap();
    let empty = dir.path().join("trace_full_407e5f.json.readsb_tmp");
    let temporary = dir.path().join("trace_full_def456.json.readsb_tmp");
    std::fs::write(&empty, []).unwrap();
    let bytes = trace("def456");
    std::fs::write(&temporary, &bytes).unwrap();
    verify_complete_trace(dir.path());
    assert_eq!(std::fs::metadata(&empty).unwrap().len(), 0);
    assert_eq!(std::fs::read(&temporary).unwrap(), bytes);
}

#[test]
fn damaged_completed_traces_still_fail_with_the_archive_and_member_names() {
    let completed = gzip(&trace("abc123"));
    let truncated = &completed[..completed.len() - 4];
    for name in [
        "traces/23/trace_full_abc123.json",
        "traces/23/trace_full_abc123.json.gz",
        "traces/23/trace_full_abc123.json.readsb_tmp.gz",
    ] {
        for broken in [&[][..], truncated] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("damaged.tar");
            let bytes = archive(&[(name, broken)]);
            std::fs::write(&path, &bytes).unwrap();
            let error = data::load(&path, &Control::default()).err().unwrap();
            let message = format!("{error:#}");
            assert!(message.contains("Could not decode trace"), "{message}");
            assert!(message.contains(name), "{message}");
            assert!(message.contains("damaged.tar"), "{message}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace_full_abc123.json.gz");
    std::fs::write(&path, truncated).unwrap();
    let error = data::load(&path, &Control::default()).err().unwrap();
    assert!(format!("{error:#}").contains("trace_full_abc123.json.gz"));
}
