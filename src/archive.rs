//! Explicit date coverage, independent of the recording's unknown UTC clock.
use crate::{Control, Progress};
use anyhow::{Context, Result, bail};
use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveProvider {
    #[default]
    AdsbLol,
    AdsbExchangeSample,
    WebtrakChicago,
    WebtrakLax,
}

impl ArchiveProvider {
    pub fn webtrak_site(self) -> Option<crate::webtrak::Site> {
        match self {
            Self::AdsbLol | Self::AdsbExchangeSample => None,
            Self::WebtrakChicago => Some(crate::webtrak::Site::Chicago),
            Self::WebtrakLax => Some(crate::webtrak::Site::Lax),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchiveRequest {
    pub date_from: String,
    #[serde(default)]
    pub date_to: Option<String>,
    pub cache: PathBuf,
    #[serde(default)]
    pub provider: ArchiveProvider,
}

impl ArchiveRequest {
    pub fn dates(&self) -> Result<Vec<String>> {
        let from = NaiveDate::parse_from_str(&self.date_from, "%Y-%m-%d")
            .context("Archive start date must be YYYY-MM-DD")?;
        let to = self
            .date_to
            .as_deref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .context("Archive end date must be YYYY-MM-DD")?
            .unwrap_or(from);
        if from > to || (to - from).num_days() > 30 {
            bail!("Choose an ordered archive date range of at most 31 days");
        }
        if from < NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()
            || to >= chrono::Utc::now().date_naive()
        {
            bail!(
                "Daily flight archives cover dates from 2023 through yesterday UTC; the latest day may not be published yet"
            );
        }
        if self.cache.as_os_str().is_empty() {
            bail!("Choose a folder for the flight archive cache");
        }
        Ok((0..=(to - from).num_days())
            .map(|i| {
                from.checked_add_days(Days::new(i as u64))
                    .unwrap()
                    .to_string()
            })
            .collect())
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
        let dates = self.dates()?;
        if self.provider == ArchiveProvider::AdsbExchangeSample {
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
                    ArchiveProvider::AdsbLol => {
                        crate::data::download_day(date, &self.cache, control, &day_progress)
                    }
                    ArchiveProvider::AdsbExchangeSample => crate::sample_archive::download_day(
                        date,
                        &self.cache,
                        bounds,
                        control,
                        &day_progress,
                    ),
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
