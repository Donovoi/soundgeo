#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod ui;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use soundgeo::{Control, Job};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "SoundGeo — local aircraft-Doppler geolocation workbench"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    /// Download the small ICAO aircraft type catalog for propeller filtering.
    Catalog {
        #[arg(long, default_value = "aircraft-types.json")]
        output: PathBuf,
    },
    /// Analyze a video/audio file using a saved job configuration.
    Analyze {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Write a synthetic recording, flight CSV and unknown-time job.
    Demo {
        #[arg(long, default_value = "demo")]
        output: PathBuf,
    },
    /// Fetch a public ADSB.lol day (several GB); completed parts are cached.
    Download {
        #[arg(long)]
        date: String,
        #[arg(long, default_value = "cache")]
        output: PathBuf,
    },
    /// Open the desktop app with an existing report.
    View { report: PathBuf },
}
fn main() -> Result<()> {
    // GUI launches stay console-free; CLI invocations attach to their parent's console.
    #[cfg(windows)]
    unsafe {
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        if std::env::args_os().len() > 1 {
            AttachConsole(u32::MAX);
        }
    }
    match Args::parse().command {
        Some(Commands::Catalog { output }) => {
            soundgeo::aircraft::download(&output)?;
            println!("Aircraft catalog: {}", output.display());
        }
        Some(Commands::Analyze { config, output }) => {
            let mut job: Job = serde_json::from_reader(std::fs::File::open(&config)?)?;
            let parent = config.parent().unwrap_or_else(|| std::path::Path::new("."));
            for path in [&mut job.media, &mut job.tracks] {
                if path.is_relative() {
                    *path = parent.join(&*path);
                }
            }
            if let Some(path) = &mut job.terrain
                && path.is_relative()
            {
                *path = parent.join(&*path);
            }
            if let Some(path) = &mut job.type_catalog
                && path.is_relative()
            {
                *path = parent.join(&*path);
            }
            if let Some(path) = &mut job.ffmpeg
                && path.is_relative()
                && path.components().count() > 1
            {
                *path = parent.join(&*path);
            }
            let report = soundgeo::analyze(&job, &Control::default(), &|message, fraction| {
                eprintln!("{:3.0}% {message}", fraction * 100.)
            })?;
            soundgeo::save_report(&report, &output)?;
            println!(
                "{}; {} candidate(s). Report: {}",
                report.status,
                report.candidates.len(),
                output.display()
            );
        }
        Some(Commands::Demo { output }) => {
            let path = std::path::absolute(&output)?;
            soundgeo::demo::create(&path)?;
            println!("Demo: {}", path.display());
        }
        Some(Commands::Download { date, output }) => {
            let path =
                soundgeo::data::download_day(&date, &output, &Control::default(), &|m, p| {
                    eprintln!("{:3.0}% {m}", p * 100.)
                })?;
            println!("Archive: {}", path.display());
        }
        command => {
            let report = if let Some(Commands::View { report }) = command {
                Some(serde_json::from_reader(std::fs::File::open(report)?)?)
            } else {
                None
            };
            let options = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_inner_size([1200., 850.])
                    .with_min_inner_size([900., 650.]),
                ..Default::default()
            };
            eframe::run_native(
                "SoundGeo",
                options,
                Box::new(move |cc| Ok(Box::new(ui::App::new(cc, report)))),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("Could not open the desktop window")?;
        }
    }
    Ok(())
}
