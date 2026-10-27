//! Note clips: reading, validation with positions, and the pitch helpers.

use tune_modules::{ACID303, KICK, ModuleDescriptor, Registry, Role};
use tune_song::{
    Clip, ClipKind, Note, NoteClip, Song, SongError, parse, parse_pitch, parse_song, parse_with,
    pitch_name,
};

const SONG: &str = r#"[song]
version = 2
name = "notes"
bpm = 120

[tracks.acid]
instrument = "acid303"
play = [{ at = 1, clip = "lead", bars = 1 }]

[clips.lead]
kind = "notes"
bars = 1
notes = [
  { start = 0, pitch = "C3", len = 3 },
  { start = 0.333, pitch = "G3", len = 0.5 },
  { start = 2.5, pitch = "D#3", len = 2, vel = 120 },
]

[[sections]]
name = "a"
bars = 1
"#;

const NOTES_BLOCK: &str = r#"notes = [
  { start = 0, pitch = "C3", len = 3 },
  { start = 0.333, pitch = "G3", len = 0.5 },
  { start = 2.5, pitch = "D#3", len = 2, vel = 120 },
]"#;

fn swap(from: &str, to: &str) -> String {
    assert_eq!(SONG.matches(from).count(), 1, "`{from}` must occur once");
    SONG.replacen(from, to, 1)
}

/// `SONG` with the `notes` of the clip replaced by the given entries.
fn with_notes(entries: &[&str]) -> String {
    let body: String = entries.iter().map(|e| format!("  {e},\n")).collect();
    swap(NOTES_BLOCK, &format!("notes = [\n{body}]"))
}

fn messages(errors: &[SongError]) -> String {
    errors
        .iter()
        .map(SongError::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn parsed(src: &str) -> Song {
    parse_song(src).unwrap_or_else(|e| panic!("must parse: {}\n{src}", messages(&e)))
}

fn pos(src: &str, needle: &str, skip: usize) -> (usize, usize) {
    assert_eq!(src.matches(needle).count(), 1, "`{needle}` must occur once");
    let offset = src.find(needle).unwrap();
    let before = &src[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[line_start..].chars().count() + 1 + skip)
}

#[track_caller]
fn fails_at(src: &str, needle: &str, skip: usize, message: &str) {
    let errors = parse(src).expect_err("must fail");
    let (line, column) = pos(src, needle, skip);
    assert_eq!(
        errors,
        [SongError {
            line: Some(line),
            column: Some(column),
            message: message.to_owned(),
        }],
        "{}\n{src}",
        messages(&errors)
    );
}

#[test]
fn a_note_clip_parses() {
    let song = parsed(SONG);
    let note = |start, len, pitch, velocity| Note {
        start,
        len,
        pitch,
        velocity,
    };
    assert_eq!(
        song.clips["lead"],
        Clip::Notes(NoteClip {
            bars: 1,
            notes: vec![
                note(0, 72, 48, 100),
                note(8, 12, 55, 100),
                note(60, 48, 51, 120)
            ],
        })
    );
    assert_eq!(song.clips["lead"].kind(), ClipKind::Notes);
    assert_eq!(song.clips["lead"].steps(), 16);
    assert_eq!(song.clips["lead"].steps_text(), None);
    let Clip::Notes(clip) = &song.clips["lead"] else {
        unreachable!()
    };
    assert_eq!(clip.steps(), 16);
    assert_eq!(clip.notes[2].end(), 108);
}

#[test]
fn a_notes_clip_may_have_no_notes() {
    let src = swap(NOTES_BLOCK, "").replace("bars = 1\n\n", "bars = 2\n\n");
    let song = parsed(&src);
    assert_eq!(
        song.clips["lead"],
        Clip::Notes(NoteClip {
            bars: 2,
            notes: vec![]
        })
    );
    assert_eq!(song.clips["lead"].steps(), 32);
}

#[test]
fn note_clip_rules() {
    fails_at(
        &swap("kind = \"notes\"", "kind = \"chords\""),
        "\"chords\"",
        0,
        "unknown clip kind `chords` (expected acid, drum or notes)",
    );
    fails_at(
        &swap("kind = \"notes\"\nbars = 1", "kind = \"notes\"\nbars = 0"),
        "bars = 0",
        7,
        "a notes clip has 1 to 64 bars, got 0",
    );
    fails_at(
        &swap("kind = \"notes\"\nbars = 1", "kind = \"notes\"\nbars = 65"),
        "bars = 65",
        7,
        "a notes clip has 1 to 64 bars, got 65",
    );
    let grid = "(positions are 16th steps in multiples of 1/24, e.g. 0.5, 0.25 or 0.333)";
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = 0.3, pitch = "G3", len = 0.5 }"#,
        ]),
        "0.3,",
        0,
        &format!("note 2 of clip `lead`: `start` 0.3 is off the grid {grid}"),
    );
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = 1, pitch = "G3", len = 0.3 }"#,
        ]),
        "0.3 }",
        0,
        &format!("note 2 of clip `lead`: `len` 0.3 is off the grid {grid}"),
    );
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = -1, pitch = "G3", len = 1 }"#,
        ]),
        "-1,",
        0,
        "note 2 of clip `lead`: `start` must be at least 0, got -1",
    );
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = 1, pitch = "G3", len = 0 }"#,
        ]),
        "0 }",
        0,
        "note 2 of clip `lead`: `len` must be more than 0, got 0",
    );
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = 15, pitch = "G3", len = 2 }"#,
        ]),
        "2 }",
        0,
        "note 2 of clip `lead` ends at step 17, past the clip's end (step 16)",
    );
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = 4, pitch = "G3", len = 1 }"#,
            r#"{ start = 3, pitch = "G3", len = 1 }"#,
        ]),
        "3,",
        0,
        "notes of clip `lead` must be in time order: note 3 starts before note 2",
    );
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = 4, pitch = "H2", len = 1 }"#,
        ]),
        "\"H2\"",
        0,
        "note 2 of clip `lead`: unknown pitch `H2` (expected a note such as `C3`, `D#2` or `Bb1`, from C-1 to G9)",
    );
    fails_at(
        &with_notes(&[
            r#"{ start = 0, pitch = "C3", len = 3 }"#,
            r#"{ start = 4, pitch = "G3", len = 1, vel = 0 }"#,
        ]),
        "0 }",
        0,
        "note 2 of clip `lead`: `vel` must be 1 to 127, got 0",
    );
    fails_at(
        &swap("kind = \"notes\"\nbars = 1", "kind = \"notes\""),
        "[clips.lead]",
        0,
        "missing field `bars` in [clips.lead]",
    );
}

