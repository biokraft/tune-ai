//! The mixer: constant-power pan, the track strips (sends and ducking), the
//! shared delay and reverb buses and the master stage (gain, then the soft
//! clipper or the limiter) with its meter.

use tune_dsp::acid303::math::db2amp;
use tune_dsp::{
    DELAY_PARAMS, Delay, Ducker, Limiter, MASTER_CEILING, MASTER_GAIN, MASTER_PARAMS,
    MASTER_RELEASE, ParamSpec, REVERB_PARAMS, Reverb, STRIP_DUCK_DEPTH, STRIP_DUCK_RELEASE,
    STRIP_PARAMS, STRIP_SEND_DELAY, STRIP_SEND_REVERB, send_gain,
};
use tune_song::{MasterMode, MixUnit};

use crate::compile::{CompiledMix, CompiledSong};

/// Where the master clipper starts to bend: `db2amp(-6.0)`.
pub const CLIP_KNEE: f64 = 0.501_187_233_627_272_2;

/// The level the master clipper never exceeds: `db2amp(-1.0)`.
pub const CLIP_CEILING: f64 = 0.891_250_938_133_745_6;

/// Master soft clipper: the identity up to [`CLIP_KNEE`], then a `tanh`
/// curve that approaches [`CLIP_CEILING`].
///
/// `|x| <= K: x`, else `sign(x) * (K + (C - K) * tanh((|x| - K) / (C - K)))`.
pub fn soft_clip(x: f64) -> f64 {
    let a = x.abs();
    if a <= CLIP_KNEE {
        return x;
    }
    let span = CLIP_CEILING - CLIP_KNEE;
    let y = (CLIP_KNEE + span * ((a - CLIP_KNEE) / span).tanh()).min(CLIP_CEILING);
    y.copysign(x)
}

/// Constant-power pan gains `(left, right)` for `pan` in `-1..=1`.
///
/// `θ = (pan + 1)·π/4`, giving `(cos θ, sin θ)`. The right gain is computed
/// as `cos((1 - pan)·π/4)`, which equals `sin θ` but makes the law exactly
/// mirror-symmetric: `pan_gains(-p)` is `pan_gains(p)` swapped, bit for bit,
/// and the centre gives two equal gains.
pub fn pan_gains(pan: f64) -> (f64, f64) {
    let quarter = std::f64::consts::FRAC_PI_4;
    (((pan + 1.0) * quarter).cos(), ((1.0 - pan) * quarter).cos())
}

/// Master level statistics since the last reset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meter {
    /// Peak of the master stage's output (what is written), either channel.
    pub peak: f64,
    /// Peak of the master bus after the master gain, before the clipper or
    /// the limiter, either channel.
    pub pre_clip_peak: f64,
    /// Clip mode: frames in which either channel of the master bus exceeded
    /// [`CLIP_KNEE`]. Limit mode: frames the limiter turned down.
    pub knee_samples: u64,
    /// The limiter's deepest gain reduction in dB (0 in clip mode).
    pub max_reduction_db: f64,
}

impl Meter {
    /// Records one master frame and returns it clipped.
    pub fn clip_frame(&mut self, l: f64, r: f64) -> (f64, f64) {
        let pre = l.abs().max(r.abs());
        self.pre_clip_peak = self.pre_clip_peak.max(pre);
        if pre > CLIP_KNEE {
            self.knee_samples += 1;
        }
        self.written(soft_clip(l), soft_clip(r))
    }

    /// Records one master frame, runs it through `limiter` and returns the
    /// limiter's output (the frame from its latency ago).
    pub fn limit_frame(&mut self, limiter: &mut Limiter, l: f64, r: f64) -> (f64, f64) {
        let (ol, or) = self.limited(limiter, l, r);
        self.written(ol, or)
    }

    /// The limiter half of [`limit_frame`](Self::limit_frame): every
    /// statistic but the output peak.
    fn limited(&mut self, limiter: &mut Limiter, l: f64, r: f64) -> (f64, f64) {
        self.pre_clip_peak = self.pre_clip_peak.max(l.abs().max(r.abs()));
        let (ol, or, gain) = limiter.process(l, r);
        if gain < 1.0 {
            self.knee_samples += 1;
            self.max_reduction_db = self.max_reduction_db.max(-20.0 * gain.log10());
        }
        (ol, or)
    }

    /// Records the frame the master stage writes and returns it.
    fn written(&mut self, l: f64, r: f64) -> (f64, f64) {
        self.peak = self.peak.max(l.abs().max(r.abs()));
        (l, r)
    }
}

