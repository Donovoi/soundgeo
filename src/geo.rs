use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElevationDatum {
    /// Backward-compatible interpretation of a manually supplied height.
    #[default]
    Wgs84Ellipsoid,
    Egm96,
}

/// NGA/OSGeo public-domain EGM96 15-minute grid. Bilinear interpolation,
/// including the periodic longitude seam. Returns N in h = H + N.
pub fn geoid_undulation(lat: f64, lon: f64) -> f64 {
    const GRID: &[u8] = include_bytes!("../assets/egm96_15.gtx");
    let y = ((lat.clamp(-90., 90.) + 90.) * 4.).min(720.);
    let x = (lon + 180.).rem_euclid(360.) * 4.;
    let row = (y.floor() as usize).min(719);
    let col = x.floor() as usize % 1440;
    let fy = y - row as f64;
    let fx = x - x.floor();
    let at = |r: usize, c: usize| {
        let offset = 40 + 4 * (r * 1440 + c % 1440);
        f32::from_be_bytes(GRID[offset..offset + 4].try_into().unwrap()) as f64
    };
    (1. - fy) * ((1. - fx) * at(row, col) + fx * at(row, col + 1))
        + fy * ((1. - fx) * at(row + 1, col) + fx * at(row + 1, col + 1))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Vec3(pub f64, pub f64, pub f64);
impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, v: Self) -> Self {
        Self(self.0 - v.0, self.1 - v.1, self.2 - v.2)
    }
}
impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, v: Self) -> Self {
        Self(self.0 + v.0, self.1 + v.1, self.2 + v.2)
    }
}
impl std::ops::Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, x: f64) -> Self {
        Self(self.0 * x, self.1 * x, self.2 * x)
    }
}
impl Vec3 {
    pub fn dot(self, v: Self) -> f64 {
        self.0 * v.0 + self.1 * v.1 + self.2 * v.2
    }
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }
}
impl Point {
    /// WGS84 ECEF. Altitudes must use a consistent vertical datum; see README.
    pub fn ecef(self) -> Vec3 {
        let (lat, lon) = (self.lat.to_radians(), self.lon.to_radians());
        let e2 = 6.694_379_990_14e-3;
        let n = 6_378_137. / (1. - e2 * lat.sin().powi(2)).sqrt();
        Vec3(
            (n + self.alt) * lat.cos() * lon.cos(),
            (n + self.alt) * lat.cos() * lon.sin(),
            (n * (1. - e2) + self.alt) * lat.sin(),
        )
    }
    pub fn distance(self, other: Self) -> f64 {
        (self.ecef() - other.ecef()).norm()
    }
    pub fn valid(self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && self.alt.is_finite()
            && (-90. ..=90.).contains(&self.lat)
            && (-180. ..=180.).contains(&self.lon)
            && (-1000. ..=30000.).contains(&self.alt)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bounds {
    pub south: f64,
    pub north: f64,
    pub west: f64,
    pub east: f64,
}
impl Bounds {
    pub fn validate(self) -> Result<()> {
        if ![self.south, self.north, self.west, self.east]
            .iter()
            .all(|v| v.is_finite())
            || self.south >= self.north
            || self.west >= self.east
            || self.south < -85.
            || self.north > 85.
            || self.west < -180.
            || self.east > 180.
        {
            bail!(
                "Use ordered bounds within ±85° latitude and ±180° longitude; split regions crossing the date line."
            );
        }
        Ok(())
    }
    pub fn contains(self, p: Point) -> bool {
        p.lat >= self.south && p.lat <= self.north && p.lon >= self.west && p.lon <= self.east
    }
    /// Reference positions can be outside the observer's region: audible slant
    /// range can be large. Discovery and import must keep the same flight area.
    pub(crate) fn contains_flight(self, p: Point) -> bool {
        let lon_margin = 0.6 / p.lat.to_radians().cos().abs().max(0.05);
        p.lat >= self.south - 0.6
            && p.lat <= self.north + 0.6
            && p.lon >= self.west - lon_margin
            && p.lon <= self.east + lon_margin
    }
    pub fn grid(self, requested: f64, limit: usize) -> (Vec<Point>, f64) {
        let lat_m = 111_320.;
        let lon_m = lat_m
            * ((self.south + self.north) * 0.5)
                .to_radians()
                .cos()
                .max(0.05);
        let height = (self.north - self.south) * lat_m;
        let width = (self.east - self.west) * lon_m;
        let mut spacing = requested.max((width * height / limit as f64).sqrt());
        loop {
            let ny = (height / spacing).ceil() as usize + 1;
            let nx = (width / spacing).ceil() as usize + 1;
            if nx.saturating_mul(ny) <= limit {
                let points = (0..ny)
                    .flat_map(|y| {
                        (0..nx).map(move |x| Point {
                            lat: self.south
                                + (self.north - self.south) * y as f64 / (ny - 1).max(1) as f64,
                            lon: self.west
                                + (self.east - self.west) * x as f64 / (nx - 1).max(1) as f64,
                            alt: 0.,
                        })
                    })
                    .collect();
                return (points, spacing);
            }
            spacing *= 1.05;
        }
    }
}

pub struct Terrain {
    root: Option<PathBuf>,
    fallback: f64,
    datum: ElevationDatum,
    tiles: HashMap<String, Option<Vec<i16>>>,
    shared: Option<Arc<SharedTiles>>,
    guard: Option<Arc<TerrainSources>>,
    pub missing: usize,
}
impl Terrain {
    pub(crate) fn supports_parallel_queries(&self) -> bool {
        self.root.is_none() || self.shared.is_some()
    }
    pub(crate) fn empty_cache_clone(&self) -> Self {
        let mut result = Self::with_datum(self.root.clone(), self.fallback, self.datum);
        result.shared = self.shared.clone();
        result.guard = self.guard.clone();
        result
    }
    pub(crate) fn with_search_scope<T>(
        &mut self,
        control: &crate::Control,
        search: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.verify_shared(control)?;
        let previous = self.guard.clone();
        let result = search(self);
        let verified = self.verify_shared(control);
        self.guard = previous;
        verified?;
        result
    }
    pub(crate) fn retain_source_guard(&mut self, view: &Self) -> Result<()> {
        if let Some(guard) = &view.guard {
            if let Some(previous) = &self.guard {
                anyhow::ensure!(
                    previous == guard,
                    "Terrain source guard changed during search"
                );
            } else {
                self.guard = Some(guard.clone());
            }
        }
        Ok(())
    }
    /// A phase-local, immutable view. Unsupported layouts or memory requirements
    /// retain the original serial loader; a detected change is always an error.
    pub(crate) fn parallel_view(&self, control: &crate::Control) -> Result<Option<Self>> {
        self.parallel_view_with_limit(control, 128 * 1024 * 1024)
    }
    fn parallel_view_with_limit(
        &self,
        control: &crate::Control,
        limit: usize,
    ) -> Result<Option<Self>> {
        self.verify_shared(control)?;
        let Some(root) = &self.root else {
            return Ok(Some(self.empty_cache_clone()));
        };
        if self.shared.is_some() {
            return Ok(Some(self.empty_cache_clone()));
        }
        let Some(sources) = tile_sources(root, control)? else {
            return Ok(None);
        };
        let live_bytes: usize = self
            .tiles
            .values()
            .flatten()
            .map(|v| v.capacity().saturating_mul(2))
            .fold(0, usize::saturating_add);
        let decoded_bytes: usize = sources
            .iter()
            .filter(|s| valid_tile_len(s.len))
            .map(|s| s.len as usize)
            .fold(0, usize::saturating_add);
        let raw_bytes = sources
            .iter()
            .filter(|s| valid_tile_len(s.len))
            .map(|s| s.len as usize)
            .max()
            .unwrap_or(0);
        // Include the caller's still-live cache, simultaneous raw/decoded data,
        // verification buffer and bounded inventory/map overhead.
        let overhead = 128 * 1024 + sources.len() * 1024;
        if live_bytes
            .saturating_add(decoded_bytes)
            .saturating_add(raw_bytes)
            .saturating_add(overhead)
            > limit
        {
            return Ok(None);
        }
        let mut tiles = HashMap::new();
        let mut hashes = Vec::new();
        let mut allocated = live_bytes + overhead;
        for source in &sources {
            control.check()?;
            if !valid_tile_len(source.len) {
                hashes.push(None);
                continue;
            }
            let raw = read_tile(root, source, control)?;
            let hash: [u8; 32] = Sha256::digest(&raw).into();
            let mut data = Vec::with_capacity(raw.len() / 2);
            for chunk in raw.chunks(64 * 1024) {
                control.check()?;
                data.extend(
                    chunk
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| i16::from_be_bytes(*b)),
                );
            }
            allocated += data.capacity() * 2;
            anyhow::ensure!(
                allocated.saturating_add(raw.capacity()) <= limit,
                "Terrain snapshot allocation exceeded its memory bound"
            );
            tiles.insert(source.canonical.clone(), data);
            hashes.push(Some(hash));
        }
        // Cached misses and cached heights are authoritative too. Never silently
        // replace a stale caller cache with newly read terrain.
        for (name, cached) in &self.tiles {
            anyhow::ensure!(
                cached.as_ref() == tiles.get(name),
                "Terrain changed since its cached sample was loaded: {name}"
            );
        }
        let mut view = self.empty_cache_clone();
        let guard = Arc::new(TerrainSources { sources, hashes });
        if let Some(previous) = &self.guard {
            anyhow::ensure!(
                previous == &guard,
                "Terrain source guard changed while preparing search"
            );
        }
        view.guard = Some(guard);
        view.shared = Some(Arc::new(SharedTiles { tiles }));
        view.verify_shared(control)?;
        Ok(Some(view))
    }
    pub(crate) fn verify_shared(&self, control: &crate::Control) -> Result<()> {
        control.check()?;
        if let Some(shared) = &self.guard {
            let root = self.root.as_ref().expect("shared terrain has a root");
            anyhow::ensure!(
                tile_sources(root, control)?.as_ref() == Some(&shared.sources),
                "Terrain tile inventory changed during search"
            );
            for (source, expected) in shared.sources.iter().zip(&shared.hashes) {
                control.check()?;
                if let Some(expected) = expected {
                    let mut file = std::fs::File::open(root.join(&source.canonical))?;
                    let mut hash = Sha256::new();
                    let mut buffer = [0; 64 * 1024];
                    let mut remaining = source.len;
                    while remaining > 0 {
                        control.check()?;
                        let length = remaining.min(buffer.len() as u64) as usize;
                        file.read_exact(&mut buffer[..length])?;
                        hash.update(&buffer[..length]);
                        remaining -= length as u64;
                    }
                    anyhow::ensure!(
                        file.read(&mut buffer[..1])? == 0
                            && <[u8; 32]>::from(hash.finalize()) == *expected,
                        "Terrain tile contents changed during search: {}",
                        source.canonical
                    );
                }
            }
            anyhow::ensure!(
                tile_sources(root, control)?.as_ref() == Some(&shared.sources),
                "Terrain tile inventory changed during verification"
            );
        }
        control.check()
    }
    pub fn new(root: Option<PathBuf>, fallback: f64) -> Self {
        Self::with_datum(root, fallback, ElevationDatum::Wgs84Ellipsoid)
    }
    pub fn with_datum(root: Option<PathBuf>, fallback: f64, datum: ElevationDatum) -> Self {
        Self {
            root,
            fallback,
            datum,
            tiles: HashMap::new(),
            shared: None,
            guard: None,
            missing: 0,
        }
    }
    pub fn elevation(&mut self, p: Point) -> f64 {
        let p = Point {
            lon: if p.lon == 180. { -180. } else { p.lon },
            ..p
        };
        let lat = p.lat.floor() as i32;
        let lon = p.lon.floor() as i32;
        let name = format!(
            "{}{:02}{}{:03}.hgt",
            if lat < 0 { "S" } else { "N" },
            lat.abs(),
            if lon < 0 { "W" } else { "E" },
            lon.abs()
        );
        if let Some(root) = &self.root {
            if let Some(shared) = &self.shared {
                if let Some(height) = shared
                    .tiles
                    .get(&name)
                    .and_then(|data| tile_height(data, p, lat, lon))
                {
                    return height;
                }
            } else {
                // Bound RAM for very broad searches. Missing tiles are cached too.
                if self.tiles.len() > 32 {
                    self.tiles.clear();
                }
                let tile = self.tiles.entry(name.clone()).or_insert_with(|| {
                    let path = root.join(&name);
                    let len = std::fs::metadata(&path).ok()?.len();
                    if len != 1201 * 1201 * 2 && len != 3601 * 3601 * 2 {
                        return None;
                    }
                    Some(
                        std::fs::read(path)
                            .ok()?
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|b| i16::from_be_bytes([b[0], b[1]]))
                            .collect(),
                    )
                });
                if let Some(data) = tile
                    && let Some(height) = tile_height(data, p, lat, lon)
                {
                    return height;
                }
            }
        }
        self.missing += 1;
        self.fallback
            + if self.datum == ElevationDatum::Egm96 {
                geoid_undulation(p.lat, p.lon)
            } else {
                0.
            }
    }
}

