//! Spectral measures from one STFT: band balance, brightness, low end, stereo.

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

use crate::Audio;

/// Band levels floor here so a silent band stays a finite number.
const BAND_FLOOR_DB: f64 = -120.0;
/// Side/mid floors (and caps) here, like the other silence levels.
pub(crate) const SIDE_MID_LIMIT_DB: f64 = 200.0;
/// Below this per-second RMS the centroid is meaningless.
const CENTROID_GATE_DB: f64 = -80.0;
const RANGE_LO_HZ: f64 = 20.0;
const RANGE_HI_HZ: f64 = 20_000.0;
/// Kick/bass split of the low-end check.
const KICK_BASS_SPLIT_HZ: f64 = 90.0;
const LOW_MONO_HZ: f64 = 120.0;

/// One frequency band of the balance report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub key: &'static str,
    pub label: &'static str,
    pub lo_hz: f64,
    pub hi_hz: f64,
}

const fn band(key: &'static str, label: &'static str, lo_hz: f64, hi_hz: f64) -> Band {
    Band {
        key,
        label,
        lo_hz,
        hi_hz,
    }
}

pub const BANDS: [Band; 6] = [
    band("sub", "sub", 20.0, 60.0),
    band("low", "low", 60.0, 250.0),
    band("low_mid", "low-mid", 250.0, 1000.0),
    band("mid", "mid", 1000.0, 4000.0),
    band("high", "high", 4000.0, 12000.0),
    band("air", "air", 12000.0, 20000.0),
];

/// One STFT frame. `power` is `|L|² + |R|²` (mono: `|X|²`), normalised so a
/// full-scale sine's bin peak reads 1.0 (0 dB). `right` is `left` for mono.
pub(crate) struct Frame<'a> {
    #[allow(dead_code)] // read by the spectrogram (Task 5)
    pub index: usize,
    pub center_frame: usize,
    pub power: &'a [f64],
    pub left: &'a [Complex<f64>],
    pub right: &'a [Complex<f64>],
}

/// STFT window length: 85 ms rounded up to a power of two (4096 at 44.1/48 kHz).
pub(crate) fn stft_size(sample_rate: u32) -> usize {
    ((f64::from(sample_rate) * 0.085) as usize)
        .next_power_of_two()
        .max(2)
}

