//! Second pass: the raw mirror to the [`Song`] model, checking every rule
//! and recording every violation (nothing stops at the first one).
//!
//! This module checks the header, the mix, ducking, ids and the song
//! length; [`tracks`] checks tracks, their fx and their params, and
//! [`arrange`] clips, sections, placements and lanes.

mod arrange;
mod tracks;

use std::ops::RangeInclusive;

use indexmap::IndexMap;
use tune_dsp::{ParamSpec, STRIP_DUCK_DEPTH, STRIP_PARAMS};
use tune_modules::{ClipKind, ModuleDescriptor, Registry};

use crate::error::Errors;
use crate::model::{MIX_TRACK_ID, MasterMode, Mix, MixUnit, SECTION_BARS, Song};
use crate::raw::{At, RawMix, RawSong};

use tracks::{TrackShape, param_values, track};

const BPM: RangeInclusive<f64> = 40.0..=300.0;
/// [`SECTION_BARS`], for the raw (signed) values of a file.
const BARS: RangeInclusive<i64> = *SECTION_BARS.start() as i64..=*SECTION_BARS.end() as i64;
const SAMPLE_RATE: RangeInclusive<i64> = 8_000..=384_000;
const MAX_SECONDS: f64 = 30.0 * 60.0;
/// The most bars a song can have: [`MAX_SECONDS`] at the highest bpm, 4
/// beats per bar (2250).
pub(super) const MAX_SONG_BARS: i64 = (MAX_SECONDS * *BPM.end() / 60.0 / 4.0) as i64;

/// Builds the song, or returns `None` when anything was recorded in `errors`.
pub(crate) fn validate(raw: RawSong, registry: &Registry, errors: &mut Errors) -> Option<Song> {
    let header = raw.song.as_ref();
    let name = header
        .and_then(|h| h.name.as_ref())
        .map(|n| n.value.clone());
    let bpm = header.and_then(|h| h.bpm.as_ref()).and_then(|bpm| {
        let ok = BPM.contains(&bpm.value);
        if !ok {
            errors.at(
                &bpm.span,
                format!("bpm must be 40 to 300, got {}", bpm.value),
            );
        }
        ok.then_some(bpm.value)
    });
    let seed = header.and_then(|h| h.seed.as_ref()).map_or(0, |s| s.value);
    let sample_rate = header.and_then(|h| h.sample_rate.as_ref()).and_then(|sr| {
        let ok = SAMPLE_RATE.contains(&sr.value);
        if !ok {
            errors.at(
                &sr.span,
                format!("sample_rate must be 8000 to 384000, got {}", sr.value),
            );
        }
        ok.then(|| u32::try_from(sr.value).expect("checked range"))
    });

    // Every track the file declares, valid or not, with what is known of
    // its shape: a reference to a declared but broken track is not reported
    // again as unknown, and its clips and lanes are still checked as far
    // as its instrument and fx types allow.
    let shapes: IndexMap<&str, TrackShape> = raw
        .tracks
        .iter()
        .map(|(id, t)| (id.value.as_str(), TrackShape::of(t, registry)))
        .collect();

    let mix = mix(raw.mix.as_ref(), &shapes, errors);

    let v2 = raw.version == 2;
    let mut tracks = IndexMap::new();
    for (id, t) in &raw.tracks {
        check_id(id, "track", errors);
        if let Some(track) = track(id, t, v2, registry, errors) {
            tracks.insert(id.value.clone(), track);
        }
    }
    let duck = Duck::of(&raw, &shapes);
    check_duck_depths(&raw, &duck, errors);

    let arrangement = if v2 {
        arrange::v2(&raw, &shapes, &duck, &mut tracks, errors)
    } else {
        arrange::v1(&raw, &shapes, &duck, &mut tracks, errors)
    };

    // Counted over every section whose bars are valid, so the length check
    // still runs when another part of a section is broken.
    let total_bars: u64 = raw
        .sections
        .iter()
        .flatten()
        .filter_map(|s| s.bars.as_ref())
        .filter(|b| BARS.contains(&b.value))
        .map(|b| b.value.unsigned_abs())
        .sum();
    if let Some(bpm) = bpm {
        // 4 beats per bar.
        let seconds = total_bars as f64 * 4.0 * 60.0 / bpm;
        if seconds > MAX_SECONDS {
            errors.global(format!(
                "the song is longer than 30 minutes: {total_bars} bars at {bpm} bpm last {:.1} minutes",
                seconds / 60.0
            ));
        }
    }

    if !errors.is_empty() {
        return None;
    }
    Some(Song {
        name: name?,
        bpm: bpm?,
        seed,
        sample_rate,
        tracks,
        clips: arrangement.clips,
        sections: arrangement.sections,
        automation: arrangement.automation,
        mix: mix?,
    })
}