fn tile_height(data: &[i16], p: Point, lat: i32, lon: i32) -> Option<f64> {
    let n = (data.len() as f64).sqrt() as usize;
    let x = ((p.lon - lon as f64) * (n - 1) as f64).round() as usize;
    let y = ((1. - (p.lat - lat as f64)) * (n - 1) as f64).round() as usize;
    let h = data[y.min(n - 1) * n + x.min(n - 1)];
    // SRTM always references EGM96, irrespective of the fallback's datum.
    (h != i16::MIN).then(|| h as f64 + geoid_undulation(p.lat, p.lon))
}

struct SharedTiles {
    tiles: HashMap<String, Vec<i16>>,
}

#[derive(PartialEq, Eq)]
struct TerrainSources {
    sources: Vec<TileSource>,
    hashes: Vec<Option<[u8; 32]>>,
}

#[derive(Debug, PartialEq, Eq)]
struct TileSource {
    name: String,
    canonical: String,
    len: u64,
    modified: Option<SystemTime>,
}

fn valid_tile_len(len: u64) -> bool {
    len == 1201 * 1201 * 2 || len == 3601 * 3601 * 2
}

fn tile_sources(root: &Path, control: &crate::Control) -> Result<Option<Vec<TileSource>>> {
    control.check()?;
    if !std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return Ok(None);
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(None);
    };
    let mut sources = Vec::new();
    for entry in entries {
        control.check()?;
        let Ok(entry) = entry else {
            return Ok(None);
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Ok(None);
        };
        let upper = name.to_ascii_uppercase();
        let bytes = upper.as_bytes();
        if bytes.len() != 11
            || !matches!(bytes[0], b'N' | b'S')
            || !bytes[1..3].iter().all(u8::is_ascii_digit)
            || !matches!(bytes[3], b'E' | b'W')
            || !bytes[4..7].iter().all(u8::is_ascii_digit)
            || &bytes[7..] != b".HGT"
        {
            continue;
        }
        let canonical = format!("{}.hgt", &upper[..7]);
        // Resolve the exact path used by the legacy loader on this filesystem.
        // Case aliases are supported only when that canonical lookup resolves.
        let Ok(actual) = entry.file_type() else {
            return Ok(None);
        };
        let Ok(metadata) = std::fs::symlink_metadata(root.join(&canonical)) else {
            return Ok(None);
        };
        if !actual.is_file()
            || !metadata.is_file()
            || sources
                .iter()
                .any(|s: &TileSource| s.canonical == canonical)
            || sources.len() >= 256
        {
            return Ok(None);
        }
        if valid_tile_len(metadata.len()) && std::fs::File::open(root.join(&canonical)).is_err() {
            return Ok(None);
        }
        sources.push(TileSource {
            name: name.into(),
            canonical,
            len: metadata.len(),
            modified: metadata.modified().ok(),
        });
    }
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Some(sources))
}

