//! Recover missing daily traces from the provider's free source snapshots.
use super::*;
use std::collections::BTreeMap;

pub(super) type Plan = BTreeMap<String, BTreeSet<u32>>;

#[derive(Serialize, Deserialize)]
pub(super) struct Recovered {
    pub aircraft: String,
    pub interval_s: u32,
    pub slots: Vec<u32>,
    pub snapshots: Vec<Object>,
    pub missing_slots: Vec<u32>,
    pub trace: Option<Object>,
    pub positions: usize,
}

fn name(second: u32) -> String {
    format!(
        "{:02}{:02}{:02}Z.json.gz",
        second / 3600,
        second / 60 % 60,
        second % 60
    )
}

fn position(value: &Value, now: f64, start: f64) -> Option<(i64, crate::geo::Point)> {
    let age = value["seen_pos"].as_f64()?;
    if !(0. ..=SNAPSHOT_INTERVAL_S).contains(&age) {
        return None;
    }
    let time = ((now - age) * 1000.).round() as i64;
    let point = crate::geo::Point {
        lat: value["lat"].as_f64()?,
        lon: value["lon"].as_f64()?,
        alt: 0.,
    };
    (now.is_finite()
        && (start..start + TRACE_DAY_S).contains(&(time as f64 / 1000.))
        && point.valid())
    .then_some((time, point))
}

pub(super) fn discover(plan: &mut Plan, value: &Value, start: f64, bounds: Bounds) {
    let Some(now) = value["now"].as_f64() else {
        return;
    };
    let Some(aircraft) = value["aircraft"].as_array() else {
        return;
    };
    for aircraft in aircraft {
        if aircraft["alt_baro"].as_str() == Some("ground")
            || aircraft["alt_geom"]
                .as_f64()
                .or_else(|| aircraft["alt_baro"].as_f64())
                .is_none()
        {
            continue;
        }
        let Some(id) = aircraft["hex"].as_str().and_then(ident) else {
            continue;
        };
        let Some((time, point)) = position(aircraft, now, start) else {
            continue;
        };
        if !bounds.contains_flight(point) {
            continue;
        }
        // Keep discovery times compact. Expand only identities whose daily
        // trace is unavailable after the normal trace download.
        plan.entry(id)
            .or_default()
            .insert((time - (start * 1000.) as i64) as u32);
    }
}

fn cadence(date: &str) -> u32 {
    if date < "2020-05-01" { 60 } else { 5 }
}

fn slots(times: &BTreeSet<u32>, interval: u32) -> BTreeSet<u32> {
    times
        .iter()
        .flat_map(|&time| {
            let second = f64::from(time) / 1000.;
            let step = f64::from(interval);
            let first = ((second - SNAPSHOT_INTERVAL_S).max(0.) / step).floor() as u32 * interval;
            let last = ((second + SNAPSHOT_INTERVAL_S).min(f64::from(86400 - interval)) / step)
                .ceil() as u32
                * interval;
            (first..=last.min(86400 - interval)).step_by(interval as usize)
        })
        .collect()
}

fn collect(
    points: &mut BTreeMap<i64, Value>,
    aircraft: &Value,
    now: f64,
    start: f64,
) -> Result<()> {
    let Some((time, point)) = position(aircraft, now, start) else {
        return Ok(());
    };
    if let Some(old) = points.get(&time) {
        ensure!(
            old["lat"].as_f64() == Some(point.lat) && old["lon"].as_f64() == Some(point.lon),
            "Conflicting snapshot positions at the same aircraft observation time"
        );
    } else {
        points.insert(time, aircraft.clone());
    }
    Ok(())
}

fn derived(id: &str, start: f64, points: &BTreeMap<i64, Value>) -> Value {
    let mut previous = None;
    let rows: Vec<_> = points
        .iter()
        .map(|(&time, aircraft)| {
            let gap = previous.is_some_and(|old| time - old > 20000)
                || aircraft["alt_baro"].as_str() == Some("ground");
            previous = Some(time);
            serde_json::json!([
                time as f64 / 1000. - start,
                aircraft["lat"],
                aircraft["lon"],
                aircraft["alt_baro"],
                aircraft["gs"],
                aircraft["track"],
                u8::from(gap),
                aircraft["baro_rate"],
                aircraft,
                aircraft["type"],
                aircraft["alt_geom"],
                aircraft["geom_rate"]
            ])
        })
        .collect();
    serde_json::json!({"icao": id, "timestamp": start, "trace": rows,
        "soundgeo_source": "Derived from free source snapshots. Source objects and gaps are recorded in sample-manifest.json."})
}

