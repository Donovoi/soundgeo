//! Conditional sensitivity diagnostic, not a location search or confidence interval.
//! Takes only a report and its reference data; no ground-truth position or clock.
use anyhow::{Context, Result, ensure};
use serde_json::json;
use soundgeo::{Control, Report, audio::Event, geo::Point, model::Track};

fn linear_rmse(points: &[(f64, f64)]) -> f64 {
    let (mut n, mut mx, mut my, mut xx, mut xy, mut yy) = (0., 0., 0., 0., 0., 0.);
    for &(x, y) in points {
        n += 1.;
        let dx = x - mx;
        let dy = y - my;
        mx += dx / n;
        my += dy / n;
        xx += dx * (x - mx);
        xy += dx * (y - my);
        yy += dy * (y - my);
    }
    if n < 3. || xx <= 0. {
        return 0.;
    }
    ((yy - xy * xy / xx).max(0.) / n).sqrt()
}

fn evaluate(
    pairs: &[(&Event, &Track)],
    point: Point,
    start: f64,
    rate: f64,
    c: f64,
) -> (f64, bool) {
    let observer = point.ecef();
    let mut total = 0.;
    let mut passes = true;
    for &(event, track) in pairs {
        let mut modeled = Vec::with_capacity(event.ridge.len());
        let (mut ff, mut yf) = (0., 0.);
        let mut used = Vec::with_capacity(event.ridge.len());
        for p in &event.ridge {
            if let Some(f) = track.factor(observer, start + rate * p.time_s, c) {
                ff += f * f;
                yf += p.hz * f;
                modeled.push(f);
                used.push((p.time_s, p.hz));
            }
        }
        let coverage = used.len() as f64 / event.ridge.len() as f64;
        if coverage < 0.9 || ff <= 0. {
            return (f64::INFINITY, false);
        }
        let tone = yf / ff;
        let mse = used
            .iter()
            .zip(modeled)
            .map(|((_, hz), f)| (hz - tone * f).powi(2))
            .sum::<f64>()
            / used.len() as f64;
        let linear = linear_rmse(&used);
        let relative = mse.sqrt() / tone;
        passes &= relative <= 0.015 && linear > f64::EPSILON && mse <= 0.5 * linear.powi(2);
        total += (100. * relative).powi(2) + (1. - coverage).powi(2);
    }
    let score = (total / pairs.len() as f64).sqrt();
    (score, passes && score <= 1.)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let input = args
        .get(1)
        .context("Expected report JSON and output JSON")?;
    let output = args.get(2).context("Expected output JSON")?;
    let report: Report = serde_json::from_reader(std::fs::File::open(input)?)?;
    let candidate = report
        .candidates
        .first()
        .context("Report has no candidate")?;
    ensure!(
        report.job.start_utc.is_none(),
        "This diagnostic requires an inferred recording clock"
    );
    let dataset = soundgeo::data::load_for_job_sources(
        &report.job,
        &report.reference_sources,
        report.duration_s,
        &Control::default(),
    )?;
    let pairs: Vec<_> = candidate
        .matches
        .iter()
        .map(|m| -> Result<_> {
            let event = report
                .events
                .get(m.event_index.context("Missing event index")?)
                .context("Unknown event")?
                .variant(m.event_variant)
                .context("Unknown observation variant")?;
            let track = dataset
                .tracks
                .iter()
                .find(|t| t.id == m.aircraft)
                .context("Reference flight missing")?;
            Ok((event, track))
        })
        .collect::<Result<_>>()?;
    ensure!(!pairs.is_empty(), "No matched curves");
    let center = Point {
        lat: candidate.latitude,
        lon: candidate.longitude,
        alt: candidate.ground_m,
    };
    let c = report.job.sound_speed();
    let baseline = evaluate(
        &pairs,
        center,
        candidate.recording_start_unix,
        candidate.recording_clock_rate,
        c,
    );
    ensure!(
        (baseline.0 - candidate.relative_error_percent).abs() < 1e-6 && baseline.1,
        "Diagnostic must reproduce original fit: {} versus {}",
        baseline.0,
        candidate.relative_error_percent
    );
    let pivot = pairs.iter().map(|(e, _)| e.cpa_s).sum::<f64>() / pairs.len() as f64;
    let drift = report.job.max_clock_drift_ppm * 1e-6;
    let mut terrain = soundgeo::geo::Terrain::with_datum(
        report.job.terrain.clone(),
        report.job.ground_m,
        report.job.ground_datum,
    );
    let mut results = Vec::new();
    for distance in [0., 100., 250., 500., 1000., 2000.] {
        for bearing in (0..360).step_by(45) {
            if distance == 0. && bearing != 0 {
                continue;
            }
            let angle = (bearing as f64).to_radians();
            let mut point = Point {
                lat: center.lat + distance * angle.cos() / 111320.,
                lon: center.lon
                    + distance * angle.sin() / (111320. * center.lat.to_radians().cos()),
                alt: 0.,
            };
            if report.job.bounds.is_some_and(|b| !b.contains(point)) {
                continue;
            }
            point.alt = terrain.elevation(point);
            let mut best = (
                f64::INFINITY,
                candidate.recording_start_unix,
                candidate.recording_clock_rate,
                false,
            );
            let mut supported: Option<(f64, f64, f64)> = None;
            let mut remember = |error: f64, pass: bool, start: f64, rate: f64| {
                if pass && supported.is_none_or(|previous| error < previous.0) {
                    supported = Some((error, start, rate));
                }
            };
            for offset in [-5., 0., 5.] {
                for rate_offset in [-drift * 0.5, 0., drift * 0.5] {
                    let mut rate = (candidate.recording_clock_rate + rate_offset)
                        .clamp(1. - drift, 1. + drift);
                    let mut start = candidate.recording_start_unix + offset
                        - (rate - candidate.recording_clock_rate) * pivot;
                    let (mut score, mut passing) = evaluate(&pairs, point, start, rate, c);
                    remember(score, passing, start, rate);
                    let (mut seconds, mut rate_step) = (4., drift * 0.5);
                    for iteration in 0..40 {
                        let mut improved = false;
                        for axis in 0..2 {
                            for sign in [-1., 1.] {
                                let next_rate = if axis == 0 {
                                    rate
                                } else {
                                    rate + sign * rate_step
                                };
                                let next_start = if axis == 0 {
                                    start + sign * seconds
                                } else {
                                    start - sign * rate_step * pivot
                                };
                                if (next_rate - 1.).abs() > drift + 1e-12 {
                                    continue;
                                }
                                let (error, pass) =
                                    evaluate(&pairs, point, next_start, next_rate, c);
                                remember(error, pass, next_start, next_rate);
                                if error < score {
                                    (score, start, rate, passing) =
                                        (error, next_start, next_rate, pass);
                                    improved = true;
                                }
                            }
                        }
                        if !improved || iteration % 5 == 4 {
                            seconds *= 0.5;
                            rate_step *= 0.5;
                        }
                        if seconds < 0.01 && rate_step < 1e-6 {
                            break;
                        }
                    }
                    if score < best.0 {
                        best = (score, start, rate, passing);
                    }
                }
            }
            let selected = supported.unwrap_or((best.0, best.1, best.2));
            results.push(json!({"offset_m":distance,"bearing_degrees":bearing,
                "latitude":point.lat,"longitude":point.lon,"error_percent":selected.0.is_finite().then_some(selected.0),
                "recording_start_unix":selected.1,"clock_rate":selected.2,"all_original_matches_pass":supported.is_some(),
                "minimum_error_percent":best.0.is_finite().then_some(best.0),"minimum_error_matches_pass":best.3}));
            eprintln!(
                "{distance:.0} m, {bearing} degrees: {:.4}%, passes {}",
                selected.0,
                supported.is_some()
            );
        }
    }
    let result = json!({"not_location_acceptance":true,"conditional_on_app_selected_matches":true,
        "ground_truth_supplied":false,"external_recording_clock_supplied":false,"inferred_clock_from_report_used":true,
        "not_a_blind_search":true,"recomputed_original_score":baseline.0,
        "original_report_score":candidate.relative_error_percent,"score_tolerance_percent":1e-6,
        "matched_groups":pairs.len(),"source_report":std::path::Path::new(input).to_string_lossy(),"points":results,
        "limitations":"Sparse local sampling with fixed associations and observation variants. This can demonstrate alternative fits but cannot bound the complete feasible region or establish a confidence level."});
    serde_json::to_writer_pretty(std::fs::File::create(output)?, &result)?;
    Ok(())
}
