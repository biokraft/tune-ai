//! `tune render`: a song (or a bar range of it) to WAV, optionally as stems.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use serde_json::json;
use tune_engine::{CLIP_CEILING, CompiledSong, RenderOptions, render_to_files};
use tune_song::MasterMode;

use super::{load, sample_rate};
use crate::report::{Failure, Success, dbfs, finish, fmt_time, parse_bar_range};

/// Limiter reduction (dB) above which the render warns.
const LIMITER_WARN_DB: f64 = 9.0;

pub struct Args {
    pub song: PathBuf,
    pub bars: Option<String>,
    pub stems: bool,
    pub out: PathBuf,
    pub json: bool,
}

fn has_wav_ext(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

fn check_out(out: &Path, stems: bool) -> Result<(), String> {
    match (stems, has_wav_ext(out)) {
        (false, false) => Err(format!(
            "--out `{}` must end in .wav (use --stems to write a directory)",
            out.display()
        )),
        (true, true) => Err(format!(
            "--stems writes a directory; `{}` looks like a .wav file",
            out.display()
        )),
        _ => Ok(()),
    }
}

fn run_inner(a: &Args) -> Result<Success, Vec<Failure>> {
    let song = load(&a.song)?;
    let total = song.total_bars();
    let range = match &a.bars {
        Some(s) => parse_bar_range(s, total).map_err(|m| vec![Failure::msg(m)])?,
        None => 0..total,
    };
    check_out(&a.out, a.stems).map_err(|m| vec![Failure::msg(m)])?;
    let compiled = Arc::new(CompiledSong::new(&song, sample_rate(&song)));
    let compiled_master = compiled.mix.master;
    let opts = RenderOptions {
        start_bar: range.start,
        end_bar: range.end,
        stems: a.stems,
    };
    let report =
        render_to_files(compiled, &opts, &a.out).map_err(|e| vec![Failure::msg(e.to_string())])?;
    let peak = dbfs(report.meter.peak);
    let pre = dbfs(report.meter.pre_clip_peak);
    let limit = compiled_master == MasterMode::Limit;
    let reduction = report.meter.max_reduction_db;
    let tail = if limit {
        format!("limiter reduced peaks by {reduction:.1} dB")
    } else {
        format!("pre-clip peak {pre:.1} dBFS")
    };
    let text = format!(
        "rendered bars {}..{} ({}) to {}, peak {peak:.1} dBFS, {tail}",
        range.start + 1,
        range.end,
        fmt_time(report.seconds),
        a.out.display()
    );
    if !a.json {
        // stderr: the stdout line stays one stable line.
        if limit {
            if reduction > LIMITER_WARN_DB {
                eprintln!(
                    "warning: limiter reduced peaks by {reduction:.1} dB: lower mix.master.gain"
                );
            } else if reduction > 0.0 {
                eprintln!("note: limiter reduced peaks by up to {reduction:.1} dB");
            }
        } else if report.meter.knee_samples > 0 {
            if report.meter.pre_clip_peak > CLIP_CEILING {
                eprintln!(
                    "warning: master clipper engaged on {} samples",
                    report.meter.knee_samples
                );
            } else {
                eprintln!(
                    "note: soft clipper shaped {} samples",
                    report.meter.knee_samples
                );
            }
        }
    }
    let files: Vec<String> = report
        .files
        .iter()
        .map(|f| f.display().to_string())
        .collect();
    Ok(Success {
        text,
        json: json!({
            "files": files,
            "frames": report.frames,
            "seconds": report.seconds,
            "peak_dbfs": peak,
            "pre_clip_peak_dbfs": pre,
            "knee_samples": report.meter.knee_samples,
            "master": compiled_master.name(),
            "max_reduction_db": (reduction * 100.0).round() / 100.0,
        }),
    })
}

pub fn run(a: &Args) -> ExitCode {
    finish(a.json, run_inner(a))
}
