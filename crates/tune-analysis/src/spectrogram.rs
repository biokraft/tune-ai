//! The labelled spectrogram PNG Claude reads in place of listening.
//!
//! Panel layout from the top (px): title 24, bar ruler 24, spectrogram 640
//! (log frequency, 30 Hz bottom to 16 kHz top), gap 8, band strip 96, time
//! axis 24. Colours are absolute: -100 dB is black, -20 dB is the brightest.

use crate::Audio;
use crate::BarMap;
use crate::canvas::{Canvas, Rgb};
use crate::spectral::{BANDS, for_each_frame, stft_size};

pub const WIDTH: u32 = 1600;
pub const PANEL_HEIGHT: u32 = 816;
/// Section boundary colour (2 px wide, from the ruler through the band strip).
pub const BOUNDARY: [u8; 3] = [255, 255, 255];

const PLOT_X0: u32 = 64;
const PLOT_W: u32 = 1520;
const TITLE_H: u32 = 24;
const RULER_Y: u32 = 24;
const SPEC_Y: u32 = 48;
const SPEC_H: u32 = 640;
const STRIP_Y: u32 = 696;
const STRIP_ROW_H: u32 = 16;
const AXIS_Y: u32 = 792;

const LOW_HZ: f64 = 30.0;
const HIGH_HZ: f64 = 16_000.0;
const SPEC_DB: (f64, f64) = (-100.0, -20.0);
const STRIP_DB: (f64, f64) = (-40.0, 0.0);
const TOTAL_LO_HZ: f64 = 20.0;
const TOTAL_HI_HZ: f64 = 20_000.0;

const BACKGROUND: Rgb = [0x10, 0x10, 0x14];
const GRID: Rgb = [0x40, 0x40, 0x40];
const TICK: Rgb = [0x80, 0x80, 0x80];
const TEXT: Rgb = [0xE0, 0xE0, 0xE8];
const SECTION_TEXT: Rgb = [0xFF, 0xD2, 0x7F];
const MAGMA: [Rgb; 6] = [
    [0x00, 0x00, 0x04],
    [0x3B, 0x0F, 0x70],
    [0x8C, 0x29, 0x81],
    [0xDE, 0x49, 0x68],
    [0xFE, 0x9F, 0x6D],
    [0xFC, 0xFD, 0xBF],
];
const HZ_LABELS: [(f64, &str); 8] = [
    (50.0, "50"),
    (100.0, "100"),
    (200.0, "200"),
    (500.0, "500"),
    (1000.0, "1k"),
    (2000.0, "2k"),
    (5000.0, "5k"),
    (10000.0, "10k"),
];
/// Strip labels, `sub` first; four characters is all the 64 px margin holds at 2x.
const STRIP_LABELS: [&str; 6] = ["sub", "low", "lmid", "mid", "high", "air"];

/// y of `hz` in a panel whose top is y = 0.
pub fn freq_to_y(hz: f64) -> u32 {
    let t = (hz / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln();
    let y = f64::from(SPEC_Y) + (1.0 - t) * f64::from(SPEC_H - 1);
    y.round()
        .clamp(f64::from(SPEC_Y), f64::from(SPEC_Y + SPEC_H - 1)) as u32
}

/// Frequency at (fractional) panel y inside the spectrogram.
fn y_to_hz(y: f64) -> f64 {
    let t = 1.0 - (y - f64::from(SPEC_Y)) / f64::from(SPEC_H - 1);
    LOW_HZ * (t * (HIGH_HZ / LOW_HZ).ln()).exp()
}

/// x of an audio frame; `total_frames` is the audio length.
pub fn x_of_frame(frame: usize, total_frames: usize) -> u32 {
    if total_frames == 0 {
        return PLOT_X0;
    }
    let off = (frame as u128 * u128::from(PLOT_W) / total_frames as u128) as u32;
    PLOT_X0 + off.min(PLOT_W - 1)
}

/// Smallest of 1, 2, 4, 8, 16, 32, 64 bars with `step * px_per_bar >= 56`.
pub fn label_step(px_per_bar: f64) -> u32 {
    [1u32, 2, 4, 8, 16, 32, 64]
        .into_iter()
        .find(|&s| f64::from(s) * px_per_bar >= 56.0)
        .unwrap_or(64)
}

/// Mean of the channel powers, so mono and L=R stereo share absolute colours.
fn display_scale(audio: &Audio) -> f64 {
    1.0 / audio.channels.len().max(1) as f64
}

fn ramp(t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0) * (MAGMA.len() - 1) as f64;
    let i = (t.floor() as usize).min(MAGMA.len() - 2);
    let f = t - i as f64;
    let mix = |k: usize| {
        (f64::from(MAGMA[i][k]) * (1.0 - f) + f64::from(MAGMA[i + 1][k]) * f).round() as u8
    };
    [mix(0), mix(1), mix(2)]
}

