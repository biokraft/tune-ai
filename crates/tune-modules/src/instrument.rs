//! The traits every instrument and effect implements.

use tune_dsp::acid303::AcidStep;
use tune_dsp::{ParamError, ParamSpec, find_spec};

/// One step of a track's pattern, as the engine hands it to an instrument.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Rest,
    /// A drum hit.
    Hit {
        accent: bool,
    },
    /// An acid303 step.
    Note(AcidStep),
}

/// Timing and context for a step boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepCtx {
    /// Absolute song step.
    pub step: u64,
    /// Whether the following step holds a note (a slide holds the gate only then).
    pub next_has_gate: bool,
    /// Gate length in samples.
    pub gate_samples: i32,
    /// Noise seed for this step (`hit_seed`).
    pub seed: u64,
}

/// A mono sound source driven step by step.
pub trait Instrument: Send {
    fn params(&self) -> &'static [ParamSpec];

    /// Sets the parameter at `index` of [`params`](Self::params), in plain units.
    fn set_param_at(&mut self, index: usize, plain: f64);

    /// Sets a parameter by id, rejecting unknown ids and out-of-range values.
    fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(self.params(), id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Called at a step boundary, before [`process`](Self::process) of the same sample.
    fn on_step(&mut self, cell: &Cell, ctx: &StepCtx);

    /// One mono sample.
    fn process(&mut self) -> f64;

    fn reset(&mut self);
}

/// A mono effect.
pub trait Effect: Send {
    fn params(&self) -> &'static [ParamSpec];
    fn set_param_at(&mut self, index: usize, plain: f64);

    fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(self.params(), id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    fn process(&mut self, x: f64) -> f64;
    fn reset(&mut self);
}
