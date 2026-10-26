//! The studio's commands: what the app (or a browser tab) asks it to do.
//!
//! One serde form serves every transport: `{"cmd": "<snake_case name>",
//! "args": {…}}`, with `args` left out for commands that take none.

use serde::{Deserialize, Serialize};
use tune_dsp::DrumHit;

/// One request to the studio. Edits change the song; the rest drive the
/// transport, the song choice and the undo history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
pub enum Command {
    /// The songs under the studio's songs root.
    ListSongs,
    /// The current [`StudioState`](crate::StudioState), as the call's
    /// result. Changes nothing; a client calls it right after subscribing,
    /// in case it missed the first `state` event.
    GetState,
    OpenSong {
        path: String,
    },
    /// Starts playback at `from_bar` (0-based), else at the cursor.
    Play {
        from_bar: Option<u32>,
    },
    Stop,
    SeekBar {
        bar: u32,
    },
    /// Loops the half-open bar range `[start, end)`, or stops looping.
    SetLoop {
        bars: Option<[u32; 2]>,
    },
    /// A knob. `commit: false` is a live drag that is heard but not written;
    /// `commit: true` (the release) writes the value, rounded to 4
    /// significant digits.
    SetParam {
        track: String,
        param: String,
        value: f64,
        commit: bool,
    },
    /// Sets the `mode` setting of the track's first fx device that has one
    /// (`tracks.<t>.<device>.mode`).
    SetFxMode {
        track: String,
        mode: String,
    },
    /// Sets one step of an acid pattern. A rest (`note: None`) carries no
    /// accent or slide.
    SetAcidStep {
        pattern: String,
        index: usize,
        note: Option<u8>,
        accent: bool,
        slide: bool,
    },
    SetDrumStep {
        pattern: String,
        index: usize,
        hit: DrumHitDto,
    },
    /// Moves section `from` to index `to` (remove, then insert).
    MoveSection {
        from: usize,
        to: usize,
    },
    ResizeSection {
        index: usize,
        bars: u32,
    },
    DuplicateSection {
        index: usize,
    },
    DeleteSection {
        index: usize,
    },
    /// Which pattern a track plays in a section; `None` silences it there.
    SetSectionPlay {
        section: usize,
        track: String,
        pattern: Option<String>,
    },
    /// Sets each given mixer field; numbers are rounded to 4 significant
    /// digits.
    SetTrackMix {
        track: String,
        gain_db: Option<f64>,
        pan: Option<f64>,
        mute: Option<bool>,
        solo: Option<bool>,
    },
    Undo,
    Redo,
    /// Re-applies (`true`) or drops the local edits an external change
    /// overrode.
    ResolveOffered {
        reapply: bool,
    },
}

/// One drum step on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DrumHitDto {
    Rest,
    Hit,
    Accent,
}

impl From<DrumHit> for DrumHitDto {
    fn from(h: DrumHit) -> Self {
        match h {
            DrumHit::Rest => Self::Rest,
            DrumHit::Hit => Self::Hit,
            DrumHit::Accent => Self::Accent,
        }
    }
}

impl From<DrumHitDto> for DrumHit {
    fn from(h: DrumHitDto) -> Self {
        match h {
            DrumHitDto::Rest => Self::Rest,
            DrumHitDto::Hit => Self::Hit,
            DrumHitDto::Accent => Self::Accent,
        }
    }
}
