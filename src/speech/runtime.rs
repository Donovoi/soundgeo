use crate::{Control, Progress, audio::quiet_command, subprocess};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(crate) const RATE: u32 = 16_000;
const MAX_SECONDS: u32 = 7200;

pub(crate) struct Decoded {
    pub samples: Vec<i16>,
    pub decoder_version: String,
}

pub(crate) fn tool(family: &str, name: &str) -> PathBuf {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let local = parent.join("speech-tools").join(family).join(&filename);
        if local.is_file() {
            return local;
        }
    }
    PathBuf::from(filename)
}

pub(crate) fn command(family: &str, name: &str) -> Result<(Command, PathBuf)> {
    let candidate = tool(family, name);
    let path = if candidate.is_file() {
        candidate.canonicalize()?
    } else {
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|directory| directory.join(&candidate))
                    .find(|p| p.is_file())
            })
            .with_context(|| {
                format!("Missing bundled speech helper {name}. Use the complete SoundGeo release.")
            })?
            .canonicalize()?
    };
    let mut command = quiet_command(&path);
    // Each native runtime has its own ggml libraries. Mixing their identically
    // named DLLs/shared objects can load a different ABI or CPU backend.
    #[cfg(unix)]
    command.env(
        "LD_LIBRARY_PATH",
        path.parent().context("Missing helper directory")?,
    );
    Ok((command, path))
}

pub(crate) fn capture(
    command: &mut Command,
    directory: &Path,
    label: &str,
    control: &Control,
    progress: &Progress<'_>,
    timeout: Duration,
    monitor: impl Fn() -> Result<()>,
) -> Result<String> {
    let logs = tempfile::tempdir_in(directory)?;
    let stdout = logs.path().join("stdout.txt");
    let stderr = logs.path().join("stderr.txt");
    let start = Instant::now();
    let last = AtomicU64::new(u64::MAX);
    progress(label, 0.);
    let status = subprocess::run(command, &stdout, &stderr, control, timeout, || {
        let seconds = start.elapsed().as_secs();
        if last.swap(seconds, Ordering::Relaxed) != seconds {
            progress(&format!("{label} · {seconds}s elapsed"), 0.);
        }
        monitor()
    })?;
    ensure!(
        status.success(),
        "{label} failed ({status}): {}",
        subprocess::tail(&stderr)
    );
    control.check()?;
    std::fs::read_to_string(stdout).context("The speech helper returned invalid UTF-8")
}

pub(crate) fn decode(
    media: &Path,
    directory: &Path,
    ffmpeg: &Path,
    control: &Control,
    progress: &Progress<'_>,
) -> Result<Decoded> {
    ensure!(
        media.is_file(),
        "Choose an existing recording before enabling speech analysis"
    );
    let version = capture(
        quiet_command(ffmpeg).arg("-version"),
        directory,
        "Checking the audio decoder",
        control,
        progress,
        Duration::from_secs(30),
        || Ok(()),
    )?
    .lines()
    .next()
    .unwrap_or("unknown FFmpeg version")
    .to_owned();
    let decoded = directory.join("speech-16k.wav");
    ensure!(
        !decoded.exists(),
        "The speech decode destination already exists"
    );
    let mut command = quiet_command(ffmpeg);
    // Preserve the input presentation timeline, including an audio stream that
    // starts after the video or has timestamp gaps. Padding is silence; aircraft
    // analysis retains its separate, existing decoder and sample rate.
    command
        .args(["-nostdin", "-v", "error", "-copyts", "-start_at_zero", "-i"])
        .arg(media)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-af",
            "aresample=16000:async=1:first_pts=0",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            "-threads",
            "2",
            "-t",
        ])
        .arg((MAX_SECONDS + 1).to_string())
        .arg(&decoded);
    capture(
        &mut command,
        directory,
        "Decoding speech audio and preserving media timestamps",
        control,
        progress,
        Duration::from_secs(900),
        || {
            ensure!(
                decoded.metadata().is_ok_and(
                    |m| m.len() <= u64::from(MAX_SECONDS + 2) * u64::from(RATE) * 2 + 8192
                ) || !decoded.exists(),
                "Decoded speech audio exceeded its size limit"
            );
            Ok(())
        },
    )?;
    let mut reader =
        hound::WavReader::open(&decoded).context("Could not read decoded speech audio")?;
    let spec = reader.spec();
    ensure!(
        spec.sample_rate == RATE
            && spec.channels == 1
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "Unexpected speech decoder format"
    );
    let count = reader.duration();
    ensure!(count > 0, "The recording has no decoded audio");
    ensure!(
        count <= RATE * MAX_SECONDS,
        "Speech analysis supports recordings up to two hours. Split this recording into shorter clips."
    );
    let mut samples = Vec::with_capacity(count as usize);
    for (index, sample) in reader.samples::<i16>().enumerate() {
        if index % 16_384 == 0 {
            control.check()?;
        }
        samples.push(sample?);
    }
    Ok(Decoded {
        samples,
        decoder_version: version,
    })
}

pub(crate) fn write_wav(path: &Path, samples: &[i16], control: &Control) -> Result<()> {
    ensure!(!samples.is_empty(), "Cannot process an empty speech chunk");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut writer = hound::WavWriter::new(std::io::BufWriter::new(file), spec)?;
    for (index, sample) in samples.iter().enumerate() {
        if index % 16_384 == 0 {
            control.check()?;
        }
        writer.write_sample(*sample)?;
    }
    writer.finalize()?;
    Ok(())
}

pub(crate) fn read_text(path: &Path) -> Result<String> {
    ensure!(
        path.metadata()?.len() <= 4 * 1024 * 1024,
        "Speech helper output exceeded 4 MiB"
    );
    std::fs::read_to_string(path).context("Invalid UTF-8 in speech helper output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_video_audio_keeps_its_original_media_offset() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("delayed.mkv");
        let ffmpeg = crate::audio::ffmpeg_path(&crate::Job::default());
        let mut command = quiet_command(&ffmpeg);
        command
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=16x16:r=1:d=4",
                "-itsoffset",
                "1.25",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=16000:duration=1",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "ffv1",
                "-c:a",
                "pcm_s16le",
                "-threads",
                "1",
            ])
            .arg(&input);
        let control = Control::default().with_offline(true);
        capture(
            &mut command,
            directory.path(),
            "Creating delayed audio fixture",
            &control,
            &|_, _| {},
            Duration::from_secs(30),
            || Ok(()),
        )
        .unwrap();
        let result = decode(&input, directory.path(), &ffmpeg, &control, &|_, _| {}).unwrap();
        assert!((result.samples.len() as f64 / f64::from(RATE) - 2.25).abs() < 0.05);
        assert!(
            result.samples[..RATE as usize]
                .iter()
                .all(|sample| sample.abs() < 2)
        );
        assert!(
            result.samples[22000..30000]
                .iter()
                .map(|s| i64::from(*s).abs())
                .sum::<i64>()
                > 1_000_000
        );
    }
}
