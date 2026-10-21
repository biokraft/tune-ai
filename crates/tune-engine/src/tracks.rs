//! Track wrappers: the `tune-dsp` voices behind the engine's traits.

use tune_dsp::acid303::{ACID303_PARAMS, Acid303, AcidGate, AcidStep};
use tune_dsp::{
    Clap, DIST_PARAMS, Distortion, EQ_PARAMS, Equalizer, Hat, HatKind, Kick, ParamSpec,
};
use tune_song::{FxSpec, InstrumentKind};

use crate::instrument::{Cell, Effect, Instrument, StepCtx};

/// An [`Acid303`] driven by the song clock instead of its own sequencer.
///
/// `process` keeps `AcidPlayer`'s order: gate tick, then the pending step,
/// then the voice sample.
pub struct AcidTrack {
    voice: Acid303,
    gate: AcidGate,
    pending: Option<(AcidStep, StepCtx)>,
}

impl AcidTrack {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            voice: Acid303::with_param_defaults(sample_rate),
            gate: AcidGate::new(),
            pending: None,
        }
    }
}

impl Instrument for AcidTrack {
    fn params(&self) -> &'static [ParamSpec] {
        ACID303_PARAMS
    }

    fn set_param_at(&mut self, index: usize, plain: f64) {
        self.voice.set_param_at(index, plain);
    }

    fn on_step(&mut self, cell: &Cell, ctx: &StepCtx) {
        self.pending = match cell {
            Cell::Note(step) => Some((step.clone(), *ctx)),
            // A rest (or a stray drum hit) still ends a pending slide.
            Cell::Rest | Cell::Hit { .. } => Some((
                AcidStep {
                    note: None,
                    accent: false,
                    slide: false,
                },
                *ctx,
            )),
        };
    }

    fn process(&mut self) -> f64 {
        self.gate.tick(&mut self.voice);
        if let Some((step, ctx)) = self.pending.take() {
            self.gate
                .on_step(&mut self.voice, &step, ctx.next_has_gate, ctx.gate_samples);
        }
        self.voice.get_sample()
    }

    fn reset(&mut self) {
        self.voice.reset();
        self.gate.reset();
        self.pending = None;
    }
}

macro_rules! drum_instrument {
    ($ty:ty, $params:expr) => {
        impl Instrument for $ty {
            fn params(&self) -> &'static [ParamSpec] {
                $params
            }
            fn set_param_at(&mut self, index: usize, plain: f64) {
                <$ty>::set_param_at(self, index, plain);
            }
            fn on_step(&mut self, cell: &Cell, ctx: &StepCtx) {
                if let Cell::Hit { accent } = cell {
                    self.trigger(*accent, ctx.seed);
                }
            }
            fn process(&mut self) -> f64 {
                self.get_sample()
            }
            fn reset(&mut self) {
                <$ty>::reset(self);
            }
        }
    };
}

drum_instrument!(Kick, tune_dsp::KICK_PARAMS);
drum_instrument!(Clap, tune_dsp::CLAP_PARAMS);

impl Instrument for Hat {
    fn params(&self) -> &'static [ParamSpec] {
        Hat::params(self)
    }
    fn set_param_at(&mut self, index: usize, plain: f64) {
        Hat::set_param_at(self, index, plain);
    }
    fn on_step(&mut self, cell: &Cell, ctx: &StepCtx) {
        if let Cell::Hit { accent } = cell {
            self.trigger(*accent, ctx.seed);
        }
    }
    fn process(&mut self) -> f64 {
        self.get_sample()
    }
    fn reset(&mut self) {
        Hat::reset(self);
    }
}

impl Effect for Distortion {
    fn params(&self) -> &'static [ParamSpec] {
        DIST_PARAMS
    }
    fn set_param_at(&mut self, index: usize, plain: f64) {
        Distortion::set_param_at(self, index, plain);
    }
    fn process(&mut self, x: f64) -> f64 {
        Distortion::process(self, x)
    }
    fn reset(&mut self) {
        Distortion::reset(self);
    }
}

impl Effect for Equalizer {
    fn params(&self) -> &'static [ParamSpec] {
        EQ_PARAMS
    }
    fn set_param_at(&mut self, index: usize, plain: f64) {
        Equalizer::set_param_at(self, index, plain);
    }
    fn process(&mut self, x: f64) -> f64 {
        Equalizer::process(self, x)
    }
    fn reset(&mut self) {
        Equalizer::reset(self);
    }
}

/// A fresh instrument of `kind` with every parameter at its default.
pub fn make_instrument(kind: InstrumentKind, sample_rate: f64) -> Box<dyn Instrument> {
    match kind {
        InstrumentKind::Acid303 => Box::new(AcidTrack::new(sample_rate)),
        InstrumentKind::Kick => Box::new(Kick::new(sample_rate)),
        InstrumentKind::Clap => Box::new(Clap::new(sample_rate)),
        InstrumentKind::HatClosed => Box::new(Hat::new(HatKind::Closed, sample_rate)),
        InstrumentKind::HatOpen => Box::new(Hat::new(HatKind::Open, sample_rate)),
        InstrumentKind::Ride => Box::new(Hat::new(HatKind::Ride, sample_rate)),
    }
}

/// A fresh effect for `fx` with its mode set; params stay at their defaults.
pub fn make_effect(fx: &FxSpec, sample_rate: f64) -> Box<dyn Effect> {
    match fx {
        FxSpec::Distortion { mode, .. } => {
            let mut d = Distortion::new(sample_rate);
            d.set_mode(*mode);
            Box::new(d)
        }
        FxSpec::Eq { .. } => Box::new(Equalizer::new(sample_rate)),
    }
}
