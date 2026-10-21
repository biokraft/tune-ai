//! The validated song model. Every map keeps song-file order.

use indexmap::IndexMap;
use tune_dsp::acid303::{ACID303_PARAMS, AcidPattern};
use tune_dsp::{
    CLAP_PARAMS, DIST_PARAMS, DistortionMode, DrumPattern, HAT_CLOSED_PARAMS, HAT_OPEN_PARAMS,
    KICK_PARAMS, ParamSpec, RIDE_PARAMS,
};

/// A whole song: header, tracks, patterns and the arrangement.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub name: String,
    pub bpm: f64,
    /// Seeds all noise (0 when the file leaves it out).
    pub seed: u64,
    /// `None` means the renderer's default (48 000 Hz).
    pub sample_rate: Option<u32>,
    pub tracks: IndexMap<String, Track>,
    pub patterns: IndexMap<String, Pattern>,
    pub sections: Vec<Section>,
}

impl Song {
    /// Length of the arrangement in bars.
    pub fn total_bars(&self) -> u32 {
        self.sections.iter().map(|s| s.bars).sum()
    }

    /// The 0-based bar each section starts at.
    pub fn section_start_bars(&self) -> Vec<u32> {
        self.sections
            .iter()
            .scan(0, |start, s| {
                let this = *start;
                *start += s.bars;
                Some(this)
            })
            .collect()
    }
}

/// Which voice a track plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InstrumentKind {
    Acid303,
    Kick,
    Clap,
    HatClosed,
    HatOpen,
    Ride,
}

impl InstrumentKind {
    /// Every kind, in the order error messages list them.
    pub const ALL: [Self; 6] = [
        Self::Acid303,
        Self::Kick,
        Self::Clap,
        Self::HatClosed,
        Self::HatOpen,
        Self::Ride,
    ];

    /// The `instrument` value in a song file; also the prefix of its param ids.
    pub fn name(self) -> &'static str {
        match self {
            Self::Acid303 => "acid303",
            Self::Kick => "kick",
            Self::Clap => "clap",
            Self::HatClosed => "hat_closed",
            Self::HatOpen => "hat_open",
            Self::Ride => "ride",
        }
    }

    /// Parses [`name`](Self::name).
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == s)
    }

    /// The instrument's parameter table.
    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            Self::Acid303 => ACID303_PARAMS,
            Self::Kick => KICK_PARAMS,
            Self::Clap => CLAP_PARAMS,
            Self::HatClosed => HAT_CLOSED_PARAMS,
            Self::HatOpen => HAT_OPEN_PARAMS,
            Self::Ride => RIDE_PARAMS,
        }
    }

    /// Drum tracks use the `x`/`X`/`.` step notation; acid303 uses notes.
    pub fn is_drum(self) -> bool {
        self != Self::Acid303
    }
}

/// One mixer channel: an instrument, its static params and its fx chain.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub instrument: InstrumentKind,
    /// Only the values the file sets, in plain units, keyed by full param id
    /// (e.g. `acid303.cutoff`). Everything else stays at its default.
    pub params: IndexMap<String, f64>,
    pub gain_db: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub fx: Vec<FxSpec>,
}

/// One effect in a track's chain.
#[derive(Clone, Debug, PartialEq)]
pub enum FxSpec {
    Distortion {
        mode: DistortionMode,
        /// Explicit plain values only, keyed by full param id (`dist.drive`).
        params: IndexMap<String, f64>,
    },
}

impl FxSpec {
    /// Every fx `type` name a song file accepts.
    pub const TYPE_NAMES: [&'static str; 1] = ["distortion"];

    /// The `type` value in a song file.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Distortion { .. } => "distortion",
        }
    }

    /// The effect's parameter table.
    pub fn params_table(&self) -> &'static [ParamSpec] {
        match self {
            Self::Distortion { .. } => DIST_PARAMS,
        }
    }
}

/// A named loop of steps for one track.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub track: String,
    pub steps: PatternSteps,
}

/// Steps in the notation of the pattern's track.
#[derive(Clone, Debug, PartialEq)]
pub enum PatternSteps {
    Acid(AcidPattern),
    Drum(DrumPattern),
}

/// A stretch of the arrangement.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    pub bars: u32,
    /// Track id to pattern id. A track missing here is silent in the section.
    pub play: IndexMap<String, String>,
    pub automation: Vec<Lane>,
}

/// A breakpoint line for one parameter within one section.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub track: String,
    /// Full param id of the instrument or one of the track's fx.
    pub param: String,
    /// `(bar offset from the section start, normalized 0..=1 value)`, with
    /// bar offsets that never decrease. Two equal bars form a jump.
    pub points: Vec<(f64, f64)>,
}