fn db_colour(power: f64, (lo, hi): (f64, f64)) -> Rgb {
    let db = if power > 0.0 {
        10.0 * power.log10()
    } else {
        lo
    };
    ramp((db.clamp(lo, hi) - lo) / (hi - lo))
}

/// Inclusive bin range for each spectrogram row: bins inside the row's band,
/// else the bin nearest its centre frequency.
fn row_bins(sample_rate: u32, bins: usize) -> Vec<(usize, usize)> {
    let n = stft_size(sample_rate) as f64;
    let sr = f64::from(sample_rate);
    (0..SPEC_H)
        .map(|r| {
            let y = f64::from(SPEC_Y + r);
            let (lo, hi) = (y_to_hz(y + 0.5), y_to_hz(y - 0.5));
            let first = (lo * n / sr).ceil().max(0.0) as usize;
            let last = (hi * n / sr).floor().max(0.0) as usize;
            if first <= last && first < bins {
                (first, last.min(bins - 1))
            } else {
                let k = ((y_to_hz(y) * n / sr).round().max(0.0) as usize).min(bins - 1);
                (k, k)
            }
        })
        .collect()
}

/// Bin range `[lo, hi)` by bin centre frequency, capped to the available bins.
fn bins_in(sample_rate: u32, bins: usize, lo_hz: f64, hi_hz: f64) -> (usize, usize) {
    let n = stft_size(sample_rate) as f64;
    let sr = f64::from(sample_rate);
    let first = (lo_hz * n / sr).ceil() as usize;
    let end = ((hi_hz * n / sr).ceil() as usize).min(bins);
    (first.min(end), end)
}

/// Per-column mean powers: `rows[col][row]` and `bands[col][0..6]` plus the
/// column total in `bands[col][6]`.
struct Columns {
    rows: Vec<Vec<f64>>,
    bands: Vec<[f64; 7]>,
}

/// Row bins, band bins and total bins of one STFT size.
type BinRanges = (Vec<(usize, usize)>, [(usize, usize); 6], (usize, usize));