fn read_tile(root: &Path, source: &TileSource, control: &crate::Control) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(root.join(&source.canonical))?;
    let mut bytes = vec![0; source.len as usize];
    for chunk in bytes.chunks_mut(64 * 1024) {
        control.check()?;
        file.read_exact(chunk)?;
    }
    let mut extra = [0];
    anyhow::ensure!(
        file.read(&mut extra)? == 0,
        "Terrain tile grew while preparing search: {}",
        source.canonical
    );
    Ok(bytes)
}

#[cfg(test)]
mod terrain_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn write_tile(path: &Path, n: usize) {
        let bytes: Vec<u8> = (0..n * n)
            .flat_map(|i| {
                let value = if i % 17 == 0 {
                    i16::MIN
                } else {
                    ((i / n + 3 * (i % n)) % 2000) as i16
                };
                value.to_be_bytes()
            })
            .collect();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn shared_tiles_preserve_legacy_sample_bits_datums_edges_and_misses() {
        let dir = tempfile::tempdir().unwrap();
        write_tile(&dir.path().join("N35W110.hgt"), 1201);
        write_tile(&dir.path().join("N36W110.hgt"), 3601);
        write_tile(&dir.path().join("N90W180.hgt"), 1201);
        std::fs::write(dir.path().join("N34W110.hgt"), [0; 7]).unwrap();
        let control = crate::Control::default();
        for datum in [ElevationDatum::Wgs84Ellipsoid, ElevationDatum::Egm96] {
            let mut legacy = Terrain::with_datum(Some(dir.path().into()), 42.25, datum);
            let mut shared = legacy.parallel_view(&control).unwrap().unwrap();
            for (lat, lon) in [
                (35., -110.),
                (35.25, -109.75),
                (35.9999999, -109.0000001),
                (36.4, -109.8),
                (34.2, -109.5),
                (32., -109.),
                (90., 180.),
                (90., -180.),
            ] {
                let point = Point { lat, lon, alt: 0. };
                assert_eq!(
                    legacy.elevation(point).to_bits(),
                    shared.elevation(point).to_bits(),
                    "{lat},{lon}/{datum:?}"
                );
            }
            // Test ties and the adjacent representable coordinates on both
            // axes; the legacy nearest-sample rounding must remain exact.
            for (south, n) in [(35_f64, 1201), (36_f64, 3601)] {
                let longitude = -110. + 600.5 / (n - 1) as f64;
                let latitude = south + 1. - 400.5 / (n - 1) as f64;
                for lat in [latitude.next_down(), latitude, latitude.next_up()] {
                    for lon in [longitude.next_down(), longitude, longitude.next_up()] {
                        let point = Point { lat, lon, alt: 0. };
                        assert_eq!(
                            legacy.elevation(point).to_bits(),
                            shared.elevation(point).to_bits()
                        );
                    }
                }
            }
            assert!(legacy.missing >= 3);
            assert_eq!(legacy.missing, shared.missing);
            assert!(shared.tiles.is_empty());
            let fork = shared.empty_cache_clone();
            assert!(Arc::ptr_eq(
                shared.shared.as_ref().unwrap(),
                fork.shared.as_ref().unwrap()
            ));
            assert_eq!(fork.missing, 0);
            shared.verify_shared(&control).unwrap();
        }
    }