pub(super) fn cache_valid(
    manifest: &Manifest,
    traces: &Path,
    shared: &Path,
    control: &Control,
) -> bool {
    let directory = traces.join("snapshot-recovery");
    let Ok(day) = NaiveDate::parse_from_str(&manifest.date, "%Y-%m-%d") else {
        return false;
    };
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64;
    let mut files = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for recovery in &manifest.snapshot_recoveries {
        if manifest.schema < 5
            || !ids.insert(&recovery.aircraft)
            || !manifest.missing_aircraft.contains(&recovery.aircraft)
            || ident(&recovery.aircraft).as_deref() != Some(&recovery.aircraft)
            || recovery.interval_s != cadence(&manifest.date)
            || recovery.slots.is_empty()
            || recovery
                .slots
                .iter()
                .any(|v| *v >= 86400 || v % recovery.interval_s != 0)
            || recovery.slots.windows(2).any(|p| p[0] >= p[1])
        {
            return false;
        }
        let expected: BTreeSet<_> = recovery.slots.iter().map(|&slot| name(slot)).collect();
        let mut actual = BTreeSet::new();
        for object in &recovery.snapshots {
            if !expected.contains(&object.file)
                || !actual.insert(object.file.clone())
                || !valid_object(&shared.join(&object.file), object, control)
            {
                return false;
            }
        }
        for &slot in &recovery.missing_slots {
            if !recovery.slots.contains(&slot) || !actual.insert(name(slot)) {
                return false;
            }
        }
        if actual != expected {
            return false;
        }
        if let Some(object) = &recovery.trace {
            let expected = format!("trace_full_{}.json.gz", recovery.aircraft);
            if object.file != expected
                || recovery.positions == 0
                || !files.insert(expected)
                || !valid_object(&directory.join(&object.file), object, control)
                || !std::fs::read(directory.join(&object.file)).is_ok_and(|bytes| {
                    parse(&bytes).is_ok_and(|value| {
                        trace(&value, start, &recovery.aircraft).is_ok()
                            && value["trace"]
                                .as_array()
                                .is_some_and(|rows| rows.len() == recovery.positions)
                    })
                })
            {
                return false;
            }
        } else if recovery.positions != 0 {
            return false;
        }
    }
    match std::fs::read_dir(&directory) {
        Ok(entries) => {
            let actual: Option<BTreeSet<_>> = entries
                .map(|entry| {
                    let entry = entry.ok()?;
                    entry
                        .file_type()
                        .ok()?
                        .is_file()
                        .then(|| entry.file_name().to_string_lossy().into_owned())
                })
                .collect();
            actual.as_ref() == Some(&files)
        }
        Err(error) => error.kind() == std::io::ErrorKind::NotFound && files.is_empty(),
    }
}