fn column_powers(audio: &Audio) -> Columns {
    let cols = PLOT_W as usize;
    let mut rows = vec![vec![0.0f64; SPEC_H as usize]; cols];
    let mut bands = vec![[0.0f64; 7]; cols];
    let mut counts = vec![0u32; cols];
    let total = audio.frames().max(1);
    let scale = display_scale(audio);
    let sr = audio.sample_rate;
    let mut ranges: Option<BinRanges> = None;

    for_each_frame(audio, |frame| {
        let (row_ranges, band_ranges, total_range) = ranges.get_or_insert_with(|| {
            let bins = frame.power.len();
            let band_ranges =
                std::array::from_fn(|i| bins_in(sr, bins, BANDS[i].lo_hz, BANDS[i].hi_hz));
            (
                row_bins(sr, bins),
                band_ranges,
                bins_in(sr, bins, TOTAL_LO_HZ, TOTAL_HI_HZ),
            )
        });
        let col =
            ((frame.center_frame as u128 * cols as u128 / total as u128) as usize).min(cols - 1);
        counts[col] += 1;
        for (acc, &(a, b)) in rows[col].iter_mut().zip(row_ranges.iter()) {
            let peak = frame.power[a..=b].iter().copied().fold(0.0f64, f64::max);
            *acc += peak * scale;
        }
        for (i, &(a, b)) in band_ranges.iter().enumerate() {
            bands[col][i] += frame.power[a..b].iter().sum::<f64>() * scale;
        }
        bands[col][6] += frame.power[total_range.0..total_range.1]
            .iter()
            .sum::<f64>()
            * scale;
    });

    for c in 0..cols {
        if counts[c] > 0 {
            let k = f64::from(counts[c]);
            rows[c].iter_mut().for_each(|v| *v /= k);
            bands[c].iter_mut().for_each(|v| *v /= k);
        }
    }
    // Columns with no frame centre take their nearest column that has one.
    let filled: Vec<usize> = (0..cols).filter(|&c| counts[c] > 0).collect();
    if !filled.is_empty() {
        for c in 0..cols {
            if counts[c] == 0 {
                let near = *filled
                    .iter()
                    .min_by_key(|&&f| (f.abs_diff(c), f))
                    .expect("filled is non-empty");
                rows[c] = rows[near].clone();
                bands[c] = bands[near];
            }
        }
    }
    Columns { rows, bands }
}