    #[test]
    fn shared_tile_cap_counts_live_cache_and_raw_decode_overlap() {
        let dir = tempfile::tempdir().unwrap();
        write_tile(&dir.path().join("N35W110.hgt"), 1201);
        let mut terrain = Terrain::new(Some(dir.path().into()), 0.);
        let control = crate::Control::default();
        assert!(
            terrain
                .parallel_view_with_limit(&control, 5 * 1024 * 1024)
                .unwrap()
                .is_none()
        );
        assert!(
            terrain
                .parallel_view_with_limit(&control, 6 * 1024 * 1024)
                .unwrap()
                .is_some()
        );
        terrain.elevation(Point {
            lat: 35.2,
            lon: -109.5,
            alt: 0.,
        });
        assert!(
            terrain
                .parallel_view_with_limit(&control, 6 * 1024 * 1024)
                .unwrap()
                .is_none()
        );
        assert!(
            terrain
                .parallel_view_with_limit(&control, 9 * 1024 * 1024)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn shared_tiles_reject_stale_cached_heights_and_misses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("N35W110.hgt");
        let point = Point {
            lat: 35.2,
            lon: -109.5,
            alt: 0.,
        };
        let control = crate::Control::default();
        let mut missed = Terrain::new(Some(dir.path().into()), 0.);
        missed.elevation(point);
        write_tile(&path, 1201);
        assert!(missed.parallel_view(&control).is_err());
        let mut cached = Terrain::new(Some(dir.path().into()), 0.);
        cached.elevation(point);
        use std::io::{Seek, Write};
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(std::io::SeekFrom::Start(0)).unwrap();
        file.write_all(&42_i16.to_be_bytes()).unwrap();
        assert!(cached.parallel_view(&control).is_err());
    }

    #[test]
    fn shared_tiles_detect_changes_growth_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("N35W110.hgt");
        write_tile(&path, 1201);
        let control = crate::Control::default();
        let terrain = Terrain::new(Some(dir.path().into()), 0.);
        let view = terrain.parallel_view(&control).unwrap().unwrap();
        std::fs::write(dir.path().join("N36W110.hgt"), [1, 2]).unwrap();
        assert!(view.verify_shared(&control).is_err());
        std::fs::remove_file(dir.path().join("N36W110.hgt")).unwrap();
        let source = &view.guard.as_ref().unwrap().sources[0];
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[0])
            .unwrap();
        assert!(read_tile(dir.path(), source, &control).is_err());
        assert!(view.verify_shared(&control).is_err());
        control.cancel.store(true, Ordering::Relaxed);
        assert!(terrain.parallel_view(&control).is_err());
        assert!(view.verify_shared(&control).is_err());
    }

    #[test]
    fn shared_source_guard_is_restored_after_success_error_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        write_tile(&dir.path().join("N35W110.hgt"), 1201);
        for outcome in 0..3 {
            let mut terrain = Terrain::new(Some(dir.path().into()), 42.);
            terrain.missing = 7;
            let control = crate::Control::default();
            let result = terrain.with_search_scope(&control, |terrain| {
                let view = terrain.parallel_view(&control)?.unwrap();
                terrain.retain_source_guard(&view)?;
                assert!(terrain.shared.is_none());
                assert!(terrain.guard.is_some());
                if outcome == 1 {
                    bail!("intentional search failure");
                }
                if outcome == 2 {
                    control.stop();
                }
                Ok(())
            });
            assert_eq!(result.is_ok(), outcome == 0);
            assert!(terrain.guard.is_none() && terrain.shared.is_none());
            assert!(terrain.tiles.is_empty());
            assert_eq!(terrain.missing, 7);
        }
    }

    #[test]
    fn shared_tiles_keep_filesystem_case_lookup_and_unsupported_serial_fallback() {
        let dir = tempfile::tempdir().unwrap();
        write_tile(&dir.path().join("n35w110.HGT"), 1201);
        let mut terrain = Terrain::new(Some(dir.path().into()), 42.);
        let prepared = terrain.parallel_view(&crate::Control::default()).unwrap();
        let point = Point {
            lat: 35.25,
            lon: -109.75,
            alt: 0.,
        };
        if dir.path().join("N35W110.hgt").exists() {
            assert_eq!(
                terrain.elevation(point).to_bits(),
                prepared.unwrap().elevation(point).to_bits()
            );
        } else {
            assert!(prepared.is_none());
            assert_eq!(terrain.elevation(point), 42.);
        }
        assert!(
            Terrain::new(Some(dir.path().join("absent")), 0.)
                .parallel_view(&crate::Control::default())
                .unwrap()
                .is_none()
        );
        #[cfg(unix)]
        {
            let other = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(
                dir.path().join("n35w110.HGT"),
                other.path().join("N35W110.hgt"),
            )
            .unwrap();
            assert!(
                Terrain::new(Some(other.path().into()), 0.)
                    .parallel_view(&crate::Control::default())
                    .unwrap()
                    .is_none()
            );
        }
    }
}
