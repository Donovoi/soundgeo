use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

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
    pub missing: usize,
}
impl Terrain {
    pub(crate) fn has_tiles(&self) -> bool {
        self.root.is_some()
    }
    pub(crate) fn empty_cache_clone(&self) -> Self {
        Self::with_datum(self.root.clone(), self.fallback, self.datum)
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
            if let Some(data) = tile {
                let n = (data.len() as f64).sqrt() as usize;
                let x = ((p.lon - lon as f64) * (n - 1) as f64).round() as usize;
                let y = ((1. - (p.lat - lat as f64)) * (n - 1) as f64).round() as usize;
                let h = data[y.min(n - 1) * n + x.min(n - 1)];
                if h != i16::MIN {
                    // SRTM .hgt heights always reference EGM96, independently
                    // of the selected datum for the manual fallback height.
                    return h as f64 + geoid_undulation(p.lat, p.lon);
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
