//! `tune analyze`: loudness, balance and brightness of a song or audio file.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_analysis::spectrogram::spectrogram_png;
use tune_analysis::{Analysis, BANDS, analyze};

use crate::ears::{Input, is_song, load_input, load_named_ref, png_path, title, write_png};
use crate::report::{Failure, Success, finish, fmt_time};

pub struct Args {
    /// Exactly one of `input` and `ref_name` is set.
    pub input: Option<PathBuf>,
    pub ref_name: Option<String>,
    pub bars: Option<String>,
    pub sections: bool,
    pub song: Option<PathBuf>,
    pub from_bar: Option<u32>,
    /// `Some("")` is a bare `--png`.
    pub png: Option<String>,
    pub json: bool,
}

fn v(x: Option<f64>, unit: &str) -> String {
    match x {
        Some(x) => format!("{x:.1}{unit}"),
        None => "n/a".to_string(),
    }
}

fn text(input: &Input, a: &Analysis, png: Option<&str>) -> String {
    let mut s = format!(
        "{}: {}, {} Hz, {}",
        input.stem,
        fmt_time(a.seconds),
        a.sample_rate,
        if a.channels == 1 { "mono" } else { "stereo" }
    );
    if input.map.is_some()
        && let Some(b) = input.source["bars"].as_array()
    {
        let _ = write!(s, ", bars {}-{}", b[0], b[1]);
    }
    let l = &a.loudness;
    let _ = write!(
        s,
        "\nloudness   {} LUFS integrated, short-term max {}, range {}; target -8..-6 LUFS: {}",
        l.integrated_lufs
            .map_or("silent".to_string(), |x| format!("{x:.1}")),
        v(l.short_term_max_lufs, " LUFS"),
        v(l.lra_lu, " LU"),
        a.targets.integrated
    );
    let _ = write!(
        s,
        "\npeaks      sample {:.1} dBFS, true {:.1} dBTP ({}, max -1), crest {}, PLR {}",
        l.sample_peak_dbfs,
        l.true_peak_dbtp,
        a.targets.true_peak,
        v(l.crest_db, " dB"),
        v(l.plr_db, " dB")
    );
    let bands: Vec<String> = BANDS
        .iter()
        .zip(&a.spectral.bands)
        .map(|(b, x)| format!("{} {}", b.key, v(*x, "")))
        .collect();
    let _ = write!(s, "\nbands      {} (dB re total)", bands.join(", "));
    let le = &a.spectral.low_end;
    let _ = write!(
        s,
        "\nlow end    kick region {} dB, bass region {} dB, kick minus bass {} dB",
        v(le.kick_region_db, ""),
        v(le.bass_region_db, ""),
        v(le.ratio_db, "")
    );
    let st = &a.spectral.stereo;
    let _ = write!(
        s,
        "\nstereo     correlation {}, side/mid {:.1} dB, low correlation {}",
        v(st.correlation, ""),
        st.side_to_mid_db,
        v(st.low_correlation, "")
    );
    let b = &a.spectral.brightness;
    let _ = write!(
        s,
        "\nbrightness median {} (p10 {}, p90 {}), range {} oct",
        v(b.median_hz, " Hz"),
        v(b.p10_hz, " Hz"),
        v(b.p90_hz, " Hz"),
        v(b.range_oct, "")
    );
    for sec in a.sections.iter().flatten() {
        let _ = write!(
            s,
            "\nsection {} bars {}-{}: {} LUFS",
            sec.name,
            sec.start_bar,
            sec.end_bar,
            sec.integrated_lufs
                .map_or("silent".to_string(), |x| format!("{x:.1}"))
        );
        if let Some(d) = sec.delta_lu {
            let _ = write!(s, " ({d:+.1} LU)");
        }
        let _ = write!(s, ", centroid {}", v(sec.centroid_median_hz, " Hz"));
        if sec.partial {
            s.push_str(", partial");
        }
        for n in &sec.notes {
            let _ = write!(s, "; {n}");
        }
    }
    if let Some(p) = png {
        let _ = write!(s, "\npng {p}");
    }
    s
}

fn run_inner(a: &Args) -> Result<Success, Vec<Failure>> {
    let mut input = match (&a.input, &a.ref_name) {
        (Some(path), None) => {
            if a.sections && a.song.is_none() && !is_song(path) {
                return Err(vec![Failure::msg(
                    "--sections needs a bar map: analyse the song, or pass --song with the WAV",
                )]);
            }
            load_input(path, a.bars.as_deref(), a.song.as_deref(), a.from_bar)?
        }
        (None, Some(name)) => load_named_ref(name)?,
        _ => {
            return Err(vec![Failure::msg(
                "give an input file or --ref NAME, not both",
            )]);
        }
    };
    input.source["ref_name"] = a.ref_name.clone().map_or(Value::Null, Value::String);
    let map = if a.sections { input.map.as_ref() } else { None };
    let analysis = analyze(&input.audio, map).map_err(|e| vec![Failure::msg(e.to_string())])?;
    let png = match &a.png {
        Some(arg) => {
            let path = png_path(Some(arg), &input.png_name);
            let bytes =
                spectrogram_png(&input.audio, &title(&input, &analysis), input.map.as_ref());
            write_png(&path, &bytes).map_err(|f| vec![f])?;
            Some(path.display().to_string())
        }
        None => None,
    };
    let mut json = serde_json::Map::new();
    json.insert("source".into(), input.source.clone());
    if let Value::Object(rest) = analysis.to_json() {
        json.extend(rest);
    }
    json.insert("png".into(), png.clone().map_or(Value::Null, Value::String));
    json.insert("notes".into(), json!(input.notes));
    if let Some(from) = input.migrated_from {
        json.insert("migrated_from".into(), Value::from(from));
    }
    if !a.json {
        // stderr, like `tune render`: stdout stays the report.
        for n in &input.notes {
            eprintln!("note: {n}");
        }
    }
    Ok(Success {
        text: text(&input, &analysis, png.as_deref()),
        json: Value::Object(json),
    })
}

pub fn run(a: &Args) -> ExitCode {
    finish(a.json, run_inner(a))
}