/// Run a Hann STFT (hop = size / 4) over frames that lie fully inside the
/// signal; a signal shorter than one window gets one zero-padded frame.
pub(crate) fn for_each_frame(audio: &Audio, mut f: impl FnMut(Frame<'_>)) {
    let n = stft_size(audio.sample_rate);
    let hop = n / 4;
    let len = audio.frames();
    if len == 0 || audio.channels.is_empty() {
        return;
    }
    let window: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
        .collect();
    let half_sum = window.iter().sum::<f64>() / 2.0;
    let norm = half_sum * half_sum;

    let plan = RealFftPlanner::<f64>::new().plan_fft_forward(n);
    let mut input = plan.make_input_vec();
    let mut left = plan.make_output_vec();
    let mut right = plan.make_output_vec();
    let mut power = vec![0.0; left.len()];
    let stereo = audio.is_stereo();

    let frames = if len >= n { (len - n) / hop + 1 } else { 1 };
    for index in 0..frames {
        let start = index * hop;
        let fill = |input: &mut [f64], ch: &[f32]| {
            for (i, v) in input.iter_mut().enumerate() {
                *v = ch.get(start + i).map_or(0.0, |&s| f64::from(s)) * window[i];
            }
        };
        fill(&mut input, &audio.channels[0]);
        plan.process(&mut input, &mut left)
            .expect("buffer lengths come from the plan");
        if stereo {
            fill(&mut input, &audio.channels[1]);
            plan.process(&mut input, &mut right)
                .expect("buffer lengths come from the plan");
            for (p, (l, r)) in power.iter_mut().zip(left.iter().zip(&right)) {
                *p = (l.norm_sqr() + r.norm_sqr()) / norm;
            }
        } else {
            for (p, l) in power.iter_mut().zip(&left) {
                *p = l.norm_sqr() / norm;
            }
        }
        f(Frame {
            index,
            center_frame: start + n / 2,
            power: &power,
            left: &left,
            right: if stereo { &right } else { &left },
        });
    }
}

/// Kick and bass regions against the whole spectrum.
#[derive(Clone, Debug, PartialEq)]
pub struct LowEnd {
    /// 20-90 Hz relative to the 20 Hz-20 kHz total.
    pub kick_region_db: Option<f64>,
    /// 90-250 Hz relative to the same total.
    pub bass_region_db: Option<f64>,
    /// `kick_region_db - bass_region_db`.
    pub ratio_db: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stereo {
    /// Time-domain L/R correlation; `None` when a side is silent.
    pub correlation: Option<f64>,
    pub side_to_mid_db: f64,
    /// L/R correlation of the STFT bins between 20 and 120 Hz.
    pub low_correlation: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Brightness {
    /// Spectral centroid per second of audio (entry `i` covers `[i, i+1)` s).
    pub centroid_hz: Vec<Option<f64>>,
    pub median_hz: Option<f64>,
    pub p10_hz: Option<f64>,
    pub p90_hz: Option<f64>,
    /// `log2(p90 / p10)`: how far the filter moves.
    pub range_oct: Option<f64>,
    /// Median centroid of each eighth of the duration.
    pub profile_hz: [Option<f64>; 8],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spectral {
    pub bands: [Option<f64>; 6],
    pub low_end: LowEnd,
    pub stereo: Stereo,
    pub brightness: Brightness,
}

/// Nearest-rank percentile (`p` in 0..=1) of an ascending, non-empty slice.
fn nearest_rank(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn percentile(values: &[Option<f64>], p: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().flatten().copied().collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(nearest_rank(&v, p))
}

fn rel_db(part: f64, total: f64) -> Option<f64> {
    (total > 0.0).then(|| {
        if part > 0.0 {
            (10.0 * (part / total).log10()).max(BAND_FLOOR_DB)
        } else {
            BAND_FLOOR_DB
        }
    })
}

/// Time-domain correlation and side/mid ratio.
fn time_stereo(audio: &Audio) -> (Option<f64>, f64) {
    let Some(l) = audio.channels.first() else {
        return (None, -SIDE_MID_LIMIT_DB);
    };
    let r = audio.channels.get(1).unwrap_or(l);
    let (mut ll, mut rr, mut lr, mut mm, mut ss) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (&a, &b) in l.iter().zip(r) {
        let (a, b) = (f64::from(a), f64::from(b));
        ll += a * a;
        rr += b * b;
        lr += a * b;
        let (m, s) = ((a + b) / 2.0, (a - b) / 2.0);
        mm += m * m;
        ss += s * s;
    }
    let correlation = (ll > 0.0 && rr > 0.0).then(|| (lr / (ll * rr).sqrt()).clamp(-1.0, 1.0));
    let side_mid = if ss <= 0.0 {
        -SIDE_MID_LIMIT_DB
    } else if mm <= 0.0 {
        SIDE_MID_LIMIT_DB
    } else {
        (10.0 * (ss / mm).log10()).clamp(-SIDE_MID_LIMIT_DB, SIDE_MID_LIMIT_DB)
    };
    (correlation, side_mid)
}

/// Sum of squares of the samples of every channel in `[a, b)`, per sample count.
fn rms_db(audio: &Audio, a: usize, b: usize) -> f64 {
    let (mut sum, mut count) = (0.0f64, 0usize);
    for ch in &audio.channels {
        for &s in &ch[a.min(ch.len())..b.min(ch.len())] {
            sum += f64::from(s) * f64::from(s);
            count += 1;
        }
    }
    if count == 0 || sum <= 0.0 {
        return f64::NEG_INFINITY;
    }
    10.0 * (sum / count as f64).log10()
}

/// Measure band balance, brightness, low end and stereo in one STFT pass.
pub fn measure_spectral(audio: &Audio) -> Spectral {
    let sr = audio.sample_rate;
    let rate = f64::from(sr);
    let size = stft_size(sr);
    let bins = size / 2 + 1;
    let hz = |k: usize| k as f64 * rate / size as f64;
    let hi_cap = RANGE_HI_HZ.min(rate / 2.0);
    let in_range = |k: usize| hz(k) >= RANGE_LO_HZ && hz(k) < hi_cap;
    let band_of: Vec<Option<usize>> = (0..bins)
        .map(|k| {
            in_range(k)
                .then(|| {
                    BANDS
                        .iter()
                        .position(|b| hz(k) >= b.lo_hz && hz(k) < b.hi_hz)
                })
                .flatten()
        })
        .collect();

    let seconds = audio.frames().div_ceil(sr as usize).max(1);
    let mut band_sum = [0.0f64; 6];
    let (mut total, mut kick, mut bass) = (0.0f64, 0.0f64, 0.0f64);
    let mut cen_num = vec![0.0f64; seconds];
    let mut cen_den = vec![0.0f64; seconds];
    let (mut x_lr, mut x_ll, mut x_rr) = (0.0f64, 0.0f64, 0.0f64);

    for_each_frame(audio, |fr| {
        let sec = (fr.center_frame / sr as usize).min(seconds - 1);
        #[allow(clippy::needless_range_loop)]
        // k indexes four parallel slices and gives the bin frequency
        for k in 0..bins {
            if !in_range(k) {
                continue;
            }
            let (f, p) = (hz(k), fr.power[k]);
            total += p;
            if f < KICK_BASS_SPLIT_HZ {
                kick += p;
            } else if f < 250.0 {
                bass += p;
            }
            if let Some(b) = band_of[k] {
                band_sum[b] += p;
            }
            cen_num[sec] += f * p;
            cen_den[sec] += p;
            if f < LOW_MONO_HZ {
                let (l, r) = (fr.left[k], fr.right[k]);
                x_lr += l.re * r.re + l.im * r.im;
                x_ll += l.norm_sqr();
                x_rr += r.norm_sqr();
            }
        }
    });

    let bands = std::array::from_fn(|i| rel_db(band_sum[i], total));
    let kick_region_db = rel_db(kick, total);
    let bass_region_db = rel_db(bass, total);
    let ratio_db = kick_region_db.zip(bass_region_db).map(|(k, b)| k - b);

    let (correlation, side_to_mid_db) = time_stereo(audio);
    let low_correlation =
        (x_ll > 0.0 && x_rr > 0.0).then(|| (x_lr / (x_ll * x_rr).sqrt()).clamp(-1.0, 1.0));

    let centroid_hz: Vec<Option<f64>> = (0..seconds)
        .map(|s| {
            let loud = rms_db(audio, s * sr as usize, (s + 1) * sr as usize) >= CENTROID_GATE_DB;
            (loud && cen_den[s] > 0.0).then(|| cen_num[s] / cen_den[s])
        })
        .collect();
    let profile_hz = std::array::from_fn(|i| {
        let n = centroid_hz.len();
        percentile(&centroid_hz[i * n / 8..(i + 1) * n / 8], 0.5)
    });
    let p10_hz = percentile(&centroid_hz, 0.1);
    let p90_hz = percentile(&centroid_hz, 0.9);
    let range_oct = p10_hz
        .zip(p90_hz)
        .filter(|(lo, _)| *lo > 0.0)
        .map(|(lo, hi)| (hi / lo).log2());

    Spectral {
        bands,
        low_end: LowEnd {
            kick_region_db,
            bass_region_db,
            ratio_db,
        },
        stereo: Stereo {
            correlation,
            side_to_mid_db,
            low_correlation,
        },
        brightness: Brightness {
            median_hz: percentile(&centroid_hz, 0.5),
            centroid_hz,
            p10_hz,
            p90_hz,
            range_oct,
            profile_hz,
        },
    }
}
