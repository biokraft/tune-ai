//! Edit addresses into a song file.
//!
//! A [`SongPath`] mirrors the TOML layout, so the producer, Claude and the
//! edit log all name a value the same way: `song.bpm`,
//! `tracks.acid.acid303.cutoff`, `mix.delay.feedback`, `sections[2].bars`.
//! Sections are addressed by index because their names need not be unique.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tune_song::MIX_TRACK_ID;

/// One addressable value (or read-only aggregate) of a song.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SongPath {
    /// `song.bpm`
    Bpm,
    /// `song.version` (read-only: the file's format version, which changes
    /// only when a file is migrated)
    Version,
    /// `tracks.<t>.gain_db|pan|mute|solo`
    TrackField { track: String, field: TrackField },
    /// `tracks.<t>.<device>.<name>`: a param of the instrument or an fx
    /// device such as `acid303.cutoff` or `dist.drive`, else a setting of
    /// that device such as `dist.mode`; or a strip param such as
    /// `send.delay`.
    TrackParam { track: String, param: String },
    /// `mix.<param id>`, a delay, reverb or master param such as
    /// `delay.feedback`.
    MixParam { param: String },
    /// `mix.master`: `"clip"` or `"limit"`.
    MixMaster,
    /// `mix.duck_source`: a track id, or `null` for the default (the first
    /// kick).
    MixDuckSource,
    /// `tracks.<t>` (read-only: the track as a whole)
    Track { track: String },
    /// `clips.<c>.steps`; `patterns.<c>.steps`, the version 1 spelling,
    /// parses to it too.
    Clip { clip: String },
    /// `clips.<c>.notes`: a notes clip's notes, an array of `{start, pitch,
    /// len, vel}` (`start` and `len` in 16th steps).
    ClipNotes { clip: String },
    /// `clips.<c>.bars`: a notes clip's length in whole bars.
    ClipBars { clip: String },
    /// `sections[<i>].bars`
    SectionBars(usize),
    /// `sections[<i>].name`
    SectionName(usize),
    /// `sections[<i>].play.<t>`
    SectionPlay { section: usize, track: String },
    /// `sections[<i>].lanes` (read-only: the section's automation lanes)
    SectionLanes(usize),
    /// `sections` (read-only: the arrangement; edited by the section ops)
    Sections,
    /// `scenes` (read-only: the scene ids in row order; edited by the scene
    /// ops)
    Scenes,
    /// `scenes.<s>` (read-only: the scene as a whole)
    Scene { scene: String },
    /// `scenes.<s>.slots.<t>`: the clip track `t` holds in scene `s`, or
    /// `null` for an empty slot.
    SceneSlot { scene: String, track: String },
}

/// A track's mixer fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackField {
    GainDb,
    Pan,
    Mute,
    Solo,
}

impl TrackField {
    /// Every field, in song-file order.
    pub const ALL: [Self; 4] = [Self::GainDb, Self::Pan, Self::Mute, Self::Solo];

    /// The key in a `[tracks.<t>]` table.
    pub fn key(self) -> &'static str {
        match self {
            Self::GainDb => "gain_db",
            Self::Pan => "pan",
            Self::Mute => "mute",
            Self::Solo => "solo",
        }
    }

    /// Parses [`key`](Self::key).
    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == s)
    }
}

impl fmt::Display for TrackField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

impl SongPath {
    /// `tracks.<track>.<param>`, or `mix.<param>` when `track` is the `mix`
    /// pseudo-track: how a knob's `(track, param)` names its value.
    pub fn param(track: &str, param: &str) -> SongPath {
        if track == MIX_TRACK_ID {
            Self::MixParam {
                param: param.to_owned(),
            }
        } else {
            Self::TrackParam {
                track: track.to_owned(),
                param: param.to_owned(),
            }
        }
    }

    /// Whether [`SongDoc::set`](crate::SongDoc::set) refuses this path
    /// (it can still be read and diffed).
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::Version
                | Self::Track { .. }
                | Self::SectionLanes(_)
                | Self::Sections
                | Self::Scenes
                | Self::Scene { .. }
        )
    }
}

impl fmt::Display for SongPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bpm => f.write_str("song.bpm"),
            Self::Version => f.write_str("song.version"),
            Self::TrackField { track, field } => write!(f, "tracks.{track}.{field}"),
            Self::TrackParam { track, param } => write!(f, "tracks.{track}.{param}"),
            Self::MixParam { param } => write!(f, "mix.{param}"),
            Self::MixMaster => f.write_str("mix.master"),
            Self::MixDuckSource => f.write_str("mix.duck_source"),
            Self::Track { track } => write!(f, "tracks.{track}"),
            Self::Clip { clip } => write!(f, "clips.{clip}.steps"),
            Self::ClipNotes { clip } => write!(f, "clips.{clip}.notes"),
            Self::ClipBars { clip } => write!(f, "clips.{clip}.bars"),
            Self::SectionBars(i) => write!(f, "sections[{i}].bars"),
            Self::SectionName(i) => write!(f, "sections[{i}].name"),
            Self::SectionPlay { section, track } => write!(f, "sections[{section}].play.{track}"),
            Self::SectionLanes(i) => write!(f, "sections[{i}].lanes"),
            Self::Sections => f.write_str("sections"),
            Self::Scenes => f.write_str("scenes"),
            Self::Scene { scene } => write!(f, "scenes.{scene}"),
            Self::SceneSlot { scene, track } => write!(f, "scenes.{scene}.slots.{track}"),
        }
    }
}

