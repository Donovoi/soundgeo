use serde_json::json;
use sha2::{Digest, Sha256};
use soundgeo::{Control, archive::ArchiveRequest};

fn cached_day(root: &std::path::Path, date: &str) {
    let folder = root.join(date);
    std::fs::create_dir_all(&folder).unwrap();
    let tag = format!("v{}-planes-readsb-prod-0", date.replace('-', "."));
    let name = format!("{tag}.tar.aa");
    let bytes = b"offline cache verification fixture";
    std::fs::write(folder.join(&name), bytes).unwrap();
    let manifest = json!({"tag_name":tag,"assets":[{"name":name,"size":bytes.len(),
        "digest":format!("sha256:{:x}",Sha256::digest(bytes)),
        "browser_download_url":"https://github.com/adsblol/globe_history_2024/releases/download/fixture/nonexistent"}]});
    std::fs::write(
        folder.join("archive-manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(folder.join("aircraft-types.json"), "{}").unwrap();
}

#[test]
fn selected_date_range_reuses_verified_cache_without_network() {
    let dir = tempfile::tempdir().unwrap();
    for date in ["2024-03-01", "2024-03-02", "2024-03-03"] {
        cached_day(dir.path(), date);
    }
    let request = ArchiveRequest {
        date_from: "2024-03-01".into(),
        date_to: Some("2024-03-02".into()),
        cache: dir.path().into(),
    };
    let sources = request.ensure(&Control::default(), &|_, _| {}).unwrap();
    assert_eq!(sources.len(), 2);
    assert!(
        sources
            .iter()
            .all(|p| !p.to_string_lossy().contains("2024-03-03"))
    );
    let control = Control::default();
    control.stop();
    assert!(
        request
            .ensure(&control, &|_, _| {})
            .unwrap_err()
            .to_string()
            .contains("Cancelled")
    );
}

#[test]
fn invalid_ranges_and_wrong_date_cache_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut request = ArchiveRequest {
        date_from: "2024-03-01".into(),
        date_to: None,
        cache: dir.path().into(),
    };
    assert_eq!(request.dates().unwrap(), ["2024-03-01"]);
    request.date_to = Some("2024-02-28".into());
    assert!(request.dates().is_err());
    request.date_to = Some("2024-04-02".into());
    assert!(request.dates().is_err());
    request.date_to = None;
    request.date_from = "2099-01-01".into();
    assert!(request.dates().is_err());
    request.date_from = "2024-03-01".into();
    cached_day(dir.path(), "2024-03-01");
    let manifest = dir.path().join("2024-03-01/archive-manifest.json");
    std::fs::write(
        manifest,
        r#"{"tag_name":"v2024.02.29-planes-readsb-prod-0"}"#,
    )
    .unwrap();
    assert!(request.ensure(&Control::default(), &|_, _| {}).is_err());
}
