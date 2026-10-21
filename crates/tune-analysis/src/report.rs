use serde_json::{Map, Value, json};

use crate::sections::{BarMap, SectionReport, analyze_sections};
use crate::spectral::{BANDS, measure_spectral};
use crate::{AnalysisError, Audio, Loudness, Spectral, measure_loudness};

/// Acid techno integrated-loudness window, LUFS.
pub const ACID_LUFS: (f64, f64) = (-8.0, -6.0);
pub const TRUE_PEAK_MAX_DBTP: f64 = -1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Targets {
    /// `silent`, `below`, `within` or `above`.
    pub integrated: &'static str,
    /// `ok` or `over`.
    pub true_peak: &'static str,
}

pub fn classify_targets(integrated_lufs: Option<f64>, true_peak_dbtp: f64) -> Targets {
    let integrated = match integrated_lufs {
        None => "silent",
        Some(l) if l < ACID_LUFS.0 => "below",
        Some(l) if l > ACID_LUFS.1 => "above",
        Some(_) => "within",
    };
    let true_peak = if true_peak_dbtp <= TRUE_PEAK_MAX_DBTP {
        "ok"
    } else {
        "over"
    };
    Targets {
        integrated,
        true_peak,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub sample_rate: u32,
    pub channels: u16,
    pub seconds: f64,
    pub loudness: Loudness,
    pub short_term_lufs: Vec<Option<f64>>,
    pub spectral: Spectral,
    pub targets: Targets,
    pub sections: Option<Vec<SectionReport>>,
}

pub fn analyze(audio: &Audio, sections: Option<&BarMap>) -> Result<Analysis, AnalysisError> {
    let (loudness, short_term_lufs) = measure_loudness(audio)?;
    let spectral = measure_spectral(audio);
    let targets = classify_targets(loudness.integrated_lufs, loudness.true_peak_dbtp);
    let sections = sections.map(|m| analyze_sections(audio, m)).transpose()?;
    Ok(Analysis {
        sample_rate: audio.sample_rate,
        channels: audio.channels.len() as u16,
        seconds: audio.seconds(),
        loudness,
        short_term_lufs,
        spectral,
        targets,
        sections,
    })
}

/// Round to 2 decimals (dB, LU, LUFS, octaves, correlation). Never `-0.0`.
pub(crate) fn r2(x: f64) -> f64 {
    round_to(x, 100.0)
}

/// Round to 1 decimal (Hz). Never `-0.0`.
pub(crate) fn r1(x: f64) -> f64 {
    round_to(x, 10.0)
}

fn round_to(x: f64, k: f64) -> f64 {
    let r = (x * k).round() / k;
    if r == 0.0 { 0.0 } else { r }
}

fn num(x: Option<f64>, round: fn(f64) -> f64) -> Value {
    match x {
        Some(v) if v.is_finite() => json!(round(v)),
        _ => Value::Null,
    }
}

fn db(x: Option<f64>) -> Value {
    num(x, r2)
}

fn hz(x: Option<f64>) -> Value {
    num(x, r1)
}

fn bands_json(bands: &[Option<f64>; 6]) -> Value {
    let mut m = Map::new();
    for (band, v) in BANDS.iter().zip(bands) {
        m.insert(band.key.to_string(), db(*v));
    }
    Value::Object(m)
}

fn section_json(s: &SectionReport) -> Value {
    json!({
        "name": s.name,
        "start_bar": s.start_bar,
        "end_bar": s.end_bar,
        "partial": s.partial,
        "integrated_lufs": db(s.integrated_lufs),
        "short_term_max_lufs": db(s.short_term_max_lufs),
        "delta_lu": db(s.delta_lu),
        "bands": bands_json(&s.bands),
        "centroid_median_hz": hz(s.centroid_median_hz),
        "notes": s.notes,
    })
}

impl Analysis {
    fn base_json(&self) -> Map<String, Value> {
        let l = &self.loudness;
        let sp = &self.spectral;
        let mut m = Map::new();
        m.insert(
            "loudness".into(),
            json!({
                "integrated_lufs": db(l.integrated_lufs),
                "short_term_max_lufs": db(l.short_term_max_lufs),
                "lra_lu": db(l.lra_lu),
                "sample_peak_dbfs": db(Some(l.sample_peak_dbfs)),
                "true_peak_dbtp": db(Some(l.true_peak_dbtp)),
                "rms_dbfs": db(Some(l.rms_dbfs)),
                "crest_db": db(l.crest_db),
                "plr_db": db(l.plr_db),
            }),
        );
        m.insert(
            "targets".into(),
            json!({
                "genre": "acid",
                "integrated_lufs": [-8, -6],
                "integrated": self.targets.integrated,
                "true_peak_max_dbtp": -1,
                "true_peak": self.targets.true_peak,
            }),
        );
        m.insert("bands".into(), bands_json(&sp.bands));
        m.insert(
            "low_end".into(),
            json!({
                "kick_region_db": db(sp.low_end.kick_region_db),
                "bass_region_db": db(sp.low_end.bass_region_db),
                "ratio_db": db(sp.low_end.ratio_db),
            }),
        );
        m.insert(
            "stereo".into(),
            json!({
                "correlation": db(sp.stereo.correlation),
                "side_to_mid_db": db(Some(sp.stereo.side_to_mid_db)),
                "low_correlation": db(sp.stereo.low_correlation),
            }),
        );
        let b = &sp.brightness;
        m.insert(
            "brightness".into(),
            json!({
                "median_hz": hz(b.median_hz),
                "p10_hz": hz(b.p10_hz),
                "p90_hz": hz(b.p90_hz),
                "range_oct": db(b.range_oct),
                "profile_hz": b.profile_hz.iter().map(|v| hz(*v)).collect::<Vec<_>>(),
            }),
        );
        m
    }

    /// The full report with stable key order.
    pub fn to_json(&self) -> Value {
        let mut m = self.base_json();
        m.insert(
            "series".into(),
            json!({
                "hop_seconds": 1.0,
                "short_term_lufs": self.short_term_lufs.iter().map(|v| db(*v)).collect::<Vec<_>>(),
                "centroid_hz": self.spectral.brightness.centroid_hz.iter().map(|v| hz(*v)).collect::<Vec<_>>(),
            }),
        );
        m.insert(
            "sections".into(),
            match &self.sections {
                Some(s) => Value::Array(s.iter().map(section_json).collect()),
                None => Value::Null,
            },
        );
        Value::Object(m)
    }

    /// The report without `series` and `sections`.
    pub fn summary_json(&self) -> Value {
        Value::Object(self.base_json())
    }
}
