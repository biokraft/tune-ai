use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde_json::json;
use tune_cli::cmd;
use tune_cli::render::{RenderSpec, Sweep, render_acid, write_wav};
use tune_cli::report::{Failure, Success, dbfs, finish};
use tune_dsp::DistortionMode;
use tune_dsp::acid303::{AcidPattern, PatternError};

#[derive(Parser)]
#[command(name = "tune", about = "tune-ai music workstation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a song file and summarise it.
    Validate(SongArgs),
    /// Show a song's sections and tracks.
    Info(SongArgs),
    /// List every instrument and effect parameter.
    Params(JsonArgs),
    /// Render an acid 303 pattern to a stereo 24-bit WAV.
    #[command(name = "render-303")]
    Render303(Render303),
}

#[derive(Args)]
struct JsonArgs {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SongArgs {
    /// Song file (TOML).
    song: PathBuf,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("source").required(true).multiple(true).args(["pattern", "demo"])))]
struct Render303 {
    /// Step pattern, e.g. "C2 C2! C3~ ."
    #[arg(long)]
    pattern: Option<String>,
    /// Built-in demo (explicit flags override its values).
    #[arg(long)]
    demo: bool,
    /// Tempo in BPM (default 145).
    #[arg(long)]
    bpm: Option<f64>,
    /// Bars of 4 beats (default 4, demo 8).
    #[arg(long)]
    bars: Option<u32>,
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    /// Plain parameter value, ID=VALUE (repeatable).
    #[arg(long = "set", value_name = "ID=VALUE")]
    set: Vec<String>,
    /// Normalized sweep over the render, ID=FROM:TO (repeatable).
    #[arg(long = "sweep", value_name = "ID=FROM:TO")]
    sweep: Vec<String>,
    /// Distortion mode: soft, hard or overdrive.
    #[arg(long, value_parser = parse_mode)]
    dist: Option<DistortionMode>,
    #[arg(long)]
    out: PathBuf,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

fn parse_mode(s: &str) -> Result<DistortionMode, String> {
    match s {
        "soft" => Ok(DistortionMode::Soft),
        "hard" => Ok(DistortionMode::Hard),
        "overdrive" => Ok(DistortionMode::Overdrive),
        _ => Err(format!("`{s}` is not soft, hard or overdrive")),
    }
}

const DEMO_PATTERN: &str = "C2 C2! C3~ C2 . D#2 C2~ G2! . C2 C3 C2~ A#1 C2! . F2~";

fn parse_set(s: &str) -> Result<(String, f64), String> {
    let (id, v) = s
        .split_once('=')
        .ok_or_else(|| format!("bad --set `{s}`: expected ID=VALUE"))?;
    let v = v
        .parse::<f64>()
        .map_err(|_| format!("bad --set `{s}`: `{v}` is not a number"))?;
    Ok((id.to_string(), v))
}

fn parse_sweep(s: &str) -> Result<Sweep, String> {
    let bad = || format!("bad --sweep `{s}`: expected ID=FROM:TO");
    let (id, range) = s.split_once('=').ok_or_else(bad)?;
    let (from, to) = range.split_once(':').ok_or_else(bad)?;
    let num = |x: &str| {
        x.parse::<f64>()
            .map_err(|_| format!("bad --sweep `{s}`: `{x}` is not a number"))
    };
    Ok(Sweep {
        id: id.to_string(),
        from_norm: num(from)?,
        to_norm: num(to)?,
    })
}

fn build_spec(a: &Render303) -> Result<RenderSpec, String> {
    let mut params: Vec<(String, f64)> = Vec::new();
    let mut sweeps: Vec<Sweep> = Vec::new();
    let mut distortion = None;
    let mut bpm = 145.0;
    let mut bars = 4;
    let mut pattern = a.pattern.as_deref();
    if a.demo {
        pattern = pattern.or(Some(DEMO_PATTERN));
        bars = 8;
        // No volume override: with the default volume (-12 dB) the demo peaks at
        // -4.1 dBFS, within the -1 dBFS ceiling (acid303.volume=0 gives -1.9).
        params.extend(
            [
                ("acid303.resonance", 85.0),
                ("acid303.env_mod", 60.0),
                ("acid303.decay", 400.0),
                ("acid303.accent", 55.0),
                ("dist.drive", 12.0),
            ]
            .map(|(k, v)| (k.to_string(), v)),
        );
        sweeps.push(Sweep {
            id: "acid303.cutoff".into(),
            from_norm: 0.15,
            to_norm: 0.85,
        });
        distortion = Some(DistortionMode::Soft);
    }
    if let Some(b) = a.bpm {
        bpm = b;
    }
    if let Some(b) = a.bars {
        bars = b;
    }
    if a.dist.is_some() {
        distortion = a.dist;
    }
    for s in &a.set {
        let (id, v) = parse_set(s)?;
        params.retain(|(k, _)| *k != id);
        sweeps.retain(|w| w.id != id);
        params.push((id, v));
    }
    for s in &a.sweep {
        let w = parse_sweep(s)?;
        params.retain(|(k, _)| *k != w.id);
        sweeps.retain(|x| x.id != w.id);
        sweeps.push(w);
    }
    let pattern = AcidPattern::parse(pattern.unwrap_or_default()).map_err(|e| match e {
        PatternError::BadToken { .. } => format!("bad token in pattern, {e}"),
        _ => format!("bad pattern: {e}"),
    })?;
    Ok(RenderSpec {
        pattern,
        bpm,
        bars,
        sample_rate: a.sample_rate,
        params,
        sweeps,
        distortion,
    })
}

fn run(a: &Render303) -> Result<Success, Vec<Failure>> {
    run_inner(a).map_err(|m| vec![Failure::msg(m)])
}

fn run_inner(a: &Render303) -> Result<Success, String> {
    let spec = build_spec(a)?;
    let samples = render_acid(&spec).map_err(|e| e.to_string())?;
    if spec.distortion.is_none() && spec.params.iter().any(|(id, _)| id.starts_with("dist.")) {
        eprintln!("note: distortion bypassed (no --dist); dist.* values ignored");
    }
    write_wav(&a.out, &samples, spec.sample_rate).map_err(|e| e.to_string())?;
    let peak = samples.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
    let secs = samples.len() as f64 / f64::from(spec.sample_rate);
    let peak_db = 20.0 * f64::from(peak).log10();
    let clipped = samples.iter().filter(|x| x.abs() > 1.0).count();
    let mut text = format!("rendered {secs:.2}s, peak {peak_db:.1} dBFS");
    if clipped > 0 {
        text.push_str(&format!("\nwarning: {clipped} samples clipped"));
    }
    Ok(Success {
        text,
        json: json!({
            "file": a.out.display().to_string(),
            "seconds": secs,
            "peak_dbfs": dbfs(f64::from(peak)),
            "clipped_samples": clipped,
        }),
    })
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate(a) => cmd::validate::run(&a.song, a.json),
        Command::Info(a) => cmd::info::run(&a.song, a.json),
        Command::Params(a) => cmd::params::run(a.json),
        Command::Render303(a) => finish(a.json, run(&a)),
    }
}