/// How long a master-mode switch crossfades, in milliseconds.
pub const MASTER_FADE_MS: f64 = 10.0;

/// A master-mode switch in progress: the clipper and the limiter both run
/// while the output crossfades from one to the other.
#[derive(Clone, Copy, Debug)]
struct MasterFade {
    /// The mode faded to.
    to: MasterMode,
    /// Frames still to play from the mode faded from before the crossfade
    /// starts: a fresh limiter's latency, while its lookahead fills.
    wait: usize,
    /// Crossfade frames done, out of `len`.
    done: usize,
    len: usize,
}

/// The strip of one track: its two send gains and its ducker, set by
/// [`STRIP_PARAMS`] index.
#[derive(Clone, Debug)]
pub struct Strip {
    /// Linear gain into the delay bus (exactly 0 when off).
    send_delay: f64,
    /// Linear gain into the reverb bus (exactly 0 when off).
    send_reverb: f64,
    ducker: Ducker,
}

impl Strip {
    /// Every strip param at its default: sends off, no ducking.
    pub fn new(sample_rate: f64) -> Self {
        let mut strip = Self {
            send_delay: 0.0,
            send_reverb: 0.0,
            ducker: Ducker::new(sample_rate),
        };
        for (index, spec) in STRIP_PARAMS.iter().enumerate() {
            strip.set_param_at(index, spec.default);
        }
        strip
    }

    /// Sets the param at `index` of [`STRIP_PARAMS`] in plain units, clamped
    /// to its range (NaN is the minimum). An index outside the table is
    /// ignored.
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let Some(spec) = STRIP_PARAMS.get(index) else {
            return;
        };
        let plain = if plain.is_nan() {
            spec.min
        } else {
            plain.clamp(spec.min, spec.max)
        };
        match index {
            STRIP_SEND_DELAY => self.send_delay = send_gain(plain),
            STRIP_SEND_REVERB => self.send_reverb = send_gain(plain),
            STRIP_DUCK_DEPTH => self.ducker.set_depth_db(plain),
            STRIP_DUCK_RELEASE => self.ducker.set_release_ms(plain),
            _ => {}
        }
    }

    /// Restarts the duck envelope (a hit of the duck source).
    pub fn trigger_duck(&mut self) {
        self.ducker.trigger();
    }

    /// `y` ducked for this frame. The envelope advances every frame, even at
    /// depth 0, so a depth raised mid-release follows the kick grid; at
    /// depth 0, `y` passes untouched.
    #[inline]
    pub fn duck(&mut self, y: f64) -> f64 {
        let gain = self.ducker.next_gain();
        if self.ducker.is_active() { y * gain } else { y }
    }

    /// Linear `(delay, reverb)` send gains.
    pub fn sends(&self) -> (f64, f64) {
        (self.send_delay, self.send_reverb)
    }

    /// Back to idle; params stay.
    pub fn reset(&mut self) {
        self.ducker.reset();
    }
}

/// The live mix units after the tracks: the delay and reverb buses (built
/// only when the song sends to them), the limiter (limit mode only), the
/// master gain and the knob overrides on mix params.
pub struct MixRuntime {
    delay: Option<Delay>,
    reverb: Option<Reverb>,
    /// Built in limit mode. After a switch to clip mode it stays (idle) so
    /// the crossfade can play out; the next swap that ends clip mode or
    /// keeps it hands it back as garbage.
    limiter: Option<Limiter>,
    /// A master-mode switch being crossfaded.
    fade: Option<MasterFade>,
    /// Crossfade length in frames ([`MASTER_FADE_MS`]).
    fade_len: usize,
    /// Linear master gain (`db2amp(master.gain)`).
    master_gain: f64,
    /// One override per param of each unit in plain units, indexed by
    /// [`unit_slot`]; `NaN` is none.
    overrides: [Vec<f64>; 3],
}

/// Index of `unit` in [`MixRuntime::overrides`].
fn unit_slot(unit: MixUnit) -> usize {
    match unit {
        MixUnit::Delay => 0,
        MixUnit::Reverb => 1,
        MixUnit::Master => 2,
    }
}