fn m_ss(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn time_step(px_per_second: f64) -> u32 {
    let steps = [1u32, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600];
    steps
        .into_iter()
        .find(|&s| f64::from(s) * px_per_second >= 80.0)
        .unwrap_or(3600)
}

fn draw_ruler(canvas: &mut Canvas, map: &BarMap, total: usize) {
    let bars = map.bar_frames.len().saturating_sub(1);
    if bars == 0 || total == 0 {
        return;
    }
    let span = map.bar_frames[bars].saturating_sub(map.bar_frames[0]);
    let px_per_bar = f64::from(PLOT_W) * span as f64 / bars as f64 / total as f64;
    let step = label_step(px_per_bar);
    let first = map.first_bar;
    let last = first + bars as u32;

    for i in 0..bars {
        let bar0 = first + i as u32;
        let x = x_of_frame(map.bar_frames[i], total);
        if bar0.is_multiple_of(step) {
            canvas.rect(x, RULER_Y + 16, 1, 8, TICK);
            // 1x text is small: strike it twice, one pixel apart, to embolden it.
            let label = (bar0 + 1).to_string();
            canvas.text(x + 2, RULER_Y + 16, &label, 1, TEXT);
            canvas.text(x + 3, RULER_Y + 16, &label, 1, TEXT);
        } else if px_per_bar >= 4.0 {
            canvas.rect(x, RULER_Y + 20, 1, 4, TICK);
        }
    }

    for sec in &map.sections {
        let (s, e) = (
            sec.start_bar.max(first),
            (sec.start_bar + sec.bars).min(last),
        );
        if s >= e {
            continue;
        }
        let x0 = x_of_frame(map.bar_frames[(s - first) as usize], total);
        let x1 = if e == last {
            PLOT_X0 + PLOT_W
        } else {
            x_of_frame(map.bar_frames[(e - first) as usize], total)
        };
        let room = x1.saturating_sub(x0 + 6) / 16;
        let name: String = sec.name.chars().take(room as usize).collect();
        if !name.is_empty() {
            canvas.text(x0 + 4, RULER_Y, &name, 2, SECTION_TEXT);
        }
    }
}

fn draw_boundaries(canvas: &mut Canvas, map: &BarMap, total: usize) {
    let bars = map.bar_frames.len().saturating_sub(1) as u32;
    let (first, last) = (map.first_bar, map.first_bar + bars);
    for sec in &map.sections {
        if sec.start_bar > first && sec.start_bar < last {
            let x = x_of_frame(map.bar_frames[(sec.start_bar - first) as usize], total);
            canvas.rect(x, RULER_Y, 2, AXIS_Y - RULER_Y, BOUNDARY);
        }
    }
}

fn panel(audio: &Audio, title: &str, map: Option<&BarMap>) -> Canvas {
    let mut canvas = Canvas::new(WIDTH, PANEL_HEIGHT, BACKGROUND);
    let total = audio.frames();
    let title: String = title
        .chars()
        .take(((WIDTH - PLOT_X0) / 16) as usize)
        .collect();
    canvas.text(PLOT_X0, (TITLE_H - 16) / 2, &title, 2, TEXT);

    let cols = column_powers(audio);
    for (c, (rows, bands)) in cols.rows.iter().zip(&cols.bands).enumerate() {
        let x = PLOT_X0 + c as u32;
        for (r, &p) in rows.iter().enumerate() {
            canvas.set(x, SPEC_Y + r as u32, db_colour(p, SPEC_DB));
        }
        for (i, &p) in bands[..6].iter().enumerate() {
            let share = if bands[6] > 0.0 { p / bands[6] } else { 0.0 };
            let y = STRIP_Y + (5 - i as u32) * STRIP_ROW_H;
            canvas.rect(x, y, 1, STRIP_ROW_H, db_colour(share, STRIP_DB));
        }
    }

    for (hz, label) in HZ_LABELS {
        let y = freq_to_y(hz);
        for x in (PLOT_X0..PLOT_X0 + PLOT_W).step_by(2) {
            canvas.set(x, y, GRID);
        }
        let w = Canvas::text_width(label, 2);
        canvas.text(PLOT_X0 - 4 - w, y.saturating_sub(8), label, 2, TEXT);
    }
    for (i, label) in STRIP_LABELS.iter().enumerate() {
        canvas.text(0, STRIP_Y + (5 - i as u32) * STRIP_ROW_H, label, 2, TEXT);
    }

    let seconds = total as f64 / f64::from(audio.sample_rate.max(1));
    if seconds > 0.0 {
        let pps = f64::from(PLOT_W) / seconds;
        let step = time_step(pps);
        let mut t = 0u32;
        while f64::from(t) <= seconds {
            let x = PLOT_X0 + (f64::from(t) * pps).round() as u32;
            let label = m_ss(t);
            if x + 3 + Canvas::text_width(&label, 2) <= WIDTH {
                canvas.rect(x, AXIS_Y, 1, 8, TICK);
                canvas.text(x + 3, AXIS_Y + 8, &label, 2, TEXT);
            }
            t += step;
        }
    }

    if let Some(map) = map {
        draw_ruler(&mut canvas, map, total);
        draw_boundaries(&mut canvas, map, total);
    }
    canvas
}

/// One labelled spectrogram panel, `WIDTH` x `PANEL_HEIGHT`.
pub fn spectrogram_png(audio: &Audio, title: &str, map: Option<&BarMap>) -> Vec<u8> {
    panel(audio, title, map).encode()
}

/// Ours over the reference: two panels, same frequency axis and colour scale.
pub fn compare_png(ours: (&Audio, &str, Option<&BarMap>), reference: (&Audio, &str)) -> Vec<u8> {
    panel(ours.0, ours.1, ours.2)
        .stacked(panel(reference.0, reference.1, None))
        .encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bin_centred_sine(sr: u32, channels: usize) -> Audio {
        let n = stft_size(sr);
        assert_eq!(n, 4096);
        let k = 100.0;
        let hz = k * f64::from(sr) / n as f64;
        let x: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(sr)).sin() as f32)
            .collect();
        Audio {
            sample_rate: sr,
            channels: vec![x; channels],
        }
    }

    #[test]
    fn full_scale_bin_sine_is_0_db_mono_or_stereo() {
        for sr in [44_100, 48_000] {
            for channels in [1, 2] {
                let audio = bin_centred_sine(sr, channels);
                let scale = display_scale(&audio);
                let mut peak = 0.0f64;
                for_each_frame(&audio, |f| {
                    peak = peak.max(f.power.iter().copied().fold(0.0, f64::max) * scale);
                });
                let db = 10.0 * peak.log10();
                assert!(db.abs() < 0.05, "sr {sr} ch {channels}: {db} dB");
            }
        }
    }
}
