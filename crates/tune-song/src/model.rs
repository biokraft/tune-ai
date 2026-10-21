//! The validated song model. Every map keeps song-file order.

use indexmap::IndexMap;
use tune_dsp::acid303::{ACID303_PARAMS, AcidPattern};
use tune_dsp::{
    CLAP_PARAMS, DELAY_PARAMS, DIST_PARAMS, DistortionMode, DrumPattern, EQ_PARAMS,
    HAT_CLOSED_PARAMS, HAT_OPEN_PARAMS, KICK_PARAMS, MASTER_PARAMS, ParamSpec, REVERB_PARAMS,
    RIDE_PARAMS, STRIP_PARAMS, STRIP_SEND_DELAY, STRIP_SEND_REVERB, find_spec,
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
    /// The `[mix]` table: master mode, duck source and bus params.
    pub mix: Mix,
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

    /// The track every `duck.depth` follows: the explicit `duck_source`, else
    /// the first `kick` track; `None` when there is neither.
    pub fn duck_source(&self) -> Option<&str> {
        match &self.mix.duck_source {
            Some(id) => Some(id.as_str()),
            None => self
                .tracks
                .iter()
                .find(|(_, t)| t.instrument == InstrumentKind::Kick)
                .map(|(id, _)| id.as_str()),
        }
    }

    /// Whether the song needs `unit` built. Delay/Reverb: some track sets
    /// `send.<unit>` explicitly or a lane drives it. Master: always true.
    pub fn bus_active(&self, unit: MixUnit) -> bool {
        let send = match unit {
            MixUnit::Delay => STRIP_PARAMS[STRIP_SEND_DELAY].id,
            MixUnit::Reverb => STRIP_PARAMS[STRIP_SEND_REVERB].id,
            MixUnit::Master => return true,
        };
        self.tracks.values().any(|t| t.params.contains_key(send))
            || self.sections.iter().any(|s| {
                s.automation
                    .iter()
                    .any(|l| l.track != MIX_TRACK_ID && l.param == send)
            })
    }
}

/// The pseudo-track id that addresses the `[mix]` table (in lanes and paths).
/// No real track may use it.
pub const MIX_TRACK_ID: &str = "mix";

/// The `[mix]` table. A song without one has the default: the soft clipper,
/// the first kick as duck source and every bus param at its default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mix {
    pub master: MasterMode,
    /// Explicit `duck_source`; `None` = the first `kick` track (see `Song::duck_source`).
    pub duck_source: Option<String>,
    /// Explicit mix params in plain units, keyed by full id (`delay.feedback`, `master.gain`).
    pub params: IndexMap<String, f64>,
}

impl Mix {
    /// Whether this is what a song without `[mix]` gets (nothing to write).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What the master stage does with the summed mix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MasterMode {
    /// The soft clipper every song had before the mixer.
    #[default]
    Clip,
    /// The lookahead true-peak limiter.
    Limit,
}

impl MasterMode {
    /// Every mode, in the order error messages list them.
    pub const ALL: [Self; 2] = [Self::Clip, Self::Limit];

    /// The `master` value in a song file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Limit => "limit",
        }
    }

    /// Parses [`name`](Self::name).
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == s)
    }
}

/// A shared unit after the tracks, whose params live in `[mix]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MixUnit {
    Delay,
    Reverb,
    Master,
}

impl MixUnit {
    /// Every unit, in param-listing order.
    pub const ALL: [Self; 3] = [Self::Delay, Self::Reverb, Self::Master];

    /// The prefix of the unit's param ids.
    pub fn name(self) -> &'static str {
        match self {
            Self::Delay => "delay",
            Self::Reverb => "reverb",
            Self::Master => "master",
        }
    }

    /// The unit's parameter table.
    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            Self::Delay => DELAY_PARAMS,
            Self::Reverb => REVERB_PARAMS,
            Self::Master => MASTER_PARAMS,
        }
    }
}

/// Finds a mix param by full id: its unit, its index in the unit's table and
/// its spec.
pub fn find_mix_param(id: &str) -> Option<(MixUnit, usize, &'static ParamSpec)> {
    MixUnit::ALL.into_iter().find_map(|unit| {
        find_spec(unit.params(), id)
            .ok()
            .map(|(index, spec)| (unit, index, spec))
    })
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
    /// Only the values the file sets, in plain units, keyed by full param id:
    /// instrument params (e.g. `acid303.cutoff`) and strip params
    /// (`send.delay`, `duck.depth`, see `STRIP_PARAMS`). Everything else
    /// stays at its default.
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
    /// The four-stage EQ (low cut, mid bell, high shelf, high cut).
    Eq {
        /// Explicit plain values only, keyed by full param id (`eq.low_cut`).
        params: IndexMap<String, f64>,
    },
}

impl FxSpec {
    /// Every fx `type` name a song file accepts.
    pub const TYPE_NAMES: [&'static str; 2] = ["distortion", "eq"];

    /// The `type` value in a song file.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Distortion { .. } => "distortion",
            Self::Eq { .. } => "eq",
        }
    }

    /// The effect's parameter table.
    pub fn params_table(&self) -> &'static [ParamSpec] {
        match self {
            Self::Distortion { .. } => DIST_PARAMS,
            Self::Eq { .. } => EQ_PARAMS,
        }
    }

    /// The explicit plain values the file sets, keyed by full param id.
    pub fn params(&self) -> &IndexMap<String, f64> {
        match self {
            Self::Distortion { params, .. } | Self::Eq { params } => params,
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
    /// A track id, or [`MIX_TRACK_ID`] for a mix param.
    pub track: String,
    /// Full param id of the instrument, the strip or one of the track's fx;
    /// for the `mix` pseudo-track, a mix param id (`reverb.size`).
    pub param: String,
    /// `(bar offset from the section start, normalized 0..=1 value)`, with
    /// bar offsets that never decrease. Two equal bars form a jump.
    pub points: Vec<(f64, f64)>,
}
