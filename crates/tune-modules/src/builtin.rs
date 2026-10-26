//! Today's modules: the `tune-dsp` voices behind the instrument and effect
//! traits, and their descriptors.

use tune_dsp::acid303::{ACID303_PARAMS, Acid303, AcidGate, AcidStep};
use tune_dsp::{
    CLAP_PARAMS, Clap, DIST_PARAMS, Distortion, DistortionMode, EQ_PARAMS, Equalizer,
    HAT_CLOSED_PARAMS, HAT_OPEN_PARAMS, Hat, HatKind, KICK_PARAMS, Kick, ParamSpec, RIDE_PARAMS,
};

use crate::descriptor::{ClipKind, ModuleDescriptor, Role, SettingSpec};
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

drum_instrument!(Kick, KICK_PARAMS);
drum_instrument!(Clap, CLAP_PARAMS);

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

const ACID: &[ClipKind] = &[ClipKind::Acid];
const DRUM: &[ClipKind] = &[ClipKind::Drum];

pub static ACID303: ModuleDescriptor = ModuleDescriptor {
    name: "acid303",
    prefix: "acid303",
    params: ACID303_PARAMS,
    settings: &[],
    role: Role::Instrument {
        plays: ACID,
        default_duck_source: false,
        make: |sample_rate, _| Box::new(AcidTrack::new(sample_rate)),
    },
};

pub static KICK: ModuleDescriptor = ModuleDescriptor {
    name: "kick",
    prefix: "kick",
    params: KICK_PARAMS,
    settings: &[],
    role: Role::Instrument {
        plays: DRUM,
        default_duck_source: true,
        make: |sample_rate, _| Box::new(Kick::new(sample_rate)),
    },
};

pub static CLAP: ModuleDescriptor = ModuleDescriptor {
    name: "clap",
    prefix: "clap",
    params: CLAP_PARAMS,
    settings: &[],
    role: Role::Instrument {
        plays: DRUM,
        default_duck_source: false,
        make: |sample_rate, _| Box::new(Clap::new(sample_rate)),
    },
};

pub static HAT_CLOSED: ModuleDescriptor = ModuleDescriptor {
    name: "hat_closed",
    prefix: "hat_closed",
    params: HAT_CLOSED_PARAMS,
    settings: &[],
    role: Role::Instrument {
        plays: DRUM,
        default_duck_source: false,
        make: |sample_rate, _| Box::new(Hat::new(HatKind::Closed, sample_rate)),
    },
};

pub static HAT_OPEN: ModuleDescriptor = ModuleDescriptor {
    name: "hat_open",
    prefix: "hat_open",
    params: HAT_OPEN_PARAMS,
    settings: &[],
    role: Role::Instrument {
        plays: DRUM,
        default_duck_source: false,
        make: |sample_rate, _| Box::new(Hat::new(HatKind::Open, sample_rate)),
    },
};

pub static RIDE: ModuleDescriptor = ModuleDescriptor {
    name: "ride",
    prefix: "ride",
    params: RIDE_PARAMS,
    settings: &[],
    role: Role::Instrument {
        plays: DRUM,
        default_duck_source: false,
        make: |sample_rate, _| Box::new(Hat::new(HatKind::Ride, sample_rate)),
    },
};

pub static DISTORTION: ModuleDescriptor = ModuleDescriptor {
    name: "distortion",
    prefix: "dist",
    params: DIST_PARAMS,
    settings: &[SettingSpec {
        name: "mode",
        values: &["soft", "hard", "overdrive"],
        default: "soft",
        help: "Soft rounds the peaks off, hard clips them flat, overdrive adds asymmetric grit.",
    }],
    role: Role::Effect {
        make: |sample_rate, settings| {
            let mut d = Distortion::new(sample_rate);
            d.set_mode(
                DistortionMode::from_name(settings[0])
                    .expect("the distortion `mode` is one of its setting values"),
            );
            Box::new(d)
        },
    },
};

pub static EQ: ModuleDescriptor = ModuleDescriptor {
    name: "eq",
    prefix: "eq",
    params: EQ_PARAMS,
    settings: &[],
    role: Role::Effect {
        make: |sample_rate, _| Box::new(Equalizer::new(sample_rate)),
    },
};