impl FromStr for SongPath {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        parse(s).ok_or_else(|| {
            format!(
                "unknown path `{s}` (expected e.g. song.bpm, tracks.<track>.gain_db, \
                 tracks.<track>.<param id>, mix.<param id>, clips.<clip>.steps|notes|bars, \
                 sections[<i>].bars, scenes.<scene>.slots.<track>)"
            )
        })
    }
}

fn parse(s: &str) -> Option<SongPath> {
    if s == "song.bpm" {
        return Some(SongPath::Bpm);
    }
    if s == "song.version" {
        return Some(SongPath::Version);
    }
    if s == "sections" {
        return Some(SongPath::Sections);
    }
    if s == "scenes" {
        return Some(SongPath::Scenes);
    }
    if let Some(rest) = s.strip_prefix("scenes.") {
        return parse_scene(rest);
    }
    if let Some(rest) = s.strip_prefix("tracks.") {
        return parse_track(rest);
    }
    if let Some(rest) = s.strip_prefix("mix.") {
        return parse_mix(rest);
    }
    if let Some(rest) = s.strip_prefix("clips.") {
        let (clip, field) = rest.rsplit_once('.')?;
        if !is_id(clip) {
            return None;
        }
        let clip = clip.to_owned();
        return match field {
            "steps" => Some(SongPath::Clip { clip }),
            "notes" => Some(SongPath::ClipNotes { clip }),
            "bars" => Some(SongPath::ClipBars { clip }),
            _ => None,
        };
    }
    if let Some(clip) = s
        .strip_prefix("patterns.")
        .and_then(|rest| rest.strip_suffix(".steps"))
    {
        return is_id(clip).then(|| SongPath::Clip {
            clip: clip.to_owned(),
        });
    }
    if let Some(rest) = s.strip_prefix("sections[") {
        let (index, rest) = rest.split_once("].")?;
        let index = parse_index(index)?;
        return match rest {
            "bars" => Some(SongPath::SectionBars(index)),
            "name" => Some(SongPath::SectionName(index)),
            "lanes" => Some(SongPath::SectionLanes(index)),
            _ => {
                let track = rest.strip_prefix("play.")?;
                is_id(track).then(|| SongPath::SectionPlay {
                    section: index,
                    track: track.to_owned(),
                })
            }
        };
    }
    None
}

/// Everything after `scenes.`.
fn parse_scene(rest: &str) -> Option<SongPath> {
    let Some((scene, slot)) = rest.split_once('.') else {
        return is_id(rest).then(|| SongPath::Scene {
            scene: rest.to_owned(),
        });
    };
    let track = slot.strip_prefix("slots.")?;
    (is_id(scene) && is_id(track)).then(|| SongPath::SceneSlot {
        scene: scene.to_owned(),
        track: track.to_owned(),
    })
}

/// Everything after `tracks.`.
fn parse_track(rest: &str) -> Option<SongPath> {
    let (track, field) = match rest.split_once('.') {
        None => {
            return is_id(rest).then(|| SongPath::Track {
                track: rest.to_owned(),
            });
        }
        Some(split) => split,
    };
    if !is_id(track) {
        return None;
    }
    let track = track.to_owned();
    if let Some(field) = TrackField::from_key(field) {
        return Some(SongPath::TrackField { track, field });
    }
    is_param_id(field).then(|| SongPath::TrackParam {
        track,
        param: field.to_owned(),
    })
}

/// Everything after `mix.`.
fn parse_mix(rest: &str) -> Option<SongPath> {
    match rest {
        "master" => Some(SongPath::MixMaster),
        "duck_source" => Some(SongPath::MixDuckSource),
        _ => is_param_id(rest).then(|| SongPath::MixParam {
            param: rest.to_owned(),
        }),
    }
}

/// A param id is `<device or strip>.<name>`, e.g. `acid303.cutoff`.
fn is_param_id(s: &str) -> bool {
    s.split_once('.')
        .is_some_and(|(prefix, name)| is_id(prefix) && is_id(name))
}

/// A decimal index without sign or leading zeros, so parsing then printing
/// gives back the same string.
fn parse_index(s: &str) -> Option<usize> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    canonical.then(|| s.parse().ok()).flatten()
}

/// Track, clip and param-name ids match `[a-z0-9_]+`.
fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Serialize for SongPath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SongPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_must_be_canonical() {
        assert_eq!(parse_index("0"), Some(0));
        assert_eq!(parse_index("12"), Some(12));
        for bad in ["", "01", "+1", "-1", "x", "1 "] {
            assert_eq!(parse_index(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn malformed_paths() {
        for bad in [
            "",
            "song",
            "song.name",
            "tracks.",
            "tracks.acid.",
            "tracks.acid.cutoff",
            "tracks.acid.acid303.",
            "tracks.acid.acid303.cut.off",
            "patterns.a1",
            "patterns.a1.track",
            "clips.a1",
            "clips.a1.kind",
            "song.versions",
            "sections[0]",
            "sections[0].play",
            "sections[0].play.",
            "sections[01].bars",
            "sections.0.bars",
            "scenes.",
            "scenes.Groove",
            "scenes.groove.",
            "scenes.groove.slots",
            "scenes.groove.slots.",
            "scenes.groove.acid",
        ] {
            assert!(bad.parse::<SongPath>().is_err(), "{bad:?} should not parse");
        }
    }
}