impl MixRuntime {
    /// The units `song.mix` asks for, with its static params set. Allocates
    /// every buffer: build off the audio thread.
    pub fn build(song: &CompiledSong) -> Self {
        let sample_rate = f64::from(song.clock.sample_rate);
        let mix = &song.mix;
        let mut rt = Self {
            delay: mix
                .delay
                .as_ref()
                .map(|_| Delay::new(sample_rate, song.clock.samples_per_step)),
            reverb: mix.reverb.as_ref().map(|_| Reverb::new(sample_rate)),
            limiter: (mix.master == MasterMode::Limit).then(|| Limiter::new(sample_rate)),
            fade: None,
            fade_len: ((MASTER_FADE_MS * 0.001 * sample_rate).round() as usize).max(1),
            master_gain: 1.0,
            overrides: [
                vec![f64::NAN; DELAY_PARAMS.len()],
                vec![f64::NAN; REVERB_PARAMS.len()],
                vec![f64::NAN; MASTER_PARAMS.len()],
            ],
        };
        rt.apply_static(mix);
        rt
    }

    /// Sets every param of the built units to its static value in `mix`. A
    /// unit `mix` lacks (or the runtime lacks) is left alone.
    pub(crate) fn apply_static(&mut self, mix: &CompiledMix) {
        for (unit, table) in [
            (MixUnit::Delay, mix.delay.as_deref()),
            (MixUnit::Reverb, mix.reverb.as_deref()),
            (MixUnit::Master, Some(mix.master_params.as_slice())),
        ] {
            for (index, &plain) in table.unwrap_or(&[]).iter().enumerate() {
                self.set_target(unit, index, plain);
            }
        }
    }

    /// The static value of `unit`'s param `index` in `mix`, if it has one.
    fn static_value(mix: &CompiledMix, unit: MixUnit, index: usize) -> Option<f64> {
        let table = match unit {
            MixUnit::Delay => mix.delay.as_deref()?,
            MixUnit::Reverb => mix.reverb.as_deref()?,
            MixUnit::Master => &mix.master_params,
        };
        table.get(index).copied()
    }

    /// Sets one param in plain units; a unit that is not built ignores it.
    pub(crate) fn set_target(&mut self, unit: MixUnit, index: usize, plain: f64) {
        match unit {
            MixUnit::Delay => {
                if let Some(d) = &mut self.delay
                    && index < DELAY_PARAMS.len()
                {
                    d.set_param_at(index, plain);
                }
            }
            MixUnit::Reverb => {
                if let Some(r) = &mut self.reverb
                    && index < REVERB_PARAMS.len()
                {
                    r.set_param_at(index, plain);
                }
            }
            MixUnit::Master => match index {
                MASTER_GAIN => self.master_gain = db2amp(plain),
                MASTER_CEILING => {
                    if let Some(l) = &mut self.limiter {
                        l.set_ceiling_db(plain);
                    }
                }
                MASTER_RELEASE => {
                    if let Some(l) = &mut self.limiter {
                        l.set_release_ms(plain);
                    }
                }
                _ => {}
            },
        }
    }

    /// The override slot of `unit`'s param `index` and its spec.
    pub(crate) fn override_slot(
        &mut self,
        unit: MixUnit,
        index: usize,
    ) -> Option<(&mut f64, &'static ParamSpec)> {
        let spec = unit.params().get(index)?;
        Some((self.overrides[unit_slot(unit)].get_mut(index)?, spec))
    }

    /// Sets every overridden param to its override.
    pub(crate) fn apply_overrides(&mut self) {
        for unit in MixUnit::ALL {
            let slot = unit_slot(unit);
            for index in 0..self.overrides[slot].len() {
                let plain = self.overrides[slot][index];
                if !plain.is_nan() {
                    self.set_target(unit, index, plain);
                }
            }
        }
    }

    /// Clears the override of `unit`'s param `index`, setting the param back
    /// to its static value in `mix`.
    pub(crate) fn restore(&mut self, mix: &CompiledMix, unit: MixUnit, index: usize) {
        if let Some(plain) = Self::static_value(mix, unit, index) {
            self.set_target(unit, index, plain);
        }
    }

    /// Clears every override, setting each param it held back to its static
    /// value in `mix`.
    pub(crate) fn clear_overrides(&mut self, mix: &CompiledMix) {
        for unit in MixUnit::ALL {
            let slot = unit_slot(unit);
            for index in 0..self.overrides[slot].len() {
                if !self.overrides[slot][index].is_nan() {
                    self.overrides[slot][index] = f64::NAN;
                    self.restore(mix, unit, index);
                }
            }
        }
    }

