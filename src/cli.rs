use crate::ui;
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use soundgeo::archive::{ArchiveProvider, ArchiveRequest};
use soundgeo::{Control, Job};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "SoundGeo — local aircraft-Doppler geolocation workbench"
)]
struct Args {
    /// Suppress progress and success messages; errors and JSON output remain.
    #[arg(long, short, global = true)]
    quiet: bool,
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
        config: Option<PathBuf>,
        /// Recording path; overrides the configuration's media path.
        #[arg(long, required_unless_present = "config")]
        input: Option<PathBuf>,
        /// Flight data path; overrides the configuration's tracks path.
        #[arg(long, required_unless_present_any = ["config", "date"])]
        tracks: Option<PathBuf>,
        /// Download/reuse the full UTC day's flight data; recording time stays unknown.
        #[arg(long)]
        date: Option<String>,
        /// Last inclusive archive date, when searching several possible days.
        #[arg(long, requires = "date")]
        date_to: Option<String>,
        /// Cache directory for automatically fetched flight archives.
        #[arg(long, requires = "date")]
        cache: Option<PathBuf>,
        /// Free flight source. Chicago replay covers the local airport radar area.
        #[arg(long, requires = "date")]
        provider: Option<ArchiveProvider>,
        /// Report file, or - for JSON on stdout.
        #[arg(long)]
        output: PathBuf,
        /// Return exit code 3 if analysis finishes without a candidate.
        #[arg(long)]
        require_match: bool,
    },
    /// Write a synthetic recording, flight CSV and unknown-time job.
    Demo {
        #[arg(long, default_value = "demo")]
        output: PathBuf,
    },
    /// Fetch a full UTC day from a free source; verified data is cached.
    Download {
        #[arg(long)]
        date: String,
        #[arg(long, default_value = "cache")]
        output: PathBuf,
        #[arg(long, default_value = "adsb-lol")]
        provider: ArchiveProvider,
    },
    /// Open the desktop app with an existing report.
    View { report: PathBuf },
}
pub fn entry(desktop_default: bool) -> std::process::ExitCode {
    let args = Args::parse();
    if !desktop_default && args.command.is_none() {
        return match Args::command().print_help() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(_) => std::process::ExitCode::FAILURE,
        };
    }
    match run(args) {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<u8> {
    let progress = |message: &str, fraction: f32| {
        if !args.quiet {
            eprintln!("{:3.0}% {message}", fraction * 100.);
        }
    };
    match args.command {
        Some(Commands::Catalog { output }) => {
            soundgeo::aircraft::download(&output)?;
            if !args.quiet {
                println!("Aircraft catalog: {}", output.display());
            }
        }
        Some(Commands::Analyze {
            config,
            input,
            tracks,
            date,
            date_to,
            cache,
            provider,
            output,
            require_match,
        }) => {
            if output.canonicalize().ok().is_some_and(|p| {
                config
                    .as_ref()
                    .is_some_and(|c| c.canonicalize().is_ok_and(|c| c == p))
            }) {
                anyhow::bail!("The report output cannot overwrite its input configuration");
            }
            let mut job: Job = match &config {
                Some(path) => serde_json::from_reader(
                    std::fs::File::open(path).context("Could not open configuration")?,
                )
                .context("Invalid job JSON")?,
                None => Job::default(),
            };
            let parent = config
                .as_deref()
                .and_then(std::path::Path::parent)
                .unwrap_or_else(|| std::path::Path::new("."));
            for path in [&mut job.media, &mut job.tracks] {
                if !path.as_os_str().is_empty() && path.is_relative() {
                    *path = parent.join(&*path);
                }
            }
            if let Some(request) = &mut job.archive
                && request.cache.is_relative()
            {
                request.cache = parent.join(&request.cache);
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
            // Explicit CLI paths are relative to the caller's working directory.
            if let Some(input) = input {
                job.media = input;
            }
            if let Some(tracks) = tracks {
                job.tracks = tracks;
                job.archive = None;
            }
            if let Some(date_from) = date {
                let provider = provider.unwrap_or_else(|| {
                    job.archive.as_ref().map(|r| r.provider).unwrap_or_default()
                });
                job.archive = Some(soundgeo::archive::ArchiveRequest {
                    date_from,
                    date_to,
                    cache: cache.unwrap_or_else(|| PathBuf::from("flight-cache")),
                    provider,
                });
            }
            let report = soundgeo::analyze(&job, &Control::default(), &progress)?;
            if output == std::path::Path::new("-") {
                use std::io::Write;
                let mut stdout = std::io::stdout().lock();
                serde_json::to_writer_pretty(&mut stdout, &report)?;
                writeln!(stdout)?;
            } else {
                soundgeo::save_report(&report, &output)?;
                if !args.quiet {
                    println!(
                        "{}; {} candidate(s). Report: {}",
                        report.status,
                        report.candidates.len(),
                        output.display()
                    );
                }
            }
            if require_match && report.candidates.is_empty() {
                return Ok(3);
            }
        }
        Some(Commands::Demo { output }) => {
            let path = std::path::absolute(&output)?;
            soundgeo::demo::create(&path)?;
            if !args.quiet {
                println!("Demo: {}", path.display());
            }
        }
        Some(Commands::Download {
            date,
            output,
            provider,
        }) => {
            let paths = ArchiveRequest {
                date_from: date,
                date_to: None,
                cache: output,
                provider,
            }
            .ensure(&Control::default(), &progress)?;
            if !args.quiet {
                for path in paths {
                    println!("Archive: {}", path.display());
                }
            }
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
    Ok(0)
}