pub(super) async fn upgrade_legacy(
    client: &Client,
    manifest: &mut Manifest,
    previous: &[u8],
    start: f64,
    root: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<()> {
    use chrono::Timelike;

    if control.offline || manifest.schema >= 5 || manifest.missing_aircraft.is_empty() {
        return Ok(());
    }
    control.check()?;
    let backup = root
        .join("rejected")
        .join(format!("sample-manifest-{}.json", hash(previous)));
    if backup.exists() {
        ensure!(
            std::fs::read(&backup)?.as_slice() == previous,
            "Saved legacy manifest has changed"
        );
    } else {
        atomic_write(&backup, previous)?;
    }
    let shared = root
        .parent()
        .context("Recovery cache has no parent")?
        .join(format!("adsbx-sample-{}-snapshots", manifest.date));
    let mut plan = Plan::new();
    for (index, object) in manifest.snapshots.iter().enumerate() {
        control.check()?;
        let bytes = std::fs::read(shared.join(&object.file))?;
        ensure!(
            hash(&bytes) == object.sha256,
            "Discovery snapshot changed during legacy recovery"
        );
        let time = chrono::NaiveTime::parse_from_str(&object.file, "%H%M%SZ.json.gz")?;
        let value = parse(&bytes)?;
        snapshot(
            &value,
            start + f64::from(time.num_seconds_from_midnight()),
            manifest.bounds,
        )?;
        discover(&mut plan, &value, start, manifest.bounds);
        if index % 24 == 0 || index + 1 == manifest.snapshots.len() {
            progress(
                &format!(
                    "Checking older cache for missing flight positions: {}/{} snapshots",
                    index + 1,
                    manifest.snapshots.len()
                ),
                0.85 * (index + 1) as f32 / manifest.snapshots.len() as f32,
            );
        }
    }
    recover(client, manifest, plan, start, root, control, progress).await?;
    control.check()?;
    let manifest_path = root.join("sample-manifest.json");
    ensure!(
        std::fs::read(&manifest_path)?.as_slice() == previous,
        "Sample manifest changed during legacy recovery"
    );
    manifest.schema = 5;
    atomic_write(&manifest_path, &serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

pub(super) async fn recover(
    client: &Client,
    manifest: &mut Manifest,
    plan: Plan,
    start: f64,
    root: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<()> {
    let shared = root
        .parent()
        .context("Recovery cache has no parent")?
        .join(format!("adsbx-sample-{}-snapshots", manifest.date));
    let plan: Plan = plan
        .into_iter()
        .filter(|(id, _)| manifest.missing_aircraft.contains(id))
        .collect();
    let mut requests: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    let mut points = Vec::new();
    let interval = cadence(&manifest.date);
    for (id, times) in plan {
        let slots = slots(&times, interval);
        let index = manifest.snapshot_recoveries.len();
        for &slot in &slots {
            requests.entry(slot).or_default().push(index);
        }
        manifest.snapshot_recoveries.push(Recovered {
            aircraft: id,
            interval_s: interval,
            slots: slots.into_iter().collect(),
            snapshots: vec![],
            missing_slots: vec![],
            trace: None,
            positions: 0,
        });
        points.push(BTreeMap::new());
    }
    let total = requests.len();
    let prefix = manifest.date.replace('-', "/");
    for (ordinal, (slot, targets)) in requests.into_iter().enumerate() {
        control.check()?;
        let filename = name(slot);
        let result = object(
            client,
            &format!("readsb-hist/{prefix}/{filename}"),
            &shared.join(&filename),
            &shared.join("rejected"),
            control,
            |value| snapshot(value, start + slot as f64, manifest.bounds).map(|_| ()),
        )
        .await?;
        if let Some((object, value)) = result {
            let now = value["now"]
                .as_f64()
                .context("Missing recovery snapshot time")?;
            let aircraft = value["aircraft"]
                .as_array()
                .context("Missing recovery aircraft")?;
            let wanted: BTreeMap<_, _> = targets
                .iter()
                .map(|&index| (manifest.snapshot_recoveries[index].aircraft.as_str(), index))
                .collect();
            for aircraft in aircraft {
                let Some(id) = aircraft["hex"].as_str().and_then(ident) else {
                    continue;
                };
                if let Some(&index) = wanted.get(id.as_str()) {
                    collect(&mut points[index], aircraft, now, start)?;
                }
            }
            for index in targets {
                manifest.snapshot_recoveries[index]
                    .snapshots
                    .push(object.clone());
            }
        } else {
            for index in targets {
                manifest.snapshot_recoveries[index].missing_slots.push(slot);
            }
        }
        progress(
            &format!(
                "Free snapshot recovery: {}/{} files checked for {} missing traces",
                ordinal + 1,
                total,
                manifest.snapshot_recoveries.len()
            ),
            0.9 + 0.09 * (ordinal + 1) as f32 / total as f32,
        );
    }
    let stage = tempfile::tempdir_in(root)?;
    for (recovery, points) in manifest.snapshot_recoveries.iter_mut().zip(points) {
        control.check()?;
        if points.is_empty() {
            continue;
        }
        let value = derived(&recovery.aircraft, start, &points);
        trace(&value, start, &recovery.aircraft)?;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        serde_json::to_writer(&mut encoder, &value)?;
        let bytes = encoder.finish()?;
        ensure!(
            bytes.len() <= MAX_COMPRESSED,
            "Recovered trace exceeds the cache size limit"
        );
        let file = format!("trace_full_{}.json.gz", recovery.aircraft);
        atomic_write(&stage.path().join(&file), &bytes)?;
        recovery.positions = points.len();
        recovery.trace = Some(Object {
            file,
            sha256: hash(&bytes),
            bytes: bytes.len(),
        });
    }
    control.check()?;
    let destination = root.join("traces/snapshot-recovery");
    if destination.exists() {
        let rejected = root.join("rejected");
        std::fs::create_dir_all(&rejected)?;
        let archive = tempfile::Builder::new()
            .prefix("snapshot-recovery-")
            .tempdir_in(&rejected)?;
        std::fs::rename(&destination, archive.path().join("files"))?;
        let _preserved = archive.keep();
    }
    if !manifest.snapshot_recoveries.is_empty() {
        std::fs::rename(stage.path(), &destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bounds() -> Bounds {
        Bounds {
            south: 10.,
            north: 11.,
            west: 20.,
            east: 21.,
        }
    }
    fn aircraft() -> Value {
        json!({"hex":"~ABC123","lat":10.5,"lon":20.5,"alt_baro":900,"alt_geom":1000,"seen_pos":1.})
    }

    #[test]
    fn recovery_discovery_uses_airborne_position_times_and_clips_day_edges() {
        let start = 1709251200.;
        let mut plan = Plan::new();
        discover(
            &mut plan,
            &json!({"now":start+301.,"aircraft":[aircraft()]}),
            start,
            bounds(),
        );
        assert_eq!(plan["~abc123"].len(), 1);
        assert_eq!(slots(&plan["~abc123"], 5).first(), Some(&0));
        assert_eq!(slots(&plan["~abc123"], 5).last(), Some(&600));
        let saved = plan.clone();
        for (key, value) in [
            ("alt_baro", json!("ground")),
            ("seen_pos", json!(-1.)),
            ("seen_pos", json!(301.)),
            ("seen_pos", Value::Null),
            ("lat", json!(20.)),
            ("hex", json!("../bad")),
        ] {
            let mut bad = aircraft();
            bad[key] = value;
            discover(
                &mut plan,
                &json!({"now":start+1000.,"aircraft":[bad]}),
                start,
                bounds(),
            );
            assert_eq!(plan, saved);
        }
        let mut empty = Plan::new();
        discover(
            &mut empty,
            &json!({"now":start,"aircraft":[aircraft()]}),
            start,
            bounds(),
        );
        assert!(empty.is_empty());
        discover(
            &mut plan,
            &json!({"now":start+86400.,"aircraft":[aircraft()]}),
            start,
            bounds(),
        );
        assert_eq!(slots(&plan["~abc123"], 5).last(), Some(&86395));
        assert!(slots(&plan["~abc123"], 5).iter().all(|slot| slot % 5 == 0));
    }

    #[test]
    fn older_free_snapshots_use_the_published_one_minute_cadence() {
        assert_eq!(cadence("2020-04-01"), 60);
        assert_eq!(cadence("2020-05-01"), 5);
        let times = BTreeSet::from([300_001, 86_399_000]);
        let planned = slots(&times, cadence("2019-07-01"));
        assert_eq!(planned.first(), Some(&0));
        assert_eq!(planned.last(), Some(&86340));
        assert!(planned.iter().all(|second| second % 60 == 0));
        assert!(planned.contains(&660));
    }

    #[test]
    fn recovered_positions_preserve_times_altitudes_ground_and_reception_gaps() {
        let start = 1709251200.;
        let mut points = BTreeMap::new();
        let point = aircraft();
        collect(&mut points, &point, start + 100., start).unwrap();
        let mut stale = point.clone();
        stale["seen_pos"] = json!(6.);
        collect(&mut points, &stale, start + 105., start).unwrap();
        assert_eq!(points.len(), 1);
        let mut conflict = stale.clone();
        conflict["lat"] = json!(10.6);
        assert!(collect(&mut points, &conflict, start + 105., start).is_err());
        collect(&mut points, &point, start + 125., start).unwrap();
        let mut ground = point.clone();
        ground["alt_baro"] = json!("ground");
        collect(&mut points, &ground, start + 130., start).unwrap();
        let value = derived("~abc123", start, &points);
        let rows = value["trace"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][0], 99.);
        assert_eq!(rows[0][3], 900);
        assert_eq!(rows[0][10], 1000);
        assert_eq!(rows[0][6], 0);
        assert_eq!(rows[1][6], 1);
        assert_eq!(rows[2][3], "ground");
        assert_eq!(rows[2][6], 1);
        assert!(trace(&value, start, "~abc123").is_ok());
    }
}
