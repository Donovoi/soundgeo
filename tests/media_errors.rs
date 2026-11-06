use soundgeo::{Control, Job, audio};
#[test]
fn cli_does_not_overwrite_the_input_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("job.json");
    let original = b"{}";
    std::fs::write(&path, original).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_soundgeo"))
        .arg("analyze")
        .arg("--config")
        .arg(&path)
        .arg("--output")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot overwrite"));
}
#[test]
fn missing_decoder_short_recording_and_silent_video_fail_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("short.wav");
    let mut writer = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 4000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..4000 * 3 {
        writer.write_sample(0i16).unwrap();
    }
    writer.finalize().unwrap();
    let mut job = Job {
        media: wav,
        ffmpeg: Some(dir.path().join("missing-ffmpeg")),
        ..Default::default()
    };
    assert!(
        audio::decode(&job, &Control::default())
            .unwrap_err()
            .to_string()
            .contains("FFmpeg could not start")
    );
    job.ffmpeg = None;
    assert!(
        audio::decode(&job, &Control::default())
            .unwrap_err()
            .to_string()
            .contains("at least 20 seconds")
    );
    let video = dir.path().join("no-audio.mp4");
    let status = audio::quiet_command(audio::ffmpeg_path(&job))
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=s=32x32:r=1",
            "-t",
            "2",
            "-c:v",
            "libx264",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(status.success());
    job.media = video;
    assert!(
        audio::decode(&job, &Control::default())
            .unwrap_err()
            .to_string()
            .contains("audio stream")
    );
    let control = Control::default();
    control.stop();
    assert!(
        audio::decode(&job, &control)
            .unwrap_err()
            .to_string()
            .contains("Cancelled")
    );
}
