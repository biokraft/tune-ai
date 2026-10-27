//! The traits every instrument and effect implements.

use tune_dsp::{ParamError, ParamSpec, find_spec};

/// One note from its On to its Off; unique among the notes sounding on a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteId(pub u32);

/// The pitch drum steps carry (C4); drum modules ignore pitch.
pub const DRUM_PITCH: u8 = 60;
/// A note-clip note at or above this velocity is accented.
pub const ACCENT_VELOCITY: u8 = 110;

/// What the engine tells an instrument about its notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteEvent {
    /// `slide`: the engine sends the next note's On before this note's Off
    /// (a 303 slide or tie, or a note-clip note that overlaps a later one).
    On {
        id: NoteId,
        pitch: u8,
        velocity: u8,
        accent: bool,
        slide: bool,
    },
    /// Ends note `id`. An instrument ignores an Off for a note it is not sounding.
    Off { id: NoteId, pitch: u8 },
    /// Ends every sounding note: a placement end, a loop wrap or a swap cut a note clip.
    AllOff,
}

/// Where and when a [`NoteEvent`] lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCtx {
    /// Song sample the event lands on; it is delivered just before `process` of that sample.
    pub pos: u64,
    /// The song step the event falls in.
    pub step: u64,
    /// `hit_seed(track seed, step)`, xor the event's tick within the step (0 on the grid).
    pub seed: u64,
}

/// A mono sound source played by note events.
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

    /// Receives a note event, before [`process`](Self::process) of the
    /// sample it lands on (`ctx.pos`).
    fn note(&mut self, event: &NoteEvent, ctx: &EventCtx);

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
