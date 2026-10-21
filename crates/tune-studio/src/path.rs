//! Edit addresses into a song file.
//!
//! A [`SongPath`] mirrors the TOML layout, so the producer, Claude and the
//! edit log all name a value the same way: `song.bpm`,
//! `tracks.acid.acid303.cutoff`, `sections[2].bars`. Sections are addressed
//! by index because their names need not be unique.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One addressable value (or read-only aggregate) of a song.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SongPath {
    /// `song.bpm`
    Bpm,
    /// `tracks.<t>.gain_db|pan|mute|solo`
    TrackField { track: String, field: TrackField },
    /// `tracks.<t>.<param id>`, an instrument or fx param such as
    /// `acid303.cutoff` or `dist.drive`.
    TrackParam { track: String, param: String },
    /// `tracks.<t>.dist.mode`
    FxMode { track: String },
    /// `tracks.<t>` (read-only: the track as a whole)
    Track { track: String },
    /// `patterns.<p>.steps`
    Pattern { pattern: String },
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
    /// Whether [`SongDoc::set`](crate::SongDoc::set) refuses this path
    /// (it can still be read and diffed).
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::Track { .. } | Self::SectionLanes(_) | Self::Sections
        )
    }
}

impl fmt::Display for SongPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bpm => f.write_str("song.bpm"),
            Self::TrackField { track, field } => write!(f, "tracks.{track}.{field}"),
            Self::TrackParam { track, param } => write!(f, "tracks.{track}.{param}"),
            Self::FxMode { track } => write!(f, "tracks.{track}.dist.mode"),
            Self::Track { track } => write!(f, "tracks.{track}"),
            Self::Pattern { pattern } => write!(f, "patterns.{pattern}.steps"),
            Self::SectionBars(i) => write!(f, "sections[{i}].bars"),
            Self::SectionName(i) => write!(f, "sections[{i}].name"),
            Self::SectionPlay { section, track } => write!(f, "sections[{section}].play.{track}"),
            Self::SectionLanes(i) => write!(f, "sections[{i}].lanes"),
            Self::Sections => f.write_str("sections"),
        }
    }
}

impl FromStr for SongPath {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        parse(s).ok_or_else(|| {
            format!(
                "unknown path `{s}` (expected e.g. song.bpm, tracks.<track>.gain_db, \
                 tracks.<track>.<param id>, patterns.<pattern>.steps, sections[<i>].bars)"
            )
        })
    }
}

fn parse(s: &str) -> Option<SongPath> {
    if s == "song.bpm" {
        return Some(SongPath::Bpm);
    }
    if s == "sections" {
        return Some(SongPath::Sections);
    }
    if let Some(rest) = s.strip_prefix("tracks.") {
        return parse_track(rest);
    }
    if let Some(rest) = s.strip_prefix("patterns.") {
        let pattern = rest.strip_suffix(".steps")?;
        return is_id(pattern).then(|| SongPath::Pattern {
            pattern: pattern.to_owned(),
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
    if field == "dist.mode" {
        return Some(SongPath::FxMode { track });
    }
    // A param id is `<prefix>.<name>`, e.g. `acid303.cutoff`.
    let (prefix, name) = field.split_once('.')?;
    (is_id(prefix) && is_id(name)).then(|| SongPath::TrackParam {
        track,
        param: field.to_owned(),
    })
}

/// A decimal index without sign or leading zeros, so parsing then printing
/// gives back the same string.
fn parse_index(s: &str) -> Option<usize> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    canonical.then(|| s.parse().ok()).flatten()
}

/// Track, pattern and param-name ids match `[a-z0-9_]+`.
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
            "sections[0]",
            "sections[0].play",
            "sections[0].play.",
            "sections[01].bars",
            "sections.0.bars",
        ] {
            assert!(bad.parse::<SongPath>().is_err(), "{bad:?} should not parse");
        }
    }
}