    /// How many overrides are set.
    pub(crate) fn overrides_set(&self) -> u32 {
        self.overrides
            .iter()
            .map(|t| t.iter().filter(|v| !v.is_nan()).count() as u32)
            .sum()
    }

    /// Takes over from `fresh`, built for `new`, each unit this runtime,
    /// built for `old`, cannot serve, and hands back in `fresh` what it
    /// gives up. Moves only: it never allocates or frees. Params and knob
    /// overrides stay here; the static reset after a swap sets the params.
    ///
    /// - The delay is replaced when the bus appears or goes, or when the
    ///   step length changes (its buffer is sized from it): its echoes stop.
    /// - The reverb is replaced only when the bus appears or goes, so its
    ///   tail rings on through a tempo change.
    /// - The limiter stays while both songs limit. A switch to the limiter
    ///   takes the fresh one, plays the clipper while its lookahead fills
    ///   and then crossfades to it over [`MASTER_FADE_MS`]; a switch to the
    ///   clipper crossfades the other way. Neither drops a frame.
    ///
    /// Returns whether every unit stayed.
    pub(crate) fn take_units(
        &mut self,
        fresh: &mut MixRuntime,
        old: &CompiledSong,
        new: &CompiledSong,
    ) -> bool {
        let (a, b) = (&old.mix, &new.mix);
        let keep_delay = a.delay.is_some() == b.delay.is_some()
            && (b.delay.is_none()
                || old.clock.samples_per_step.to_bits() == new.clock.samples_per_step.to_bits());
        if !keep_delay {
            std::mem::swap(&mut self.delay, &mut fresh.delay);
        }
        let keep_reverb = a.reverb.is_some() == b.reverb.is_some();
        if !keep_reverb {
            std::mem::swap(&mut self.reverb, &mut fresh.reverb);
        }
        let len = self.fade_len;
        let fade = |to, wait, done| {
            Some(MasterFade {
                to,
                wait,
                done,
                len,
            })
        };
        match (a.master, b.master) {
            (MasterMode::Limit, MasterMode::Limit) => {}
            (MasterMode::Limit, MasterMode::Clip) => {
                self.fade = match self.fade {
                    // Back before the crossfade began: the clipper was
                    // playing all along.
                    Some(f) if f.wait > 0 => None,
                    Some(f) => fade(MasterMode::Clip, 0, len - f.done),
                    None if self.limiter.is_some() => fade(MasterMode::Clip, 0, 0),
                    None => None,
                };
            }
            (MasterMode::Clip, MasterMode::Limit) => match self.fade {
                // Back while fading to the clipper: the limiter has kept
                // running, so fade back to it from where the fade is.
                Some(f) => self.fade = fade(MasterMode::Limit, 0, len - f.done),
                None => {
                    std::mem::swap(&mut self.limiter, &mut fresh.limiter);
                    let wait = self.limiter.as_ref().map(Limiter::latency);
                    self.fade = wait.and_then(|w| fade(MasterMode::Limit, w, 0));
                }
            },
            (MasterMode::Clip, MasterMode::Clip) => {
                // An idle limiter left by an earlier switch goes back now,
                // unless its crossfade is still playing.
                if self.fade.is_none() {
                    std::mem::swap(&mut self.limiter, &mut fresh.limiter);
                }
            }
        }
        keep_delay && keep_reverb && a.master == b.master
    }

    /// Whether the delay bus is built.
    pub fn has_delay(&self) -> bool {
        self.delay.is_some()
    }

    /// Whether the reverb bus is built.
    pub fn has_reverb(&self) -> bool {
        self.reverb.is_some()
    }

    /// Adds the bus returns for this frame's sends to the master bus: the
    /// delay's, then the reverb's. A bus that is not built is off.
    #[inline]
    pub(crate) fn add_returns(
        &mut self,
        ml: &mut f64,
        mr: &mut f64,
        delay_in: f64,
        reverb_in: f64,
    ) {
        if let Some(d) = &mut self.delay {
            let (dl, dr) = d.process(delay_in);
            *ml += dl;
            *mr += dr;
        }
        if let Some(r) = &mut self.reverb {
            let (rl, rr) = r.process(reverb_in);
            *ml += rl;
            *mr += rr;
        }
    }

