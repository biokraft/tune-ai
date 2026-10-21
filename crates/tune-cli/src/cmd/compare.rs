//! `tune compare`: our song or passage against a reference track.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_analysis::spectrogram::compare_png;
use tune_analysis::{Analysis, BANDS, Comparison, analyze};

use crate::ears::{Input, load_input, png_path, title, write_png};
use crate::refs::find_refs;
use crate::report::{Failure, Success, finish};

pub struct Args {
    pub ours: PathBuf,
    pub reference: Option<PathBuf>,
    pub ref_name: Option<String>,
    pub bars: Option<String>,
    pub ref_seconds: Option<String>,
    /// `Some("")` is a bare `--png`.
    pub png: Option<String>,
    pub json: bool,
}

fn fail(message: impl Into<String>) -> Vec<Failure> {
    vec![Failure::msg(message)]
}

/// Parses `A..B` seconds (decimals allowed) and checks it against `len`.
fn parse_seconds(s: &str, len: f64) -> Result<(f64, f64), String> {
    let bad = || format!("bad --ref-seconds `{s}`: expected A..B in seconds, e.g. 90..150.5");
    let (a, b) = s.split_once("..").ok_or_else(bad)?;
    let a: f64 = a.trim().parse().map_err(|_| bad())?;
    let b: f64 = b.trim().parse().map_err(|_| bad())?;
    if !(a.is_finite() && b.is_finite()) {
        return Err(bad());
    }
    if !(a >= 0.0 && a < b && b <= len) {
        return Err(format!(
            "bad --ref-seconds `{s}`: the reference is {len:.1} s long"
        ));
    }
    Ok((a, b))
}

fn cell(x: Option<f64>, prec: usize) -> String {
    x.map_or("n/a".to_string(), |x| format!("{x:.prec$}"))
}

fn dcell(x: Option<f64>, prec: usize) -> String {
    x.map_or("n/a".to_string(), |x| format!("{x:+.prec$}"))
}

fn row(s: &mut String, name: &str, ours: String, theirs: String, delta: String, unit: &str) {
    let _ = write!(s, "\n{name:<13}{ours:>8}{theirs:>8}{delta:>8} {unit}");
}

fn text(
    ours: &Input,
    oa: &Analysis,
    reference: &Input,
    ra: &Analysis,
    c: &Comparison,
    png: &str,
) -> String {
    let mut s = format!(
        "ours {}\nref  {}\n{:<13}{:>8}{:>8}{:>8}",
        title(ours, oa),
        title(reference, ra),
        "",
        "ours",
        "ref",
        "delta"
    );
    let d = &c.deltas;
    row(
        &mut s,
        "loudness",
        cell(oa.loudness.integrated_lufs, 1),
        cell(ra.loudness.integrated_lufs, 1),
        dcell(d.integrated_lu, 1),
        "LUFS",
    );
    row(
        &mut s,
        "true peak",
        cell(Some(oa.loudness.true_peak_dbtp), 1),
        cell(Some(ra.loudness.true_peak_dbtp), 1),
        dcell(Some(d.true_peak_db), 1),
        "dBTP",
    );
    for (i, b) in BANDS.iter().enumerate() {
        row(
            &mut s,
            b.label,
            cell(oa.spectral.bands[i], 1),
            cell(ra.spectral.bands[i], 1),
            dcell(d.bands[i], 1),
            "dB re total",
        );
    }
    row(
        &mut s,
        "kick/bass",
        cell(oa.spectral.low_end.ratio_db, 1),
        cell(ra.spectral.low_end.ratio_db, 1),
        dcell(d.low_end_ratio_db, 1),
        "dB",
    );
    row(
        &mut s,
        "side/mid",
        cell(Some(oa.spectral.stereo.side_to_mid_db), 1),
        cell(Some(ra.spectral.stereo.side_to_mid_db), 1),
        dcell(d.side_to_mid_db, 1),
        "dB",
    );
    row(
        &mut s,
        "brightness",
        cell(oa.spectral.brightness.median_hz, 0),
        cell(ra.spectral.brightness.median_hz, 0),
        dcell(d.centroid_median_oct, 2),
        "Hz median (delta in oct)",
    );
    for v in &c.verdicts {
        let _ = write!(s, "\n- {v}");
    }
    let _ = write!(s, "\npng {png}");
    s
}

fn load_reference(a: &Args) -> Result<(Input, Option<String>), Vec<Failure>> {
    let (path, name) = match (&a.reference, &a.ref_name) {
        (Some(p), None) => (p.clone(), None),
        (None, Some(n)) => {
            let cwd = std::env::current_dir().map_err(|e| fail(e.to_string()))?;
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let refs = find_refs(&cwd, home.as_deref()).map_err(fail)?;
            (refs.resolve(n).map_err(fail)?, Some(n.clone()))
        }
        _ => return Err(fail("give a reference path or --ref NAME, not both")),
    };
    let mut input = load_input(&path, None, None, None)?;
    if let Some(n) = &name {
        input.stem.clone_from(n);
    }
    if let Some(r) = &a.ref_seconds {
        let sr = f64::from(input.audio.sample_rate);
        let (lo, hi) = parse_seconds(r, input.audio.seconds()).map_err(fail)?;
        let frames = input.audio.frames();
        let lo_f = ((lo * sr).round() as usize).min(frames);
        let hi_f = ((hi * sr).round() as usize).min(frames);
        input.audio = input.audio.slice(lo_f..hi_f);
        input.source["seconds"] = json!(input.audio.seconds());
        input.source["seconds_range"] = json!([lo, hi]);
    }
    if input.source.get("seconds_range").is_none() {
        input.source["seconds_range"] = Value::Null;
    }
    input.source["ref_name"] = name.clone().map_or(Value::Null, Value::String);
    Ok((input, name))
}

fn run_inner(a: &Args) -> Result<Success, Vec<Failure>> {
    let ours = load_input(&a.ours, a.bars.as_deref(), None, None)?;
    let (reference, _) = load_reference(a)?;
    let err = |e: tune_analysis::AnalysisError| fail(e.to_string());
    let oa = analyze(&ours.audio, None).map_err(err)?;
    let ra = analyze(&reference.audio, None).map_err(err)?;
    let cmp = tune_analysis::compare(&oa, &ra);

    let default_name = format!("compare-{}-vs-{}", ours.png_name, reference.stem);
    let path = png_path(a.png.as_deref(), &default_name);
    let bytes = compare_png(
        (&ours.audio, &title(&ours, &oa), ours.map.as_ref()),
        (&reference.audio, &title(&reference, &ra)),
    );
    write_png(&path, &bytes).map_err(|f| vec![f])?;
    let png = path.display().to_string();

    let mut json = serde_json::Map::new();
    json.insert("ours".into(), merged(&ours, &oa));
    json.insert("reference".into(), merged(&reference, &ra));
    if let Value::Object(rest) = cmp.to_json() {
        json.extend(rest);
    }
    json.insert("png".into(), Value::String(png.clone()));
    Ok(Success {
        text: text(&ours, &oa, &reference, &ra, &cmp, &png),
        json: Value::Object(json),
    })
}

fn merged(input: &Input, a: &Analysis) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("source".into(), input.source.clone());
    if let Value::Object(rest) = a.summary_json() {
        m.extend(rest);
    }
    Value::Object(m)
}

pub fn run(a: &Args) -> ExitCode {
    finish(a.json, run_inner(a))
}
