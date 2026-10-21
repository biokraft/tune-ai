//! Compare a passage with a reference: numeric deltas and plain-language verdicts.

use serde_json::{Map, Value, json};

use crate::report::{Analysis, TRUE_PEAK_MAX_DBTP, r2};
use crate::spectral::{BANDS, SIDE_MID_LIMIT_DB};

/// A band level at or below this is treated as "no content" and not compared.
const NO_CONTENT_DB: f64 = -100.0;

/// Ours minus reference. `None` when either side lacks the quantity.
#[derive(Clone, Debug, PartialEq)]
pub struct Deltas {
    pub integrated_lu: Option<f64>,
    pub true_peak_db: f64,
    pub lra_lu: Option<f64>,
    pub crest_db: Option<f64>,
    pub bands: [Option<f64>; 6],
    pub low_end_ratio_db: Option<f64>,
    pub side_to_mid_db: Option<f64>,
    /// `log2(ours median / reference median)`.
    pub centroid_median_oct: Option<f64>,
    /// Ours `range_oct` minus the reference's.
    pub centroid_range_oct: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    pub deltas: Deltas,
    pub verdicts: Vec<String>,
}

fn diff(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a - b),
        _ => None,
    }
}

fn oct(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 => Some((a / b).log2()),
        _ => None,
    }
}

/// Words for a band delta: (above reference, below reference).
const BAND_WORDS: [(&str, &str); 6] = [
    ("heavier sub", "thinner sub"),
    ("boomier", "less bass weight"),
    ("muddier", "hollower"),
    ("more forward", "more scooped"),
    ("brighter, maybe harsh", "duller"),
    ("more air", "less air"),
];

