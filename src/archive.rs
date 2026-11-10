//! Explicit date coverage, independent of the recording's unknown UTC clock.
use crate::{Control, Progress};
use anyhow::{Context, Result, bail};
use chrono::{Datelike, Days, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveProvider {
    #[default]
    AdsbLol,
    AdsbExchangeSample,
    AdsbExchangeSampleHires,
    WebtrakChicago,
    WebtrakLax,
}

impl ArchiveProvider {
    pub fn sample_resolution(self) -> Option<crate::sample_archive::TraceResolution> {
        use crate::sample_archive::TraceResolution;
        match self {
            Self::AdsbExchangeSample => Some(TraceResolution::Standard),
            Self::AdsbExchangeSampleHires => Some(TraceResolution::High),
            _ => None,
        }
    }

    pub fn webtrak_site(self) -> Option<crate::webtrak::Site> {
        match self {
            Self::AdsbLol | Self::AdsbExchangeSample | Self::AdsbExchangeSampleHires => None,
            Self::WebtrakChicago => Some(crate::webtrak::Site::Chicago),
            Self::WebtrakLax => Some(crate::webtrak::Site::Lax),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchiveRequest {
    #[serde(default)]
    pub date_from: String,
    #[serde(default)]
    pub date_to: Option<String>,
    pub cache: PathBuf,
    #[serde(default)]
    pub provider: ArchiveProvider,
}

impl ArchiveRequest {
    pub fn validate(&self) -> Result<()> {
        if self.cache.as_os_str().is_empty() {
            bail!("Choose a folder for the flight archive cache");
        }
        if self.date_from.trim().is_empty() {
            if self.date_to.as_ref().is_some_and(|s| !s.trim().is_empty()) {
                bail!("Set both range endpoints, or leave both blank to discover dates");
            }
        } else {
            self.dates()?;
        }
        Ok(())
    }

    pub fn resolve_dates(
        &self,
        control: &Control,
        progress: &Progress<'_>,
    ) -> Result<(Vec<String>, Option<crate::availability::Catalog>)> {
        self.validate()?;
        control.check()?;
        if !self.date_from.trim().is_empty() || control.offline {
            return Ok((self.dates()?, None));
        }
        let catalog = crate::availability::discover(self.provider, &self.cache, control, progress)?;
        let mut dates: std::collections::BTreeSet<_> = catalog.dates.iter().cloned().collect();
        // Keep older prepared days after a rolling provider window expires.
        dates.extend(self.cached_dates_allow_empty()?);
        Ok((dates.into_iter().collect(), Some(catalog)))
    }

    fn first_year(&self) -> i32 {
        if self.provider.sample_resolution().is_some() {
            2016
        } else {
            2023
        }
    }
    pub fn dates(&self) -> Result<Vec<String>> {
        if self.date_from.trim().is_empty() {
            if self.date_to.as_ref().is_some_and(|s| !s.trim().is_empty()) {
                bail!("Set both range endpoints, or leave both blank to search cached dates");
            }
            return self.cached_dates();
        }
        let from = NaiveDate::parse_from_str(&self.date_from, "%Y-%m-%d")
            .context("Archive start date must be YYYY-MM-DD")?;
        let to = self
            .date_to
            .as_deref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .context("Archive end date must be YYYY-MM-DD")?
            .unwrap_or(from);
        if from > to {
            bail!("Archive end date must be on or after the start date");
        }
        if from < NaiveDate::from_ymd_opt(self.first_year(), 1, 1).unwrap()
            || to >= chrono::Utc::now().date_naive()
        {
            bail!(
                "This provider supports dates from {} through yesterday UTC; the latest day may not be published yet",
                self.first_year()
            );
        }
        if self.cache.as_os_str().is_empty() {
            bail!("Choose a folder for the flight archive cache");
        }
        let dates: Vec<String> = (0..=(to - from).num_days())
            .map(|i| {
                from.checked_add_days(Days::new(i as u64))
                    .unwrap()
                    .to_string()
            })
            .filter(|date| {
                self.provider.sample_resolution().is_none()
                    || self.date_to.is_none()
                    || NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok_and(|day| day.day() == 1)
            })
            .collect();
        if dates.is_empty() {
            bail!("The selected range contains no first-of-month sample dates");
        }
        Ok(dates)
    }

    fn cached_dates(&self) -> Result<Vec<String>> {
        let dates = self.cached_dates_allow_empty()?;
        if dates.is_empty() {
            bail!(
                "No cached dates found for this provider. Prepare flight data online before offline analysis."
            );
        }
        Ok(dates)
    }

    fn cached_dates_allow_empty(&self) -> Result<Vec<String>> {
        let mut dates = std::collections::BTreeSet::new();
        let entries = match std::fs::read_dir(&self.cache) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(error).context("Could not read the flight cache"),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let date = match self.provider {
                ArchiveProvider::AdsbLol => name.as_str(),
                ArchiveProvider::WebtrakChicago => name.strip_prefix("webtrak-cda-").unwrap_or(""),
                ArchiveProvider::WebtrakLax => name.strip_prefix("webtrak-lax4-").unwrap_or(""),
                ArchiveProvider::AdsbExchangeSample | ArchiveProvider::AdsbExchangeSampleHires => {
                    if !entry.path().join("sample-manifest.json").is_file() {
                        continue;
                    }
                    let prefix = self.provider.sample_resolution().unwrap().cache_prefix();
                    name.strip_prefix(&format!("{prefix}-"))
                        .and_then(|s| s.get(..10))
                        .unwrap_or("")
                }
            };
            if let Ok(day) = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                && day >= NaiveDate::from_ymd_opt(self.first_year(), 1, 1).unwrap()
                && day < chrono::Utc::now().date_naive()
            {
                dates.insert(day.to_string());
            }
        }
        Ok(dates.into_iter().collect())
    }

    pub fn ensure(&self, control: &Control, progress: &Progress<'_>) -> Result<Vec<PathBuf>> {
        self.ensure_for_bounds(None, control, progress)
    }

    pub fn ensure_for_bounds(
        &self,
        bounds: Option<crate::geo::Bounds>,
        control: &Control,
        progress: &Progress<'_>,
    ) -> Result<Vec<PathBuf>> {
        if self.provider.sample_resolution().is_some() {
            crate::sample_archive::validate_bounds(bounds)?;
        }
        let (dates, _) = self.resolve_dates(control, progress)?;
        if self.provider.sample_resolution().is_some() {
            for date in &dates {
                crate::sample_archive::validate(date, bounds)?;
            }
        }
        let mut paths = Vec::with_capacity(dates.len());
        for (i, date) in dates.iter().enumerate() {
            control.check()?;
            progress(
                &format!("Preparing flight archive {date}"),
                i as f32 / dates.len() as f32,
            );
            let day_progress = |m: &str, p| {
                progress(&format!("{date}: {m}"), (i as f32 + p) / dates.len() as f32);
            };
            paths.push(
                match self.provider {
                    ArchiveProvider::AdsbLol => crate::archive_refresh::download_day(
                        date,
                        &self.cache,
                        control,
                        &day_progress,
                    ),
                    ArchiveProvider::AdsbExchangeSample
                    | ArchiveProvider::AdsbExchangeSampleHires => {
                        crate::sample_archive::download_day_with_resolution(
                            date,
                            &self.cache,
                            bounds,
                            self.provider.sample_resolution().unwrap(),
                            control,
                            &day_progress,
                        )
                    }
                    ArchiveProvider::WebtrakChicago => crate::webtrak::download_day(
                        crate::webtrak::Site::Chicago,
                        date,
                        &self.cache,
                        control,
                        &day_progress,
                    ),
                    ArchiveProvider::WebtrakLax => crate::webtrak::download_day(
                        crate::webtrak::Site::Lax,
                        date,
                        &self.cache,
                        control,
                        &day_progress,
                    ),
                }
                .with_context(|| {
                    format!(
                        "Could not obtain flight data for {date}; no dates were silently omitted"
                    )
                })?,
            );
        }
        Ok(paths)
    }
}
