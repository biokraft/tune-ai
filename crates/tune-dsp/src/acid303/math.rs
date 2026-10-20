//! Ports of the helpers from `GlobalFunctions.h`, `GlobalDefinitions.h`,
//! `rosic_RealFunctions.h` and `rosic_NumberManipulations.h` that the ported
//! modules call.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

/// `#define PI`.
pub const PI: f64 = 3.1415926535897932384626433832795;
/// `#define LN2`.
pub const LN2: f64 = 0.69314718055994530941723212145818;
/// `#define ONE_OVER_LN2`.
pub const ONE_OVER_LN2: f64 = 1.4426950408889634073599246810019;
/// `#define TINY FLT_MIN` (a float constant promoted to double).
pub const TINY: f64 = f32::MIN_POSITIVE as f64;

/// `amp2dB`.
pub fn amp2db(amp: f64) -> f64 {
    8.6858896380650365530225783783321 * amp.ln()
}

/// `dB2amp`.
pub fn db2amp(db: f64) -> f64 {
    (db * 0.11512925464970228420089957273422).exp()
}

/// `beatsToSeconds`.
pub fn beats_to_seconds(beat: f64, bpm: f64) -> f64 {
    (60.0 / bpm) * beat
}

/// `secondsToBeats`.
pub fn seconds_to_beats(time_in_seconds: f64, bpm: f64) -> f64 {
    time_in_seconds * (bpm / 60.0)
}

/// `freqToPitch(freq)` with A4 = 440 Hz.
pub fn freq_to_pitch(freq: f64) -> f64 {
    12.0 * log2(freq / 440.0) + 69.0
}

/// `log2` as defined in `GlobalFunctions.h`.
pub fn log2(x: f64) -> f64 {
    ONE_OVER_LN2 * x.ln()
}

/// `linToLin`.
pub fn lin_to_lin(input: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    let mut tmp = (input - in_min) / (in_max - in_min);
    tmp *= out_max - out_min;
    tmp += out_min;
    tmp
}

/// `linToExp`.
pub fn lin_to_exp(input: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    let tmp = (input - in_min) / (in_max - in_min);
    out_min * (tmp * (out_max / out_min).ln()).exp()
}

/// `expToLin`.
pub fn exp_to_lin(input: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    let tmp = (input / in_min).ln() / (in_max / in_min).ln();
    out_min + tmp * (out_max - out_min)
}

/// `pitchOffsetToFreqFactor`.
pub fn pitch_offset_to_freq_factor(pitch_offset: f64) -> f64 {
    (0.057762265046662109118102676788181 * pitch_offset).exp()
}

/// `pitchToFreq(pitch)` with A4 = 440 Hz.
pub fn pitch_to_freq(pitch: f64) -> f64 {
    8.1757989156437073336828122976033 * (0.057762265046662109118102676788181 * pitch).exp()
}

/// `pitchToFreq(pitch, masterTuneA4)`.
pub fn pitch_to_freq_tuned(pitch: f64, master_tune_a4: f64) -> f64 {
    master_tune_a4
        * 0.018581361171917516667460937040007
        * (0.057762265046662109118102676788181 * pitch).exp()
}

/// `rosic::asinh` (the rosic version, not libm's).
pub fn asinh(x: f64) -> f64 {
    (x + (x * x + 1.0).sqrt()).ln()
}

/// `rosic::clip` (generic in the C++).
pub fn clip<T: PartialOrd>(x: T, min: T, max: T) -> T {
    if x > max {
        max
    } else if x < min {
        min
    } else {
        x
    }
}

/// `sinCos` (the GCC branch: separate `sin` and `cos`). Returns `(sin, cos)`.
pub fn sin_cos(x: f64) -> (f64, f64) {
    (x.sin(), x.cos())
}

/// `roundToInt` (the non-MSVC branch: floor, then round half up).
pub fn round_to_int(x: f64) -> i32 {
    let x_floor = x.floor();
    let x_frac = x - x_floor;
    if x_frac >= 0.5 {
        x_floor as i32 + 1
    } else {
        x_floor as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_is_float_min() {
        assert_eq!(TINY, 1.1754943508222875e-38);
    }

    #[test]
    fn pitch_69_is_440() {
        assert!((pitch_to_freq(69.0) - 440.0).abs() < 1e-9);
        assert!((pitch_to_freq_tuned(69.0, 440.0) - 440.0).abs() < 1e-9);
        assert!((freq_to_pitch(440.0) - 69.0).abs() < 1e-9);
    }

    #[test]
    fn rounding_and_clip() {
        assert_eq!(round_to_int(2.5), 3);
        assert_eq!(round_to_int(-2.5), -2);
        assert_eq!(round_to_int(2.49), 2);
        assert_eq!(clip(130, 0, 127), 127);
        assert_eq!(clip(-1.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn mappings_round_trip() {
        let y = lin_to_exp(0.3, 0.0, 1.0, 200.0, 2000.0);
        assert!((exp_to_lin(y, 200.0, 2000.0, 0.0, 1.0) - 0.3).abs() < 1e-12);
        assert!((lin_to_lin(0.5, 0.0, 1.0, -60.0, 0.0) + 30.0).abs() < 1e-12);
        assert!((amp2db(db2amp(-12.0)) + 12.0).abs() < 1e-12);
        assert!((beats_to_seconds(1.0, 120.0) - 0.5).abs() < 1e-12);
        assert!((seconds_to_beats(0.5, 120.0) - 1.0).abs() < 1e-12);
        assert!((pitch_offset_to_freq_factor(12.0) - 2.0).abs() < 1e-12);
    }
}