pub fn compare(ours: &Analysis, reference: &Analysis) -> Comparison {
    let (ol, rl) = (&ours.loudness, &reference.loudness);
    let (os, rs) = (&ours.spectral, &reference.spectral);
    let mut bands = [None; 6];
    let mut empty = [false; 6];
    for (i, b) in bands.iter_mut().enumerate() {
        if let (Some(a), Some(r)) = (os.bands[i], rs.bands[i]) {
            if a <= NO_CONTENT_DB || r <= NO_CONTENT_DB {
                empty[i] = true;
            } else {
                *b = Some(a - r);
            }
        }
    }
    let (so, sr) = (os.stereo.side_to_mid_db, rs.stereo.side_to_mid_db);
    let (so_mono, sr_mono) = (so.abs() >= SIDE_MID_LIMIT_DB, sr.abs() >= SIDE_MID_LIMIT_DB);
    // Both mono: they agree (0). Exactly one mono: not comparable.
    let width = if so_mono && sr_mono {
        Some(0.0)
    } else if so_mono || sr_mono {
        None
    } else {
        Some(so - sr)
    };
    let deltas = Deltas {
        integrated_lu: diff(ol.integrated_lufs, rl.integrated_lufs),
        true_peak_db: ol.true_peak_dbtp - rl.true_peak_dbtp,
        lra_lu: diff(ol.lra_lu, rl.lra_lu),
        crest_db: diff(ol.crest_db, rl.crest_db),
        bands,
        low_end_ratio_db: diff(os.low_end.ratio_db, rs.low_end.ratio_db),
        side_to_mid_db: width,
        centroid_median_oct: oct(os.brightness.median_hz, rs.brightness.median_hz),
        centroid_range_oct: diff(os.brightness.range_oct, rs.brightness.range_oct),
    };

    let mut v: Vec<String> = Vec::new();
    if let Some(d) = deltas.integrated_lu.filter(|d| d.abs() >= 1.0) {
        let word = if d > 0.0 { "louder" } else { "quieter" };
        v.push(format!("loudness {d:+.1} LU vs reference: {word}"));
    }
    if ol.true_peak_dbtp > TRUE_PEAK_MAX_DBTP {
        v.push(format!(
            "true peak {:.2} dBTP: over the -1 dBTP ceiling",
            ol.true_peak_dbtp
        ));
    }
    if let Some(d) = deltas.crest_db.filter(|d| d.abs() >= 2.0) {
        let word = if d > 0.0 {
            "more dynamic"
        } else {
            "more squashed"
        };
        v.push(format!("crest factor {d:+.1} dB vs reference: {word}"));
    }
    for (i, band) in BANDS.iter().enumerate() {
        if empty[i] {
            v.push(format!(
                "{}: one side has no content there, not compared",
                band.label
            ));
            continue;
        }
        if let Some(d) = deltas.bands[i].filter(|d| d.abs() >= 1.5) {
            let prefix = if d.abs() < 3.0 { "slightly " } else { "" };
            let word = if d > 0.0 {
                BAND_WORDS[i].0
            } else {
                BAND_WORDS[i].1
            };
            v.push(format!(
                "{} {d:+.1} dB vs reference: {prefix}{word}",
                band.label
            ));
        }
    }
    if let Some(d) = deltas.low_end_ratio_db.filter(|d| d.abs() >= 2.0) {
        let tail = if d > 0.0 {
            "low end leans on the kick region (20-90 Hz)"
        } else {
            "low end leans on the bass region (90-250 Hz)"
        };
        v.push(format!("kick/bass ratio {d:+.1} dB vs reference: {tail}"));
    }
    if let Some(c) = os.stereo.low_correlation.filter(|c| *c < 0.9) {
        v.push(format!(
            "low end below 120 Hz is not mono (correlation {c:.2}): keep kick and bass centred"
        ));
    }
    if so_mono != sr_mono {
        v.push("width: one side is mono, not compared".to_string());
    } else if let Some(d) = width.filter(|d| d.abs() >= 3.0) {
        let word = if d > 0.0 { "wider" } else { "narrower" };
        v.push(format!("side/mid {d:+.1} dB vs reference: {word}"));
    }
    if let Some(d) = deltas.centroid_median_oct.filter(|d| d.abs() >= 0.25) {
        let word = if d > 0.0 { "brighter" } else { "darker" };
        v.push(format!("brightness {d:+.2} oct vs reference: {word}"));
    }
    if let (Some(d), Some(o), Some(r)) = (
        deltas.centroid_range_oct,
        os.brightness.range_oct,
        rs.brightness.range_oct,
    ) && d.abs() >= 0.5
    {
        let word = if d > 0.0 { "more sweep" } else { "less sweep" };
        v.push(format!(
            "filter movement {o:.1} oct vs {r:.1} oct in the reference: {word}"
        ));
    }
    if !numeric_verdict_fired(&v) {
        v.push("matches the reference within tolerance".to_string());
    }
    Comparison {
        deltas,
        verdicts: v,
    }
}

fn num(x: Option<f64>) -> Value {
    match x {
        Some(v) if v.is_finite() => json!(r2(v)),
        _ => Value::Null,
    }
}

impl Comparison {
    /// `{deltas {..., bands {sub..air}}, verdicts}`, 2-decimal rounding, `None` as `null`.
    pub fn to_json(&self) -> Value {
        let d = &self.deltas;
        let mut bands = Map::new();
        for (band, v) in BANDS.iter().zip(&d.bands) {
            bands.insert(band.key.to_string(), num(*v));
        }
        json!({
            "deltas": {
                "integrated_lu": num(d.integrated_lu),
                "true_peak_db": num(Some(d.true_peak_db)),
                "lra_lu": num(d.lra_lu),
                "crest_db": num(d.crest_db),
                "bands": Value::Object(bands),
                "low_end_ratio_db": num(d.low_end_ratio_db),
                "side_to_mid_db": num(d.side_to_mid_db),
                "centroid_median_oct": num(d.centroid_median_oct),
                "centroid_range_oct": num(d.centroid_range_oct),
            },
            "verdicts": self.verdicts,
        })
    }
}

fn numeric_verdict_fired(v: &[String]) -> bool {
    v.iter().any(|s| !s.ends_with(", not compared"))
}
