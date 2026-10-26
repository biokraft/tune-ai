//! The validated song model. Every map keeps song-file order.

use std::ops::Range;

use indexmap::IndexMap;
use tune_dsp::acid303::AcidPattern;
use tune_dsp::{
    DELAY_PARAMS, DrumPattern, MASTER_PARAMS, ParamSpec, REVERB_PARAMS, STRIP_PARAMS,
    STRIP_SEND_DELAY, STRIP_SEND_REVERB, find_spec,
};
use tune_modules::{ClipKind, ModuleDescriptor};

/// A whole song: header, tracks, clips and the arrangement.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub name: String,
    pub bpm: f64,
    /// Seeds all noise (0 when the file leaves it out).
    pub seed: u64,
    /// `None` means the renderer's default (48 000 Hz).
    pub sample_rate: Option<u32>,
    pub tracks: IndexMap<String, Track>,
    /// The clip pool, in song-file order.
    pub clips: IndexMap<String, Clip>,
    /// Named stretches laid end to end from bar 0; they set the song length.
    pub sections: Vec<Section>,
    /// Every lane stretch, in song-file order.
    pub automation: Vec<Lane>,
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
    /// the first track whose instrument is a default duck source (a kick);
    /// `None` when there is neither.
    pub fn duck_source(&self) -> Option<&str> {
        match &self.mix.duck_source {
            Some(id) => Some(id.as_str()),
            None => self
                .tracks
                .iter()
                .find(|(_, t)| t.instrument.module.default_duck_source())
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
        self.tracks.values().any(|t| t.strip.contains_key(send))
            || self
                .automation
                .iter()
                .any(|l| l.track != MIX_TRACK_ID && l.param == send)
    }

    /// The 0-based bar range of section `section`. Panics when there is no
    /// such section.
    fn section_bars(&self, section: usize) -> Range<u32> {
        let start = self.sections[..section].iter().map(|s| s.bars).sum::<u32>();
        start..start + self.sections[section].bars
    }

    /// Whether the 0-based bar range `span` (of a placement or a lane
    /// stretch) covers exactly section `section`. Panics when there is no
    /// such section.
    pub fn covers_section(&self, section: usize, span: Range<u32>) -> bool {
        span == self.section_bars(section)
    }

    /// What track `track` plays in section `section`: nothing, one clip over
    /// the whole section, or anything else (the placements that overlap
    /// it, in time order). An unknown track is silent. Panics when there is
    /// no such section.
    pub fn section_play(&self, section: usize, track: &str) -> SectionPlay<'_> {
        let bars = self.section_bars(section);
        let Some(t) = self.tracks.get(track) else {
            return SectionPlay::Silent;
        };
        let overlapping: Vec<&Placement> = t
            .play
            .iter()
            .filter(|p| overlaps(&p.span(), &bars))
            .collect();
        match overlapping.as_slice() {
            [] => SectionPlay::Silent,
            [p] if p.start_bar <= bars.start && p.span().end >= bars.end => {
                SectionPlay::Clip(&p.clip)
            }
            _ => SectionPlay::Other(overlapping),
        }
    }

    /// The lane stretches that overlap section `section`, in file order.
    /// Panics when there is no such section.
    pub fn lanes_in(&self, section: usize) -> Vec<&Lane> {
        let bars = self.section_bars(section);
        self.automation
            .iter()
            .filter(|l| overlaps(&l.span(), &bars))
            .collect()
    }
}

/// Whether two half-open bar ranges share a bar.
fn overlaps(a: &Range<u32>, b: &Range<u32>) -> bool {
    a.start < b.end && b.start < a.end
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

/// One instance of a module on a track: the instrument or an effect.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// Unique within the track; the prefix of its param ids
    /// (`<id>.<param>`). A v1 file always uses the module's prefix.
    pub id: String,
    pub module: &'static ModuleDescriptor,
    /// Every setting, in `module.settings` order, defaults filled in.
    pub settings: Vec<&'static str>,
    /// Explicit plain values only, keyed by local param name (`cutoff`, `drive`).
    pub params: IndexMap<String, f64>,
}

impl Device {
    /// A device of `module` called `id`, with default settings and no
    /// explicit params.
    pub fn new(id: &str, module: &'static ModuleDescriptor) -> Self {
        Self {
            id: id.to_owned(),
            module,
            settings: module.default_settings(),
            params: IndexMap::new(),
        }
    }

    /// The track-relative id of param `local`: `<id>.<local>`.
    pub fn param_id(&self, local: &str) -> String {
        format!("{}.{local}", self.id)
    }

