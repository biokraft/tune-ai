//! The song engine: transport clock, instrument and effect traits, the
//! track wrappers around the `tune-dsp` voices, song compilation and the
//! sample-by-sample [`Engine`].

#![forbid(unsafe_code)]

pub mod clock;
pub mod compile;
pub mod engine;
pub mod instrument;
pub mod live;
pub mod mixer;
pub mod offline;
pub mod tracks;
pub mod wav;

pub use clock::{Clock, hit_seed, track_seed};
pub use compile::{
    CompiledFx, CompiledLane, CompiledSection, CompiledSong, CompiledTrack, ParamTarget,
};
pub use engine::{Engine, TrackRuntime};
pub use instrument::{Cell, Effect, Instrument, StepCtx};
pub use live::{AudioOut, LiveController, LiveError, LiveProcessor, SongUpdate, live};
pub use mixer::{CLIP_CEILING, CLIP_KNEE, Meter, pan_gains, soft_clip};
pub use offline::{RenderError, RenderOptions, RenderReport, Rendered, render, render_to_files};
pub use tracks::{AcidTrack, make_effect, make_instrument};
pub use wav::{WavError, WavFormat, WavWriter, write_wav};