    /// The master stage: gain, then the limiter when `master` is
    /// [`MasterMode::Limit`] and one is built, else the soft clipper.
    #[inline]
    pub(crate) fn master(
        &mut self,
        master: MasterMode,
        meter: &mut Meter,
        ml: f64,
        mr: f64,
    ) -> (f64, f64) {
        let (l, r) = (ml * self.master_gain, mr * self.master_gain);
        if let (Some(fade), Some(limiter)) = (&mut self.fade, &mut self.limiter) {
            let out = fade.frame(limiter, meter, l, r);
            if fade.done == fade.len {
                self.fade = None;
            }
            return out;
        }
        match (master, &mut self.limiter) {
            (MasterMode::Limit, Some(limiter)) => meter.limit_frame(limiter, l, r),
            _ => meter.clip_frame(l, r),
        }
    }

    /// Clears every bus and the limiter and ends a master crossfade; params
    /// and overrides stay.
    pub fn reset(&mut self) {
        self.fade = None;
        if let Some(d) = &mut self.delay {
            d.reset();
        }
        if let Some(r) = &mut self.reverb {
            r.reset();
        }
        if let Some(l) = &mut self.limiter {
            l.reset();
        }
    }
}

impl MasterFade {
    /// One master frame of the crossfade: both stages run, the output moves
    /// linearly from the mode faded from to `to`, reaching it on the last
    /// frame. Linear, because both sides
    /// carry the same music: their sum never exceeds the louder of the two.
    fn frame(&mut self, limiter: &mut Limiter, meter: &mut Meter, l: f64, r: f64) -> (f64, f64) {
        let limited = meter.limited(limiter, l, r);
        let clipped = (soft_clip(l), soft_clip(r));
        let (from, to) = match self.to {
            MasterMode::Limit => (clipped, limited),
            MasterMode::Clip => (limited, clipped),
        };
        if self.wait > 0 {
            self.wait -= 1;
            return meter.written(from.0, from.1);
        }
        self.done += 1;
        let a = self.done as f64 / self.len as f64;
        meter.written(from.0 + a * (to.0 - from.0), from.1 + a * (to.1 - from.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tune_dsp::acid303::math::db2amp;

    #[test]
    fn constants_match_db2amp() {
        assert_eq!(CLIP_KNEE, db2amp(-6.0));
        assert_eq!(CLIP_CEILING, db2amp(-1.0));
    }

    #[test]
    fn soft_clip_shape() {
        assert_eq!(soft_clip(0.25), 0.25);
        assert_eq!(soft_clip(-0.25), -0.25);
        assert!(soft_clip(1e6) <= CLIP_CEILING);
        assert!(soft_clip(-1e6) >= -CLIP_CEILING);
        assert!(soft_clip(f64::INFINITY) <= CLIP_CEILING);
        let x = CLIP_KNEE + 1e-9;
        assert!((soft_clip(x) - x).abs() < 1e-12);
        assert_eq!(soft_clip(CLIP_KNEE), CLIP_KNEE);
    }

    #[test]
    fn soft_clip_monotonic() {
        let n = 10_000;
        let mut prev = f64::NEG_INFINITY;
        for i in 0..=n {
            let x = -10.0 + 20.0 * f64::from(i) / f64::from(n);
            let y = soft_clip(x);
            assert!(y >= prev, "not monotonic at {x}: {y} < {prev}");
            assert!(y.abs() <= CLIP_CEILING);
            prev = y;
        }
    }

    #[test]
    fn pan_constant_power() {
        // 0.7071067811865476 on both sides.
        let centre = std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(pan_gains(0.0), (centre, centre));
        assert!(pan_gains(1.0).0 < 1e-15);
        assert!(pan_gains(-1.0).1 < 1e-15);
        for i in -10..=10 {
            let pan = f64::from(i) * 0.1;
            let (l, r) = pan_gains(pan);
            assert!((l * l + r * r - 1.0).abs() < 1e-12, "pan {pan}");
            let (ml, mr) = pan_gains(-pan);
            assert_eq!((ml.to_bits(), mr.to_bits()), (r.to_bits(), l.to_bits()));
        }
        let (l, r) = pan_gains(0.3);
        assert!(r > l);
    }

    #[test]
    fn meter_tracks_peaks_and_knee() {
        let mut m = Meter::default();
        assert_eq!(m.clip_frame(0.1, -0.2), (0.1, -0.2));
        m.clip_frame(2.0, 0.0);
        m.clip_frame(0.0, -0.6);
        assert_eq!(m.pre_clip_peak, 2.0);
        assert_eq!(m.knee_samples, 2);
        assert!(m.peak > CLIP_KNEE && m.peak <= CLIP_CEILING);
    }
}