    /// The local name of the track-relative param id `id` when it names one
    /// of this device's params (`eq2.low_cut` on device `eq2` is `low_cut`).
    pub fn local_param<'a>(&self, id: &'a str) -> Option<&'a str> {
        self.module.local_param(&self.id, id)
    }

    /// The value of setting `name`, if the module has it.
    pub fn setting(&self, name: &str) -> Option<&'static str> {
        let (index, _) = self.module.setting(name)?;
        self.settings.get(index).copied()
    }
}

/// One mixer channel: an instrument, its fx chain and its strip.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub instrument: Device,
    pub fx: Vec<Device>,
    /// Explicit strip params (`send.delay`, `duck.depth`, …, see
    /// `STRIP_PARAMS`) in plain units, keyed by full id.
    pub strip: IndexMap<String, f64>,
    pub gain_db: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    /// Where the track plays which clip: in time order, never overlapping.
    pub play: Vec<Placement>,
}

impl Track {
    /// The instrument or the fx device called `id`.
    pub fn device(&self, id: &str) -> Option<&Device> {
        std::iter::once(&self.instrument)
            .chain(&self.fx)
            .find(|d| d.id == id)
    }

    /// The id a new device of `module` gets: its prefix, else `<prefix>2`,
    /// `<prefix>3`, … (the first one no device of the track has).
    pub fn next_device_id(&self, module: &ModuleDescriptor) -> String {
        let prefix = module.prefix;
        if self.device(prefix).is_none() {
            return prefix.to_owned();
        }
        (2..)
            .map(|n| format!("{prefix}{n}"))
            .find(|id| self.device(id).is_none())
            .expect("a track has finitely many devices")
    }
}

/// A named loop of steps in the pool of clips. A clip belongs to no track;
/// which tracks can play it follows from its kind.
#[derive(Clone, Debug, PartialEq)]
pub enum Clip {
    /// Steps in 303 notation.
    Acid(AcidPattern),
    /// Steps in drum notation.
    Drum(DrumPattern),
}

impl Clip {
    /// The kind a module must list in `plays` to play this clip.
    pub fn kind(&self) -> ClipKind {
        match self {
            Self::Acid(_) => ClipKind::Acid,
            Self::Drum(_) => ClipKind::Drum,
        }
    }

    /// The steps in the clip's notation, as the song file writes them.
    pub fn steps_text(&self) -> String {
        match self {
            Self::Acid(p) => p.to_string(),
            Self::Drum(p) => p.to_string(),
        }
    }
}

/// One clip played on one track over a stretch of bars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// 0-based.
    pub start_bar: u32,
    pub bars: u32,
    pub clip: String,
    /// Bars into the clip at which the placement starts playing it.
    pub offset_bars: u32,
}

impl Placement {
    /// The 0-based bars the placement covers.
    pub fn span(&self) -> Range<u32> {
        self.start_bar..self.start_bar + self.bars
    }
}

/// A named stretch of the arrangement. Sections hold no content: they lay
/// out the song's bars.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    pub bars: u32,
}

/// What one track plays in one section (see [`Song::section_play`]).
#[derive(Clone, Debug, PartialEq)]
pub enum SectionPlay<'a> {
    /// No placement overlaps the section.
    Silent,
    /// Exactly one placement overlaps the section and covers all of it.
    Clip(&'a str),
    /// Any other arrangement: the overlapping placements, in time order.
    Other(Vec<&'a Placement>),
}

/// One stretch of one param's lane. Outside every stretch of its param the
/// param has its static value.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    /// A track id, or [`MIX_TRACK_ID`] for a mix param.
    pub track: String,
    /// Full param id of the instrument, the strip or one of the track's fx;
    /// for the `mix` pseudo-track, a mix param id (`reverb.size`).
    pub param: String,
    /// 0-based.
    pub start_bar: u32,
    pub bars: u32,
    /// `(bar offset from start_bar, normalized 0..=1 value)`, with bar
    /// offsets that never decrease. Two equal bars form a jump.
    pub points: Vec<(f64, f64)>,
}

impl Lane {
    /// The 0-based bars the stretch covers.
    pub fn span(&self) -> Range<u32> {
        self.start_bar..self.start_bar + self.bars
    }
}

/// The normalized lane value at bar offset `x` of breakpoints `points`
/// (bars never decreasing, at least one point): the first point's value
/// before it, linear between points, the last point's value after it, and
/// the later point where two share a bar.
pub fn lane_value(points: &[(f64, f64)], x: f64) -> f64 {
    let i = points.partition_point(|&(bar, _)| bar <= x);
    if i == 0 {
        return points[0].1;
    }
    if i == points.len() {
        return points[i - 1].1;
    }
    let (b0, v0) = points[i - 1];
    let (b1, v1) = points[i];
    v0 + (v1 - v0) * (x - b0) / (b1 - b0)
}
