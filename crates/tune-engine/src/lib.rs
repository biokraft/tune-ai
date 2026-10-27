//! The song engine: transport clock, song compilation and the
//! sample-by-sample [`Engine`] over the `tune-modules` instruments and
//! effects.

#![forbid(unsafe_code)]

pub mod clock;
pub mod compile;
pub mod engine;
pub mod live;
pub mod mixer;
pub mod offline;
pub mod wav;

pub use clock::{Clock, hit_seed, track_seed};
pub use compile::{
    Cell, CellSource, ClipEvent, CompiledClip, CompiledDevice, CompiledLane, CompiledMix,
    CompiledRegion, CompiledSection, CompiledSong, CompiledTrack, MIX_TRACK, NOTE_CLIP_IDS,
    NoteClipEvents, ParamTarget,
};
pub use engine::{Engine, TrackRuntime};
pub use live::{AudioOut, LiveController, LiveError, LiveProcessor, SongUpdate, live};
pub use mixer::{CLIP_CEILING, CLIP_KNEE, Meter, MixRuntime, Strip, pan_gains, soft_clip};
pub use offline::{RenderError, RenderOptions, RenderReport, Rendered, render, render_to_files};
pub use tune_modules::{AcidTrack, Effect, EventCtx, Instrument, NoteEvent, NoteId};
pub use wav::{WavError, WavFormat, WavWriter, write_wav};
