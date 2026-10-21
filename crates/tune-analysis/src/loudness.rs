use ebur128::{Channel, EbuR128, Mode};

use crate::{AnalysisError, Audio};

/// Peaks and levels are floored here so silence stays a finite number.
const FLOOR_DB: f64 = -200.0;

/// Loudness per EBU R128 / ITU-R BS.1770, plus peak and RMS levels.
#[derive(Clone, Debug, PartialEq)]
pub struct Loudness {
    pub integrated_lufs: Option<f64>,
    pub short_term_max_lufs: Option<f64>,
    pub lra_lu: Option<f64>,
    pub sample_peak_dbfs: f64,
    pub true_peak_dbtp: f64,
    pub rms_dbfs: f64,
    /// `sample_peak_dbfs - rms_dbfs`; `None` for silence.
    pub crest_db: Option<f64>,
    /// `true_peak_dbtp - integrated_lufs`; `None` when there is no integrated loudness.
    pub plr_db: Option<f64>,
}

fn meter_err(e: ebur128::Error) -> AnalysisError {
    AnalysisError::Meter(e.to_string())
}

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

fn db(linear: f64) -> f64 {
    if linear > 0.0 {
        (20.0 * linear.log10()).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

/// Measure `audio`. The series is on a 1 s grid: entry `i` is the 3 s
/// short-term window ending at `i + 1` s (`None` for `i < 2` and for silence).
pub fn measure_loudness(audio: &Audio) -> Result<(Loudness, Vec<Option<f64>>), AnalysisError> {
    let channels = audio.channels.len();
    if channels == 0 || audio.frames() == 0 {
        return Err(AnalysisError::Invalid("audio is empty".into()));
    }
    let rate = audio.sample_rate as usize;
    let mode = Mode::I | Mode::S | Mode::LRA | Mode::SAMPLE_PEAK | Mode::TRUE_PEAK;
    let mut meter = EbuR128::new(channels as u32, audio.sample_rate, mode).map_err(meter_err)?;
    if channels == 1 {
        meter.set_channel(0, Channel::DualMono).map_err(meter_err)?;
    }

    let frames = audio.frames();
    let mut series = Vec::with_capacity(frames / rate);
    let mut start = 0;
    while start < frames {
        let end = (start + rate).min(frames);
        let chunk: Vec<&[f32]> = audio.channels.iter().map(|c| &c[start..end]).collect();
        meter.add_frames_planar_f32(&chunk).map_err(meter_err)?;
        if end - start == rate {
            let i = start / rate;
            series.push(if i < 2 {
                None
            } else {
                meter.loudness_shortterm().ok().and_then(finite)
            });
        }
        start = end;
    }

    let integrated_lufs = meter.loudness_global().ok().and_then(finite);
    let short_term_max_lufs = series.iter().flatten().copied().reduce(f64::max);
    let lra_lu = if audio.seconds() < 3.0 {
        None
    } else {
        meter.loudness_range().ok().and_then(finite)
    };

    let mut sample_peak = 0.0f64;
    let mut true_peak = 0.0f64;
    for c in 0..channels as u32 {
        sample_peak = sample_peak.max(meter.sample_peak(c).map_err(meter_err)?);
        true_peak = true_peak.max(meter.true_peak(c).map_err(meter_err)?);
    }
    let sample_peak_dbfs = db(sample_peak);
    let true_peak_dbtp = db(true_peak);

    let mut sum_sq = 0.0f64;
    for ch in &audio.channels {
        for &s in ch {
            sum_sq += f64::from(s) * f64::from(s);
        }
    }
    let mean_sq = sum_sq / (frames * channels) as f64;
    let rms_dbfs = db(mean_sq.sqrt());
    let crest_db = (rms_dbfs > FLOOR_DB).then_some(sample_peak_dbfs - rms_dbfs);
    let plr_db = integrated_lufs.map(|i| true_peak_dbtp - i);

    Ok((
        Loudness {
            integrated_lufs,
            short_term_max_lufs,
            lra_lu,
            sample_peak_dbfs,
            true_peak_dbtp,
            rms_dbfs,
            crest_db,
            plr_db,
        },
        series,
    ))
}
