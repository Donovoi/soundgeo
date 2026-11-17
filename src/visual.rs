//! Optional, local OCR. These clues do not change an audio search or its result.
use crate::{Control, Job, Progress, audio, geo::Bounds};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub media: PathBuf,
    pub gazetteer: PathBuf,
    pub ffmpeg: Option<PathBuf>,
    pub tesseract: Option<PathBuf>,
    pub tessdata: Option<PathBuf>,
    pub bounds: Option<Bounds>,
    /// Zero uses the default of three frames.
    pub frames: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameText {
    /// Seconds from the first video presentation timestamp.
    pub offset_s: f64,
    pub view: String,
    pub text: String,
    /// Literal dates and times. No timezone or recording start is inferred.
    pub clock_text: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaceClue {
    pub geoname_id: u64,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub feature_code: String,
    pub country: String,
    pub matched_name: String,
    pub frame_indices: Vec<usize>,
    pub source_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub method: String,
    pub status: String,
    pub offline: bool,
    pub media_sha256: String,
    pub gazetteer_sha256: String,
    pub gazetteer_source: Option<crate::visual_data::Source>,
    pub ocr_version: String,
    pub bounds: Option<Bounds>,
    pub frames: Vec<FrameText>,
    pub places: Vec<PlaceClue>,
    pub warnings: Vec<String>,
}

pub(crate) fn hash_file(path: &Path, control: &Control) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("Open {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buf = [0; 128 * 1024];
    loop {
        control.check()?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn run_file(command: &mut Command, dir: &Path, control: &Control) -> Result<PathBuf> {
    let out = dir.join("stdout.txt");
    let err = dir.join("stderr.txt");
    let mut child = command
        .stdin(Stdio::null())
        .stdout(File::create(&out)?)
        .stderr(File::create(&err)?)
        .spawn()
        .with_context(|| format!("Start {}", command.get_program().to_string_lossy()))?;
    let started = Instant::now();
    let status = loop {
        if let Err(error) = control.check() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Video clue tool timed out after 120 seconds");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    fn bounded(path: &Path) -> Result<String> {
        let mut text = String::new();
        File::open(path)?
            .take(256 * 1024)
            .read_to_string(&mut text)?;
        Ok(text)
    }
    if !status.success() {
        bail!("Video clue tool failed: {}", bounded(&err)?);
    }
    Ok(out)
}

fn run(command: &mut Command, dir: &Path, control: &Control) -> Result<String> {
    let out = run_file(command, dir, control)?;
    let mut text = String::new();
    File::open(out)?
        .take(256 * 1024)
        .read_to_string(&mut text)?;
    Ok(text)
}

fn video_span(reader: impl BufRead, control: &Control) -> Result<(f64, f64, usize)> {
    let (mut start, mut end, mut packets) = (f64::INFINITY, f64::NEG_INFINITY, 0);
    for line in reader.lines() {
        control.check()?;
        let line = line?;
        let number = |key: &str| {
            line.split('|').find_map(|value| {
                value
                    .strip_prefix(key)?
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
            })
        };
        if let Some(pts) = number("pts_time=") {
            let duration = number("duration_time=").unwrap_or(0.).max(0.);
            start = start.min(pts);
            end = end.max(pts + duration);
            packets += 1;
        }
    }
    if packets == 0 || !(end - start).is_finite() {
        bail!("The file has no usable video packet timestamps");
    }
    Ok((start, end - start, packets))
}

fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clocks(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|s| {
            let s = s.trim_matches(|c: char| !c.is_ascii_digit());
            let date = s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok();
            let time = s.len() == 8 && chrono::NaiveTime::parse_from_str(s, "%H:%M:%S").is_ok();
            (date || time).then(|| s.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn match_places(
    reader: impl BufRead,
    frames: &[FrameText],
    bounds: Option<Bounds>,
    control: &Control,
) -> Result<Vec<PlaceClue>> {
    // Phrase lookup makes a worldwide dump a streaming read, with bounded memory.
    let mut phrases: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for (i, frame) in frames.iter().enumerate() {
        // A line break may separate unrelated labels. Do not join across it.
        for line in frame.text.lines() {
            let normalized = normalize(line);
            let words: Vec<_> = normalized.split_whitespace().collect();
            for len in 2..=12.min(words.len()) {
                for phrase in words.windows(len) {
                    phrases.entry(phrase.join(" ")).or_default().insert(i);
                }
            }
        }
    }
    if phrases.is_empty() {
        return Ok(Vec::new());
    }
    let mut matches = BTreeMap::new();
    let mut rows = 0;
    for line in reader.lines() {
        control.check()?;
        let line = line?;
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() < 19 {
            bail!("Invalid GeoNames row: expected 19 tab-separated fields");
        }
        rows += 1;
        let latitude: f64 = fields[4].parse().context("Invalid GeoNames latitude")?;
        let longitude: f64 = fields[5].parse().context("Invalid GeoNames longitude")?;
        if !latitude.is_finite()
            || !longitude.is_finite()
            || latitude.abs() > 90.
            || longitude.abs() > 180.
        {
            bail!("Invalid GeoNames coordinates");
        }
        if bounds.is_some_and(|b| {
            latitude < b.south || latitude > b.north || longitude < b.west || longitude > b.east
        }) {
            continue;
        }
        let mut found = BTreeSet::new();
        let mut best = String::new();
        let mut best_words = 0;
        for name in [fields[1], fields[2]]
            .into_iter()
            .chain(fields[3].split(','))
        {
            let norm = normalize(name);
            // Short numeric aliases occur in GeoNames and in garbled OCR.
            // Require two words with letters before treating an alias as a name.
            let (words, letters) = norm
                .split_whitespace()
                .map(|word| word.chars().filter(|c| c.is_alphabetic()).count())
                .fold((0, 0), |(words, letters), n| {
                    (words + usize::from(n >= 2), letters + n)
                });
            if words < 2 || letters < 5 {
                continue;
            }
            if let Some(indices) = phrases.get(&norm) {
                found.extend(indices);
                let count = norm.split_whitespace().count();
                if count > best_words {
                    best = name.into();
                    best_words = count;
                }
            }
        }
        if found.is_empty() {
            continue;
        }
        let id: u64 = fields[0].parse().context("Invalid GeoNames ID")?;
        matches.insert(
            id,
            PlaceClue {
                geoname_id: id,
                name: fields[1].into(),
                latitude,
                longitude,
                feature_code: fields[7].into(),
                country: fields[8].into(),
                matched_name: best,
                frame_indices: found.into_iter().collect(),
                source_url: format!("https://www.geonames.org/{id}"),
            },
        );
        if matches.len() > 10_000 {
            bail!("Too many place-name matches. Review the OCR text or select a search area.");
        }
    }
    if rows == 0 {
        bail!("The GeoNames file is empty");
    }
    let mut places: Vec<_> = matches.into_values().collect();
    places.sort_by_key(|p| {
        (
            std::cmp::Reverse(normalize(&p.matched_name).split_whitespace().count()),
            std::cmp::Reverse(p.frame_indices.len()),
            p.geoname_id,
        )
    });
    Ok(places)
}

/// Reads local media and local reference data only, regardless of the global mode.
pub fn analyze(options: &Options, control: &Control, progress: &Progress<'_>) -> Result<Report> {
    control.check()?;
    if !options.media.is_file() {
        bail!("Select a local video file");
    }
    if !options.gazetteer.is_file() {
        bail!("Prepare a GeoNames file with video-data, then select the .txt file");
    }
    if let Some(bounds) = options.bounds {
        bounds.validate()?;
    }
    let count = if options.frames == 0 {
        3
    } else {
        options.frames
    };
    if count > 12 {
        bail!("Use 1 to 12 video frames");
    }
    let dir = tempfile::tempdir()?;
    progress("Checking local video and place data", 0.);
    let media_sha256 = hash_file(&options.media, control)?;
    let gazetteer_sha256 = hash_file(&options.gazetteer, control)?;
    let gazetteer_source =
        crate::visual_data::verify_source(&options.gazetteer, &gazetteer_sha256)?;
    let ffmpeg = audio::ffmpeg_path(&Job {
        ffmpeg: options.ffmpeg.clone(),
        ..Job::default()
    });
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let media = options.media.canonicalize()?;
    // Fragmented live-stream clips can report an absolute end timestamp as
    // their duration. Read the actual video packet span, including reordered PTS.
    let packet_file = run_file(
        audio::quiet_command(&ffprobe)
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-select_streams",
                "v:0",
                "-show_entries",
                "packet=pts_time,duration_time",
                "-of",
                "compact=p=0:nk=0",
            ])
            .arg(&media),
        dir.path(),
        control,
    )?;
    let (start, duration, packets) = video_span(BufReader::new(File::open(packet_file)?), control)?;
    let count = count.min(packets);
    let tesseract = options
        .tesseract
        .clone()
        .unwrap_or_else(|| "tesseract".into());
    let ocr_version = run(
        audio::quiet_command(&tesseract).arg("--version"),
        dir.path(),
        control,
    )?
    .lines()
    .next()
    .unwrap_or_default()
    .to_string();
    let mut frames = Vec::new();
    for i in 0..count {
        let offset = duration * (i as f64 + 0.5) / count as f64;
        for (view, crop, inverted) in [
            ("full", "", false),
            ("top", "crop=iw:ih/10:0:0,", true),
            ("bottom", "crop=iw:ih/5:0:ih-ih/5,", true),
        ] {
            control.check()?;
            progress(
                &format!("Reading video text: frame {} of {count}, {view}", i + 1),
                (i as f32 + 0.3) / (count + 1) as f32,
            );
            let image = dir.path().join(format!("frame-{i}-{view}.png"));
            let invert = if inverted { "negate," } else { "" };
            let filter = format!(
                "{crop}scale='min(3600,iw*4)':-1:flags=lanczos,format=gray,{invert}pad=iw+40:ih+40:20:20:color=white"
            );
            run(
                audio::quiet_command(&ffmpeg)
                    .args([
                        "-v",
                        "error",
                        "-nostdin",
                        "-y",
                        "-protocol_whitelist",
                        "file,pipe",
                        "-seek_timestamp",
                        "1",
                        "-ss",
                        &(start + offset).to_string(),
                        "-i",
                    ])
                    .arg(&media)
                    .args(["-frames:v", "1", "-vf", &filter])
                    .arg(&image),
                dir.path(),
                control,
            )?;
            if !image.metadata().is_ok_and(|m| m.len() > 0) {
                bail!("No video frame was available at {offset:.3} seconds");
            }
            let mut cmd = audio::quiet_command(&tesseract);
            cmd.arg(&image).args(["stdout", "--psm", "11", "-l", "eng"]);
            if let Some(path) = &options.tessdata {
                cmd.arg("--tessdata-dir").arg(path);
            }
            cmd.args(["-c", "thresholding_method=2", "-c", "tessedit_do_invert=0"]);
            let text = run(&mut cmd, dir.path(), control)?;
            frames.push(FrameText {
                offset_s: offset,
                view: view.into(),
                clock_text: clocks(&text),
                text,
            });
        }
    }
    progress("Matching visible names against local GeoNames data", 0.9);
    let places = match_places(
        BufReader::new(File::open(&options.gazetteer)?),
        &frames,
        options.bounds,
        control,
    )?;
    progress("Video clues ready", 1.);
    Ok(Report {
        method: "video_text".into(), status: if places.is_empty() { "no_place_clues" } else { "place_clues_found" }.into(),
        offline: true, media_sha256, gazetteer_sha256, gazetteer_source, ocr_version, bounds: options.bounds, frames, places,
        warnings: vec!["Coordinates refer to named places. Review the visible text and the map before treating a place as the recording location.".into()],
    })
}

pub fn write_report(report: &Report, output: &Path, inputs: &[&Path]) -> Result<()> {
    if output == Path::new("-") {
        serde_json::to_writer_pretty(std::io::stdout().lock(), report)?;
        println!();
    } else {
        if let Ok(target) = output.canonicalize()
            && inputs
                .iter()
                .any(|p| p.canonicalize().is_ok_and(|p| p == target))
        {
            bail!("The report cannot overwrite an input file");
        }
        let (mut temp, destination) = crate::atomic_file::prepare(output)?;
        serde_json::to_writer_pretty(&mut temp, report)?;
        temp.persist(destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_span_uses_packet_timestamps_instead_of_container_duration() {
        let packets = b"pts_time=20516298.766667|duration_time=0.033333\npts_time=N/A\npts_time=20516293.800000|duration_time=0.033333\n";
        let (start, duration, count) = video_span(&packets[..], &Control::default()).unwrap();
        assert_eq!(start, 20516293.8);
        assert!((duration - 5.).abs() < 1e-7);
        assert_eq!(count, 2);
        assert_eq!(
            video_span(&b"pts_time=12|duration_time=N/A\n"[..], &Control::default()).unwrap(),
            (12., 0., 1)
        );
        assert!(video_span(&b"pts_time=N/A\n"[..], &Control::default()).is_err());
    }
    fn row(id: u64, name: &str, aliases: &str, lat: f64) -> String {
        format!(
            "{id}\t{name}\t{name}\t{aliases}\t{lat}\t-1.0\tS\tAIRP\tGB\t\t\t\t\t\t0\t0\t0\tEurope/London\t2026-01-01\n"
        )
    }
    #[test]
    fn preserves_ambiguous_places_and_matches_whole_phrases() {
        let data = row(1, "Test Airport", "Test Field", 50.)
            + &row(2, "Test Airport", "", 51.)
            + &row(3, "Airport", "", 50.)
            + &row(4, "Example Village", "To 1", 50.);
        let frames = vec![FrameText {
            offset_s: 0.,
            view: "full".into(),
            text: "LIVE | TEST AIRPORT TO 1 2026-11-06 11:59:46 AKST".into(),
            clock_text: vec![],
        }];
        let places = match_places(data.as_bytes(), &frames, None, &Control::default()).unwrap();
        assert_eq!(
            places.iter().map(|p| p.geoname_id).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(places[0].frame_indices, [0]);
        assert!(
            match_places(
                data.as_bytes(),
                &[FrameText {
                    text: "Contest airport\nTest\nAirport".into(),
                    ..frames[0].clone()
                }],
                None,
                &Control::default()
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(clocks(&frames[0].text), ["11:59:46", "2026-11-06"]);
        assert!(clocks("2026-99-99 25:61:61").is_empty());
    }
    #[test]
    fn aliases_bounds_and_bad_data() {
        let frame = FrameText {
            offset_s: 1.,
            view: "top".into(),
            text: "Test Field".into(),
            clock_text: vec![],
        };
        let data =
            row(1, "Test Airport", "Test Field", 50.) + &row(2, "Test Airport", "Test Field", 51.);
        let bounds = Bounds {
            south: 49.,
            north: 50.5,
            west: -2.,
            east: 0.,
        };
        let places = match_places(
            data.as_bytes(),
            std::slice::from_ref(&frame),
            Some(bounds),
            &Control::default(),
        )
        .unwrap();
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].matched_name, "Test Field");
        assert!(
            match_places(
                "bad".as_bytes(),
                std::slice::from_ref(&frame),
                None,
                &Control::default()
            )
            .is_err()
        );
        let cancelled = Control::default();
        cancelled.stop();
        assert!(
            match_places(
                data.as_bytes(),
                std::slice::from_ref(&frame),
                None,
                &cancelled
            )
            .is_err()
        );
        let data = row(1, "Test-Field-Airport", "Test Field", 50.);
        let frame = FrameText {
            text: "Test Field Airport".into(),
            ..frame
        };
        let places = match_places(data.as_bytes(), &[frame], None, &Control::default()).unwrap();
        assert_eq!(places[0].matched_name, "Test-Field-Airport");
    }
    #[test]
    fn rejects_remote_media_before_starting_a_tool() {
        let options = Options {
            media: "https://example.com/video.mp4".into(),
            ..Options::default()
        };
        assert!(
            analyze(&options, &Control::default(), &|_, _| {})
                .unwrap_err()
                .to_string()
                .contains("local video")
        );
    }

    #[test]
    fn report_writer_preserves_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("recording.mp4");
        std::fs::write(&input, "original recording").unwrap();
        let report = Report {
            method: "video_text".into(),
            status: "no_place_clues".into(),
            offline: true,
            media_sha256: String::new(),
            gazetteer_sha256: String::new(),
            gazetteer_source: None,
            ocr_version: String::new(),
            bounds: None,
            frames: vec![],
            places: vec![],
            warnings: vec![],
        };
        assert!(write_report(&report, &input, &[&input]).is_err());
        assert_eq!(
            std::fs::read_to_string(&input).unwrap(),
            "original recording"
        );
        let output = dir.path().join("report.json");
        write_report(&report, &output, &[&input]).unwrap();
        let decoded: Report = serde_json::from_reader(File::open(output).unwrap()).unwrap();
        assert!(decoded.offline);
        assert_eq!(decoded.status, "no_place_clues");
    }
}