/// Checks the `[mix]` table; a song without one gets [`Mix::default`].
fn mix(
    raw: Option<&RawMix>,
    shapes: &IndexMap<&str, TrackShape>,
    errors: &mut Errors,
) -> Option<Mix> {
    let Some(raw) = raw else {
        return Some(Mix::default());
    };
    let master = match &raw.master {
        None => Some(MasterMode::default()),
        Some(m) => {
            let mode = MasterMode::from_name(&m.value);
            if mode.is_none() {
                let expected: Vec<&str> = MasterMode::ALL.iter().map(|m| m.name()).collect();
                errors.at(
                    &m.span,
                    format!(
                        "unknown master mode `{}` (expected {})",
                        m.value,
                        expected.join(" or ")
                    ),
                );
            }
            mode
        }
    };
    let duck_source = match &raw.duck_source {
        None => Some(None),
        Some(src) => match shapes.get(src.value.as_str()) {
            None => {
                errors.at(&src.span, unknown_track(&src.value));
                None
            }
            // An unknown instrument already has its own error.
            Some(shape) if shape.module().is_some_and(|m| !plays_drums(m)) => {
                errors.at(
                    &src.span,
                    format!(
                        "duck_source `{}` must be a drum track (ducking follows its hits)",
                        src.value
                    ),
                );
                None
            }
            Some(_) => Some(Some(src.value.clone())),
        },
    };
    let before = errors.len();
    let tables: Vec<&'static [ParamSpec]> = MixUnit::ALL.iter().map(|u| u.params()).collect();
    let params = param_values(&raw.params, &tables, MIX_TRACK_ID, errors);
    (errors.len() == before).then_some(())?;
    Some(Mix {
        master: master?,
        duck_source: duck_source?,
        params,
    })
}

/// Who `duck.depth` follows, as far as the raw file tells.
enum Duck<'a> {
    /// `[mix] duck_source` is set: the track when it names a drum track,
    /// `None` when it is invalid (it has its own error, and nothing else is
    /// derived from it).
    Explicit(Option<&'a str>),
    /// The first kick track, if any.
    FirstKick(Option<&'a str>),
}

impl<'a> Duck<'a> {
    fn of(raw: &'a RawSong, shapes: &IndexMap<&'a str, TrackShape>) -> Self {
        match raw.mix.as_ref().and_then(|m| m.duck_source.as_ref()) {
            Some(src) => Self::Explicit(
                shapes
                    .get(src.value.as_str())
                    .and_then(TrackShape::module)
                    .is_some_and(plays_drums)
                    .then_some(src.value.as_str()),
            ),
            None => Self::FirstKick(
                shapes
                    .iter()
                    .find(|(_, s)| s.module().is_some_and(|m| m.default_duck_source()))
                    .map(|(id, _)| *id),
            ),
        }
    }

    /// The track that ducks the others, when there is one.
    fn source(&self) -> Option<&'a str> {
        match self {
            Self::Explicit(source) | Self::FirstKick(source) => *source,
        }
    }

    /// No explicit source and no kick: nothing for `duck.depth` to follow.
    fn missing(&self) -> bool {
        matches!(self, Self::FirstKick(None))
    }

    /// The error for `duck.depth` > 0 (or a lane on it) on `track`, if any.
    fn depth_error(&self, track: &str) -> Option<String> {
        if self.missing() {
            Some(NO_DUCK_SOURCE.to_owned())
        } else if self.source() == Some(track) {
            Some(format!(
                "track `{track}` is the duck source and cannot duck itself"
            ))
        } else {
            None
        }
    }
}

const NO_DUCK_SOURCE: &str =
    "`duck.depth` needs a kick to follow: add a kick track or set duck_source in [mix]";

/// Every explicit `duck.depth` above 0 needs a source, and the source may
/// not duck itself. Lanes on `duck.depth` follow the same rules (see [`lane`]).
fn check_duck_depths(raw: &RawSong, duck: &Duck<'_>, errors: &mut Errors) {
    let depth = &STRIP_PARAMS[STRIP_DUCK_DEPTH];
    for (id, t) in &raw.tracks {
        for (param, v) in &t.params {
            // An out-of-range depth already has its own error.
            if param.value != depth.id || v.value <= 0.0 || depth.check(v.value).is_err() {
                continue;
            }
            if let Some(message) = duck.depth_error(&id.value) {
                errors.at(&v.span, message);
            }
        }
    }
}

/// Track, device, clip and pattern ids match `[a-z0-9_]+`.
fn check_id(id: &At<String>, what: &str, errors: &mut Errors) {
    let ok = !id.value.is_empty()
        && id
            .value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        errors.at(
            &id.span,
            format!("{what} id `{}` may only contain a-z, 0-9 and _", id.value),
        );
    } else if what == "track" && id.value == MIX_TRACK_ID {
        // Stems are written as `<id>.wav`; `mix.wav` is the master.
        errors.at(
            &id.span,
            "track id `mix` is reserved (`mix.wav` is the master render)".to_owned(),
        );
    }
}

/// Ducking follows hits, so a duck source must play drum clips.
fn plays_drums(module: &ModuleDescriptor) -> bool {
    module.plays(ClipKind::Drum)
}

fn unknown_track(id: &str) -> String {
    format!("unknown track `{id}`")
}
