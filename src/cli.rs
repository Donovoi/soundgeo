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
    /// Use cached data with networking disabled.
    #[arg(long, global = true)]
    offline: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    /// Discover free flight archive dates without downloading flight files.
    Dates {
        #[arg(long, default_value = "adsb-lol")]
        provider: ArchiveProvider,
        #[arg(long, default_value = "flight-cache")]
        cache: PathBuf,
        /// Report file, or - for JSON on stdout.
        #[arg(long, default_value = "-")]
        output: PathBuf,
    },
    /// Prepare free GeoNames place data for optional offline video clues.
    VideoData {
        /// ALL, or a two-letter country code such as US.
        #[arg(long, default_value = "ALL")]
        country: String,
        #[arg(long, default_value = "video-data")]
        output: PathBuf,
    },
    /// Read visible place names and clocks locally. Produces a separate clue report.
    VideoClues {
        #[arg(long)]
        input: PathBuf,
        /// GeoNames country or worldwide .txt file.
        #[arg(long)]
        gazetteer: PathBuf,
        #[arg(long)]
        ffmpeg: Option<PathBuf>,
        #[arg(long)]
        tesseract: Option<PathBuf>,
        #[arg(long)]
        tessdata: Option<PathBuf>,
        #[arg(long, default_value = "3", value_parser = clap::value_parser!(u8).range(1..=12))]
        frames: u8,
        #[arg(long, allow_hyphen_values = true, requires_all = ["north", "west", "east"])]
        south: Option<f64>,
        #[arg(long, allow_hyphen_values = true, requires = "south")]
        north: Option<f64>,
        #[arg(long, allow_hyphen_values = true, requires = "south")]
        west: Option<f64>,
        #[arg(long, allow_hyphen_values = true, requires = "south")]
        east: Option<f64>,
        /// Report file, or - for JSON on stdout.
        #[arg(long)]
        output: PathBuf,
        /// Return exit code 3 when no place names match.
        #[arg(long)]
        require_match: bool,
    },
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
        #[arg(long, required_unless_present_any = ["config", "date", "cache"])]
        tracks: Option<PathBuf>,
        /// Download/reuse the full UTC day's flight data; recording time stays unknown.
        #[arg(long)]
        date: Option<String>,
        /// Last inclusive archive date, when searching several possible days.
        #[arg(long, requires = "date")]
        date_to: Option<String>,
        /// Cache directory for automatically fetched flight archives.
        #[arg(long)]
        cache: Option<PathBuf>,
        /// Free flight source. Airport replay covers the selected local radar area.
        #[arg(long)]
        provider: Option<ArchiveProvider>,
        /// Download/reuse free terrain for the configured search area in this folder.
        #[arg(long)]
        terrain_cache: Option<PathBuf>,
        /// Maximum search hypotheses; overrides the saved job. Larger limits take longer.
        #[arg(long, value_parser = clap::value_parser!(u64).range(100..=100_000_000))]
        max_hypotheses: Option<u64>,
        /// Maximum CPU workers for flight-corridor matching (default: up to 4).
        #[arg(long)]
        search_threads: Option<std::num::NonZeroUsize>,
        /// Sound matching method; passage timing needs bounds and at least eight minutes.
        #[arg(long)]
        method: Option<soundgeo::AnalysisMethod>,
        /// Report file, or - for JSON on stdout.
        #[arg(long)]
        output: PathBuf,
        /// Return exit code 3 if analysis finishes without a candidate.
        #[arg(long)]
        require_match: bool,
        /// Require candidates_found: exit 3 for no candidate, or 4 for ambiguity.
        #[arg(long)]
        require_unambiguous: bool,
    },
    /// Write a synthetic recording, flight CSV and unknown-time job.
    Demo {
        #[arg(long, default_value = "demo")]
        output: PathBuf,
    },
    /// Prepare flight data and optional terrain for later offline analysis.
    Download {
        /// First date. Omit both dates to discover the provider's available days.
        #[arg(long)]
        date: Option<String>,
        /// Last date to prepare, inclusive.
        #[arg(long)]
        date_to: Option<String>,
        /// Prepare terrain for the same bounds in this folder.
        #[arg(long)]
        terrain_cache: Option<PathBuf>,
        #[arg(long, default_value = "cache")]
        output: PathBuf,
        #[arg(long, default_value = "adsb-lol")]
        provider: ArchiveProvider,
        /// Required with the free first-of-month sample source.
        #[arg(long, allow_hyphen_values = true, requires_all = ["north", "west", "east"])]
        south: Option<f64>,
        #[arg(long, allow_hyphen_values = true, requires = "south")]
        north: Option<f64>,
        #[arg(long, allow_hyphen_values = true, requires = "south")]
        west: Option<f64>,
        #[arg(long, allow_hyphen_values = true, requires = "south")]
        east: Option<f64>,
    },
    /// Download free terrain for a region before analysis (at most 64 tiles).
    Terrain {
        #[arg(long, allow_hyphen_values = true)]
        south: f64,
        #[arg(long, allow_hyphen_values = true)]
        north: f64,
        #[arg(long, allow_hyphen_values = true)]
        west: f64,
        #[arg(long, allow_hyphen_values = true)]
        east: f64,
        #[arg(long, default_value = "terrain-cache")]
        output: PathBuf,
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
    let control = Control::default().with_offline(args.offline);
    let progress = |message: &str, fraction: f32| {
        if !args.quiet {
            eprintln!("{:3.0}% {message}", fraction * 100.);
        }
    };
    match args.command {
        Some(Commands::Dates {
            provider,
            cache,
            output,
        }) => {
            let catalog = soundgeo::availability::discover(provider, &cache, &control, &progress)?;
            soundgeo::availability::write_report(&catalog, &output, &cache)?;
        }
        Some(Commands::VideoData { country, output }) => {
            let path = soundgeo::visual_data::prepare(&country, &output, &control, &progress)?;
            if !args.quiet {
                println!("Place data: {}", path.display());
            }
        }
        Some(Commands::VideoClues {
            input,
            gazetteer,
            ffmpeg,
            tesseract,
            tessdata,
            frames,
            south,
            north,
            west,
            east,
            output,
            require_match,
        }) => {
            let options = soundgeo::visual::Options {
                media: input,
                gazetteer,
                ffmpeg,
                tesseract,
                tessdata,
                frames: frames as usize,
                bounds: south.map(|south| soundgeo::geo::Bounds {
                    south,
                    north: north.unwrap(),
                    west: west.unwrap(),
                    east: east.unwrap(),
                }),
            };
            let report = soundgeo::visual::analyze(&options, &control, &progress)?;
            soundgeo::visual::write_report(
                &report,
                &output,
                &[&options.media, &options.gazetteer],
            )?;
            return Ok(if require_match && report.places.is_empty() {
                3
            } else {
                0
            });
        }
        Some(Commands::Terrain {
            south,
            north,
            west,
            east,
            output,
        }) => {
            let records = soundgeo::terrain_archive::ensure(
                soundgeo::geo::Bounds {
                    south,
                    north,
                    west,
                    east,
                },
                &output,
                &control,
                &progress,
            )?;
            if !args.quiet {
                println!(
                    "{} verified terrain tiles: {}",
                    records.len(),
                    output.display()
                );
            }
        }
        Some(Commands::Catalog { output }) => {
            control.require_network(&output.display().to_string())?;
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
            terrain_cache,
            max_hypotheses,
            search_threads,
            method,
            output,
            require_match,
            require_unambiguous,
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
            if date.is_some() || cache.is_some() || provider.is_some() {
                let mut request = job.archive.clone().unwrap_or(ArchiveRequest {
                    date_from: String::new(),
                    date_to: None,
                    cache: "flight-cache".into(),
                    provider: Default::default(),
                });
                if let Some(from) = date {
                    request.date_from = from;
                    request.date_to = date_to;
                }
                if let Some(cache) = cache {
                    request.cache = cache;
                }
                if let Some(provider) = provider {
                    request.provider = provider;
                }
                job.archive = Some(request);
            }
            job.offline |= args.offline;
            if let Some(limit) = max_hypotheses {
                // Clap limits this to 100 million, which also fits 32-bit usize.
                job.max_hypotheses = limit as usize;
            }
            if let Some(threads) = search_threads {
                job.search_threads = Some(threads.get());
            }
            if let Some(method) = method {
                job.analysis_method = method;
            }
            if let Some(path) = terrain_cache {
                job.terrain = Some(path);
                job.download_terrain = true;
            }
            let report = soundgeo::analyze(&job, &control, &progress)?;
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
                        report.candidate_count(),
                        output.display()
                    );
                }
            }
            if (require_match || require_unambiguous) && report.candidate_count() == 0 {
                return Ok(3);
            }
            if require_unambiguous && report.status != "candidates_found" {
                return Ok(4);
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
            date_to,
            terrain_cache,
            output,
            provider,
            south,
            north,
            west,
            east,
        }) => {
            let bounds = south.map(|south| soundgeo::geo::Bounds {
                south,
                north: north.unwrap(),
                west: west.unwrap(),
                east: east.unwrap(),
            });
            if let Some(bounds) = bounds {
                bounds.validate()?;
            }
            let job = Job {
                archive: Some(ArchiveRequest {
                    date_from: date.unwrap_or_default(),
                    date_to,
                    cache: output,
                    provider,
                }),
                bounds,
                download_terrain: terrain_cache.is_some(),
                terrain: terrain_cache,
                offline: args.offline,
                ..Default::default()
            };
            let prepared = soundgeo::prepare_data(&job, &control, &progress)?;
            let paths = prepared.flight_sources;
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