#[test]
fn note_clip_keys_have_hints() {
    fails_at(
        &swap("bars = 1\nnotes", "bars = 1\nsteps = \"x\"\nnotes"),
        "steps",
        0,
        "a notes clip has no `steps`: its length is `bars` and its notes are `notes`",
    );
    let acid = SONG.replace(
        "[[sections]]",
        "[clips.a]\nkind = \"acid\"\nsteps = \"C2\"\nbars = 1\n\n[[sections]]",
    );
    fails_at(
        &acid,
        "bars = 1\n\n[[sections]]",
        0,
        "`bars` and `notes` are for a notes clip (kind = \"notes\")",
    );
    let drum = SONG.replace(
        "[[sections]]",
        "[clips.d]\nkind = \"drum\"\nsteps = \"x...\"\nnotes = []\n\n[[sections]]",
    );
    fails_at(
        &drum,
        "notes = []",
        0,
        "`bars` and `notes` are for a notes clip (kind = \"notes\")",
    );
}

/// A test-only instrument that lists drum clips only.
static THUMP: ModuleDescriptor = ModuleDescriptor {
    name: "thump",
    prefix: "thump",
    params: &[],
    settings: &[],
    role: Role::Instrument {
        plays: &[ClipKind::Drum],
        default_duck_source: false,
        make: |sample_rate, settings| match &KICK.role {
            Role::Instrument { make, .. } => make(sample_rate, settings),
            Role::Effect { .. } => unreachable!(),
        },
    },
};

#[test]
fn every_instrument_plays_note_clips() {
    assert!(KICK.plays(ClipKind::Notes));
    assert!(ACID303.plays(ClipKind::Notes));
    let registry = Registry::builtin().with(&THUMP);
    for instrument in ["kick", "acid303", "thump"] {
        let src = swap(
            "instrument = \"acid303\"",
            &format!("instrument = \"{instrument}\""),
        );
        let song = parse_with(&src, &registry)
            .map(|p| p.song)
            .unwrap_or_else(|e| panic!("{instrument}: {}", messages(&e)));
        assert_eq!(song.tracks["acid"].play[0].clip, "lead");
    }
}

#[test]
fn pitch_names_round_trip() {
    assert_eq!(parse_pitch("C3"), Some(48));
    assert_eq!(parse_pitch("Bb1"), Some(34));
    assert_eq!(parse_pitch("C-1"), Some(0));
    assert_eq!(parse_pitch("G9"), Some(127));
    assert_eq!(parse_pitch("C3!"), None);
    assert_eq!(parse_pitch("C3~"), None);
    assert_eq!(parse_pitch("."), None);
    assert_eq!(parse_pitch("H2"), None);
    assert_eq!(parse_pitch("G#9"), None);
    assert_eq!(pitch_name(51), "D#3");
    assert_eq!(pitch_name(0), "C-1");
    for pitch in 0..=127 {
        assert_eq!(parse_pitch(&pitch_name(pitch)), Some(pitch));
    }
}
