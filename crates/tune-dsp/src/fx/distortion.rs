//! Oversampled distortion: drive, shaper, DC blocker, dry/wet mix, output gain.

use crate::acid303::{EllipticQuarterBandFilter, OnePoleFilter, OnePoleMode, math::db2amp};
use crate::param::{Curve, ParamError, ParamSpec};

/// Distortion curve applied to the driven signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistortionMode {
    Soft,
    Hard,
    Overdrive,
}

/// User-facing parameters of [`Distortion`], in plain units.
pub const DIST_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "dist.drive",
        label: "Drive",
        unit: "dB",
        min: 0.0,
        max: 48.0,
        curve: Curve::Linear,
        default: 12.0,
        help: "How hard the 303 is pushed into distortion. More drive, more grit and sustain.",
    },
    ParamSpec {
        id: "dist.mix",
        label: "Mix",
        unit: "",
        min: 0.0,
        max: 1.0,
        curve: Curve::Linear,
        default: 1.0,
        help: "Blend of distorted and clean signal.",
    },
    ParamSpec {
        id: "dist.output",
        label: "Output",
        unit: "dB",
        min: -24.0,
        max: 6.0,
        curve: Curve::Linear,
        default: -6.0,
        help: "Level after the distortion, to keep loudness in check.",
    },
];

const OVERSAMPLE: usize = 4;
const OVERDRIVE_BIAS: f64 = 0.25;
const DC_BLOCK_HZ: f64 = 10.0;
/// Samples of delay applied to the dry path (matches the wet path).
pub const LATENCY: usize = 4;

/// Oversampled waveshaper.
///
/// Signal path: drive gain, 4x zero-stuff upsample (gain 4) through an
/// elliptic anti-alias filter, shaper, a second anti-alias filter, decimate,
/// 10 Hz DC blocker, dry/wet crossfade, output gain.
///
/// Latency: the wet path lags by the group delay of the two anti-alias
/// filters, measured at 4 samples (44.1 kHz, 440 Hz). The dry path is delayed
/// by the same fixed [`LATENCY`] samples so the crossfade does not comb-filter.
/// The whole effect therefore runs [`LATENCY`] samples behind its input.
#[derive(Clone, Debug)]
pub struct Distortion {
    mode: DistortionMode,
    drive_gain: f64,
    mix: f64,
    output_gain: f64,
    up_filter: EllipticQuarterBandFilter,
    down_filter: EllipticQuarterBandFilter,
    dc_blocker: OnePoleFilter,
    dry_delay: [f64; LATENCY],
    dry_pos: usize,
}

impl Distortion {
    /// Creates a distortion at `sample_rate` Hz with [`DIST_PARAMS`] defaults
    /// and [`DistortionMode::Soft`].
    pub fn new(sample_rate: f64) -> Self {
        let mut dc_blocker = OnePoleFilter::new();
        dc_blocker.set_sample_rate(sample_rate);
        dc_blocker.set_mode(OnePoleMode::Highpass);
        dc_blocker.set_cutoff(DC_BLOCK_HZ);
        let mut d = Self {
            mode: DistortionMode::Soft,
            drive_gain: 1.0,
            mix: 1.0,
            output_gain: 1.0,
            up_filter: EllipticQuarterBandFilter::new(),
            down_filter: EllipticQuarterBandFilter::new(),
            dc_blocker,
            dry_delay: [0.0; LATENCY],
            dry_pos: 0,
        };
        for spec in DIST_PARAMS {
            d.apply(spec.id, spec.default);
        }
        d
    }

    pub fn set_mode(&mut self, m: DistortionMode) {
        self.mode = m;
    }

