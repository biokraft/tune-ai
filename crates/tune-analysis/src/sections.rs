use crate::spectral::measure_spectral;
use crate::{AnalysisError, Audio, measure_loudness};

/// One named stretch of the song, in 0-based song bars.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionSpan {
    pub name: String,
    pub start_bar: u32,
    pub bars: u32,
}

/// Where the song's bars and sections fall inside an audio file.
#[derive(Clone, Debug, PartialEq)]
pub struct BarMap {
    /// 0-based song bar at audio frame 0.
    pub first_bar: u32,
    /// Frame of each bar start from `first_bar` on, plus the end (len = bars + 1).
    pub bar_frames: Vec<usize>,
    /// The audio ends inside the last bar of `bar_frames`, so the section
    /// holding that bar is partial.
    pub partial_end: bool,
    /// The whole song's sections; analysis clips them to the audio.
    pub sections: Vec<SectionSpan>,
}

/// Measurements of one section. Bars are 1-based and inclusive.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionReport {
    pub name: String,
    pub start_bar: u32,
    pub end_bar: u32,
    pub partial: bool,
    pub integrated_lufs: Option<f64>,
    pub short_term_max_lufs: Option<f64>,
    /// Integrated loudness minus the previous reported section's.
    pub delta_lu: Option<f64>,
    pub bands: [Option<f64>; 6],
    pub centroid_median_hz: Option<f64>,
    pub notes: Vec<String>,
}

fn bands_match(a: &[Option<f64>; 6], b: &[Option<f64>; 6]) -> bool {
    a.iter().zip(b).all(|(x, y)| match (x, y) {
        (Some(x), Some(y)) => (x - y).abs() <= 1.0,
        (None, None) => true,
        _ => false,
    })
}

/// Measure every section of `map` that overlaps the audio, clipped to it.
pub fn analyze_sections(audio: &Audio, map: &BarMap) -> Result<Vec<SectionReport>, AnalysisError> {
    let audio_bars = map.bar_frames.len().saturating_sub(1) as u32;
    let first = map.first_bar;
    let last = first + audio_bars;
    let mut out: Vec<SectionReport> = Vec::new();
    for span in &map.sections {
        let end = span.start_bar + span.bars;
        let cs = span.start_bar.max(first);
        let ce = end.min(last);
        if cs >= ce {
            continue;
        }
        let a = map.bar_frames[(cs - first) as usize];
        let b = map.bar_frames[(ce - first) as usize];
        let slice = audio.slice(a..b);
        if slice.frames() == 0 {
            continue;
        }
        let (loud, _) = measure_loudness(&slice)?;
        let spectral = measure_spectral(&slice);
        let mut report = SectionReport {
            name: span.name.clone(),
            start_bar: cs + 1,
            end_bar: ce,
            partial: cs != span.start_bar || ce != end || (ce == last && map.partial_end),
            integrated_lufs: loud.integrated_lufs,
            short_term_max_lufs: loud.short_term_max_lufs,
            delta_lu: None,
            bands: spectral.bands,
            centroid_median_hz: spectral.brightness.median_hz,
            notes: Vec::new(),
        };
        if report.integrated_lufs.is_none() {
            report.notes.push("silent".to_string());
        }
        if let Some(prev) = out.last()
            && let (Some(c), Some(p)) = (report.integrated_lufs, prev.integrated_lufs)
        {
            let d = c - p;
            report.delta_lu = Some(d);
            if d.abs() < 0.5 && bands_match(&report.bands, &prev.bands) {
                report.notes.push(format!(
                    "same energy and balance as `{}`: nothing changes here",
                    prev.name
                ));
            }
        }
        out.push(report);
    }
    Ok(out)
}
