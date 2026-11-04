//! Classification uses explicit ICAO descriptions, never a guess from a callsign.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AircraftKind {
    Propeller,
    Jet,
    Rotorcraft,
    Other,
    #[default]
    Unknown,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AircraftFilter {
    All,
    #[default]
    PropellerOrUnknown,
    PropellerOnly,
}
impl AircraftFilter {
    pub fn accepts(self, kind: AircraftKind) -> bool {
        match self {
            Self::All => true,
            Self::PropellerOrUnknown => {
                matches!(kind, AircraftKind::Propeller | AircraftKind::Unknown)
            }
            Self::PropellerOnly => kind == AircraftKind::Propeller,
        }
    }
}
pub fn classify(description: &str) -> AircraftKind {
    let s = description.trim().to_ascii_uppercase();
    let b = s.as_bytes();
    if b.len() != 3 || !b[1].is_ascii_digit() {
        return AircraftKind::Unknown;
    }
    if matches!(b[0], b'H' | b'G') {
        return AircraftKind::Rotorcraft;
    }
    match (b[0], b[2]) {
        (b'L' | b'S' | b'A', b'P' | b'T') => AircraftKind::Propeller,
        (_, b'J') => AircraftKind::Jet,
        (_, b'E' | b'P' | b'T') => AircraftKind::Other,
        _ => AircraftKind::Unknown,
    }
}
#[derive(Default)]
pub struct Catalog(BTreeMap<String, AircraftKind>);
impl Catalog {
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).context("Could not read aircraft type catalog")?;
        Self::from_bytes(&bytes)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 4_000_000 {
            bail!("Aircraft catalog exceeds 4 MB");
        }
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let entries = value
            .as_object()
            .context("Aircraft type catalog must be a JSON object")?;
        let mut catalog = BTreeMap::new();
        for (code, row) in entries {
            let description = row
                .get(1)
                .and_then(|s| s.as_str())
                .or_else(|| row.get("description").and_then(|s| s.as_str()))
                .unwrap_or("");
            catalog.insert(code.to_uppercase(), classify(description));
        }
        if catalog.is_empty() {
            bail!("Aircraft catalog contains no entries");
        }
        Ok(Self(catalog))
    }
    pub fn kind(&self, type_code: &str, description: &str) -> AircraftKind {
        let direct = classify(description);
        if direct != AircraftKind::Unknown {
            direct
        } else {
            self.0
                .get(&type_code.trim().to_ascii_uppercase())
                .copied()
                .unwrap_or_default()
        }
    }
}

pub fn download(path: &Path) -> Result<()> {
    use std::io::Read;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("soundgeo/0.2")
        .build()?;
    let response=client.get("https://raw.githubusercontent.com/Mictronics/readsb-protobuf/dev/webapp/src/db/types.json").send()?.error_for_status()?;
    let mut bytes = vec![];
    response.take(4_000_001).read_to_end(&mut bytes)?;
    Catalog::from_bytes(&bytes)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut temp, &bytes)?;
    temp.persist(path)?;
    std::fs::write(
        path.with_extension("source.txt"),
        "Aircraft type descriptions: Mictronics/readsb-protobuf, webapp/src/db/types.json\nhttps://github.com/Mictronics/readsb-protobuf\nRetrieved separately for local use. License details: see the source repository's LICENSE file. Retain source attribution when redistributing this catalog.\n",
    )?;
    Ok(())
}