    /// Sets a parameter in plain units (see [`DIST_PARAMS`]).
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let spec = DIST_PARAMS
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| ParamError::UnknownId(id.to_owned()))?;
        if !(spec.min..=spec.max).contains(&plain) {
            return Err(ParamError::OutOfRange {
                id: spec.id.to_owned(),
                value: plain,
                min: spec.min,
                max: spec.max,
            });
        }
        self.apply(spec.id, plain);
        Ok(())
    }

    fn apply(&mut self, id: &str, plain: f64) {
        match id {
            "dist.drive" => self.drive_gain = db2amp(plain),
            "dist.mix" => self.mix = plain,
            "dist.output" => self.output_gain = db2amp(plain),
            _ => unreachable!("DIST_PARAMS id `{id}` has no setter"),
        }
    }

    #[inline]
    fn shape(&self, x: f64) -> f64 {
        let v = self.drive_gain * x;
        match self.mode {
            DistortionMode::Soft => v.tanh(),
            DistortionMode::Hard => v.clamp(-1.0, 1.0),
            DistortionMode::Overdrive => (v + OVERDRIVE_BIAS).tanh() - OVERDRIVE_BIAS.tanh(),
        }
    }

    pub fn process(&mut self, x: f64) -> f64 {
        let mut wet = 0.0;
        for k in 0..OVERSAMPLE {
            let stuffed = if k == 0 { OVERSAMPLE as f64 * x } else { 0.0 };
            let up = self.up_filter.get_sample(stuffed);
            let shaped = self.shape(up);
            // Keep the last of the four filtered samples.
            wet = self.down_filter.get_sample(shaped);
        }
        let wet = self.dc_blocker.get_sample(wet);
        let dry = self.dry_delay[self.dry_pos];
        self.dry_delay[self.dry_pos] = x;
        self.dry_pos = (self.dry_pos + 1) % LATENCY;
        let mixed = self.mix * wet + (1.0 - self.mix) * dry;
        mixed * self.output_gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44100.0;

    fn sine(amp: f64, freq: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / SR).sin())
            .collect()
    }

    fn rms(x: &[f64]) -> f64 {
        (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn make(mode: DistortionMode, drive: f64, mix: f64, out: f64) -> Distortion {
        let mut d = Distortion::new(SR);
        d.set_mode(mode);
        d.set_param("dist.drive", drive).unwrap();
        d.set_param("dist.mix", mix).unwrap();
        d.set_param("dist.output", out).unwrap();
        d
    }

    #[test]
    fn soft_drive_zero_is_transparent_for_small_signal() {
        let mut d = make(DistortionMode::Soft, 0.0, 1.0, 0.0);
        let input = sine(0.01, 440.0, SR as usize);
        let out: Vec<f64> = input.iter().map(|&x| d.process(x)).collect();
        let skip = (0.1 * SR) as usize;
        let r_in = rms(&input[skip..]);
        let r_out = rms(&out[skip..]);
        assert!(
            ((r_out - r_in) / r_in).abs() < 0.01,
            "in {r_in} out {r_out}"
        );
    }

    #[test]
    fn hard_clip_stays_bounded() {
        let mut d = make(DistortionMode::Hard, 24.0, 1.0, 0.0);
        let input = sine(1.0, 220.0, 2 * SR as usize);
        let out: Vec<f64> = input.iter().map(|&x| d.process(x)).collect();
        let peak = out[SR as usize..]
            .iter()
            .fold(0.0f64, |m, v| m.max(v.abs()));
        // Measured peak 1.164: elliptic filter overshoot on the clipped square.
        assert!(peak <= 1.2, "peak {peak}");
    }

    #[test]
    fn overdrive_has_no_dc() {
        let mut d = make(DistortionMode::Overdrive, 12.0, 1.0, 0.0);
        let input = sine(0.5, 1000.0, 2 * SR as usize);
        let out: Vec<f64> = input.iter().map(|&x| d.process(x)).collect();
        let tail = &out[SR as usize..];
        let mean = tail.iter().sum::<f64>() / tail.len() as f64;
        assert!(mean.abs() < 1e-3, "mean {mean}");
    }

    #[test]
    fn extreme_drive_on_noise_is_finite() {
        for mode in [
            DistortionMode::Soft,
            DistortionMode::Hard,
            DistortionMode::Overdrive,
        ] {
            let mut d = make(mode, 48.0, 1.0, 6.0);
            let mut s: u64 = 0x1234_5678_9abc_def0;
            for _ in 0..100_000 {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                let x = (s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
                assert!(d.process(x).is_finite());
            }
        }
    }

    #[test]
    fn bad_params_are_rejected() {
        let mut d = Distortion::new(SR);
        assert!(matches!(
            d.set_param("dist.nope", 1.0),
            Err(ParamError::UnknownId(_))
        ));
        assert!(matches!(
            d.set_param("dist.drive", 49.0),
            Err(ParamError::OutOfRange { .. })
        ));
        assert!(matches!(
            d.set_param("dist.mix", -0.1),
            Err(ParamError::OutOfRange { .. })
        ));
        assert!(matches!(
            d.set_param("dist.output", f64::NAN),
            Err(ParamError::OutOfRange { .. })
        ));
        assert!(d.set_param("dist.output", 6.0).is_ok());
    }

    #[test]
    fn deterministic() {
        let input = sine(0.8, 330.0, 5000);
        let run = || {
            let mut d = make(DistortionMode::Overdrive, 30.0, 0.7, -3.0);
            input
                .iter()
                .map(|&x| d.process(x).to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn mix_zero_is_delayed_dry() {
        let mut d = make(DistortionMode::Hard, 48.0, 0.0, 0.0);
        let input = sine(0.7, 330.0, 2000);
        let out: Vec<f64> = input.iter().map(|&x| d.process(x)).collect();
        for i in LATENCY..input.len() {
            assert_eq!(out[i], input[i - LATENCY]);
        }
    }

    #[test]
    fn half_mix_stays_aligned() {
        // Linear region: dry and wet are in phase, so the blend keeps its level.
        let mut d = make(DistortionMode::Soft, 0.0, 0.5, 0.0);
        let input = sine(0.01, 440.0, SR as usize);
        let out: Vec<f64> = input.iter().map(|&x| d.process(x)).collect();
        let skip = (0.1 * SR) as usize;
        let ratio = rms(&out[skip..]) / rms(&input[skip..]);
        assert!((ratio - 1.0).abs() < 0.01, "ratio {ratio}");
    }
}
