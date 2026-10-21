//! The studio core and changes that arrive from disk: its own writes are
//! ignored, Claude's are attributed, external ones are logged, and the
//! producer's pending edits are merged by path so none are lost.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::json;
use tune_studio::{
    Command, EditEntry, EditLog, SongPath, Source, StudioCore, StudioError, set_paths,
};

const FIXTURE: &str = include_str!("fixtures/commented.toml");
const FIRST_ACID: &str = include_str!("../../../songs/first-acid/song.toml");
const CUTOFF_500: &str = "\"acid303.cutoff\" = 500";

/// A temp dir holding a copy of the commented fixture as `song.toml`.
struct Song {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn song() -> Song {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("song.toml");
    fs::write(&path, FIXTURE).expect("write fixture");
    Song { _dir: dir, path }
}

fn open(s: &Song) -> StudioCore {
    StudioCore::open(&s.path).expect("opens")
}

fn disk(s: &Song) -> String {
    fs::read_to_string(&s.path).expect("read song")
}

fn log(s: &Song) -> Vec<EditEntry> {
    EditLog::for_song(&s.path)
        .read_since(None)
        .expect("read log")
}

fn cutoff(value: f64) -> Command {
    Command::SetParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
        value,
        commit: true,
    }
}

fn cutoff_path() -> SongPath {
    SongPath::TrackParam {
        track: "acid".into(),
        param: "acid303.cutoff".into(),
    }
}

/// The fixture with `bpm = 121`.
fn bpm_121(text: &str) -> String {
    assert!(text.contains("bpm = 120"));
    text.replace("bpm = 120", "bpm = 121")
}

fn section_names(core: &StudioCore) -> Vec<String> {
    core.song()
        .sections
        .iter()
        .map(|s| s.name.clone())
        .collect()
}

#[test]
fn own_write_ignored() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    core.flush().unwrap();
    let lines = log(&s).len();

    assert!(core.on_disk_text(disk(&s)).is_none());
    assert_eq!(log(&s).len(), lines, "no new log line");
    core.undo().unwrap();
    assert!(!core.can_undo(), "no new undo entry");
    assert_eq!(disk(&s), FIXTURE);
}

#[test]
fn unchanged_text_ignored() {
    let s = song();
    let mut core = open(&s);
    assert!(core.on_disk_text(FIXTURE.to_owned()).is_none());
    assert!(!core.can_undo());
}

#[test]
fn external_edit_logged_and_undoable() {
    let s = song();
    let mut core = open(&s);
    let text = bpm_121(FIXTURE);
    fs::write(&s.path, &text).unwrap();
    let ext = core.on_disk_text(text).expect("an external change");
    assert_eq!(ext.source, Source::External);
    assert_eq!(ext.changes.len(), 1);
    assert_eq!(ext.changes[0].path, SongPath::Bpm);
    assert_eq!(
        (&ext.changes[0].old, &ext.changes[0].new),
        (&json!(120), &json!(121))
    );
    assert!(!ext.replaced);
    assert!(ext.merged.is_empty() && ext.offered.is_empty() && ext.errors.is_empty());
    assert_eq!(core.song().bpm, 121.0);

    let entries = log(&s);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source, Source::External);
    assert_eq!(entries[0].path, "song.bpm");

    core.undo().unwrap();
    assert_eq!(disk(&s), FIXTURE, "undo writes bpm 120 back");
    assert_eq!(core.song().bpm, 120.0);
}

#[test]
fn claude_edit_attributed() {
    let s = song();
    let mut core = open(&s);
    set_paths(&s.path, &[(SongPath::Bpm, json!(121))], Source::Claude).unwrap();
    let lines = log(&s).len();
    assert_eq!(lines, 1);

    let ext = core.on_disk_text(disk(&s)).expect("a change");
    assert_eq!(ext.source, Source::Claude);
    assert_eq!(ext.changes[0].path, SongPath::Bpm);
    assert_eq!(log(&s).len(), lines, "tune set already logged it");
    assert!(core.can_undo());
}

/// Human entries carry a file hash too; only `claude` entries attribute.
#[test]
fn human_hash_not_attributed_to_claude() {
    let s = song();
    let mut core = open(&s);
    set_paths(&s.path, &[(SongPath::Bpm, json!(121))], Source::Human).unwrap();
    assert!(log(&s)[0].file_hash.is_some());
    let ext = core.on_disk_text(disk(&s)).expect("a change");
    assert_eq!(ext.source, Source::External);
}

/// §8: no lost edits.
#[test]
fn merge_keeps_both_sides() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    let text = bpm_121(FIXTURE);
    fs::write(&s.path, &text).unwrap();

    let ext = core.on_disk_text(text).expect("a change");
    assert_eq!(ext.source, Source::External);
    assert_eq!(ext.merged.len(), 1);
    assert_eq!(ext.merged[0].path, cutoff_path());
    assert_eq!(ext.merged[0].new, json!(820));
    assert!(ext.offered.is_empty());
    assert!(core.pending().is_empty(), "the merge was flushed");
    let merged = disk(&s);
    assert!(merged.contains("bpm = 121"), "{merged}");
    assert!(merged.contains("\"acid303.cutoff\" = 820"), "{merged}");
    assert!(merged.contains("# the squelch"));

    core.undo().unwrap();
    let once = disk(&s);
    assert!(
        once.contains("bpm = 121") && once.contains(CUTOFF_500),
        "{once}"
    );
    core.undo().unwrap();
    assert_eq!(disk(&s), FIXTURE);
}

#[test]
fn pending_section_move_merges() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&Command::MoveSection { from: 1, to: 0 }, Instant::now())
        .unwrap();
    let text = bpm_121(FIXTURE);
    fs::write(&s.path, &text).unwrap();

    let ext = core.on_disk_text(text).expect("a change");
    assert_eq!(ext.merged.len(), 1);
    assert_eq!(ext.merged[0].path, SongPath::Sections);
    assert_eq!(section_names(&core), ["b", "a"]);
    let reopened = StudioCore::open(&s.path).unwrap();
    assert_eq!(reopened.song().bpm, 121.0);
    assert_eq!(section_names(&reopened), ["b", "a"]);
}

#[test]
fn same_path_conflict_offers_local() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    let text = FIXTURE.replace(CUTOFF_500, "\"acid303.cutoff\" = 700");
    fs::write(&s.path, &text).unwrap();

    let ext = core.on_disk_text(text.clone()).expect("a change");
    assert!(ext.merged.is_empty());
    assert_eq!(ext.offered.len(), 1);
    assert_eq!(ext.offered[0].path, cutoff_path());
    assert_eq!(ext.offered[0].new, json!(820));
    assert_eq!(core.offered(), ext.offered.as_slice());
    assert_eq!(disk(&s), text, "the external value wins");
    assert!(core.pending().is_empty());

    let changes = core.resolve_offered(true).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        (&changes[0].old, &changes[0].new),
        (&json!(700), &json!(820))
    );
    assert!(core.offered().is_empty());
    assert!(disk(&s).contains("\"acid303.cutoff\" = 820"));
    let last = log(&s).pop().unwrap();
    assert_eq!((last.source, last.new), (Source::Human, json!(820)));
}

/// A pending edit on another path still merges when one conflicts.
#[test]
fn conflict_and_merge_together() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    core.apply_edit(&cutoff(820.0), t0).unwrap();
    core.apply_edit(&Command::ResizeSection { index: 0, bars: 4 }, t0)
        .unwrap();
    let text = FIXTURE.replace(CUTOFF_500, "\"acid303.cutoff\" = 700");
    fs::write(&s.path, &text).unwrap();

    let ext = core.on_disk_text(text).expect("a change");
    assert_eq!(ext.offered.len(), 1);
    assert_eq!(ext.merged.len(), 1);
    assert_eq!(ext.merged[0].path, SongPath::SectionBars(0));
    let now = disk(&s);
    assert!(
        now.contains("\"acid303.cutoff\" = 700") && now.contains("bars = 4"),
        "{now}"
    );
}

#[test]
fn whole_replace_offers_pending() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    fs::write(&s.path, FIRST_ACID).unwrap();

    let ext = core.on_disk_text(FIRST_ACID.to_owned()).expect("a change");
    assert!(ext.replaced);
    assert_eq!(ext.offered.len(), 1);
    assert!(ext.merged.is_empty());
    assert_eq!(disk(&s), FIRST_ACID, "byte for byte");
    assert_eq!(core.song().bpm, 145.0);
    assert!(core.pending().is_empty());

    assert!(core.resolve_offered(false).unwrap().is_empty());
    assert!(core.offered().is_empty());
    assert_eq!(disk(&s), FIRST_ACID, "dropping writes nothing");
    core.undo().unwrap();
    assert_eq!(disk(&s), FIXTURE, "byte for byte");
}

/// Review Focus 2: an editor writes the file in two chunks while local
/// edits are pending, and the first chunk is invalid.
#[test]
fn pending_survives_invalid_external() {
    let s = song();
    let mut core = open(&s);
    let t0 = Instant::now();
    core.apply_edit(&cutoff(820.0), t0).unwrap();
    fs::write(&s.path, "[song\n").unwrap();

    let ext = core.on_disk_text("[song\n".to_owned()).expect("errors");
    assert_eq!(ext.errors[0].line, Some(1));
    assert!(ext.changes.is_empty() && ext.merged.is_empty() && ext.offered.is_empty());
    assert_eq!(core.file_errors(), ext.errors.as_slice());
    assert_eq!(
        core.pending().len(),
        1,
        "held, not merged into the half file"
    );
    assert_eq!(core.song().bpm, 120.0, "the last good document stays");
    assert_eq!(
        core.apply_edit(&cutoff(900.0), t0),
        Err(StudioError::FileInvalid)
    );
    assert_eq!(core.undo(), Err(StudioError::FileInvalid));
    assert_eq!(core.redo(), Err(StudioError::FileInvalid));
    assert!(core.flush().unwrap().is_empty());
    assert_eq!(disk(&s), "[song\n", "flush writes nothing");

    let text = bpm_121(FIXTURE);
    fs::write(&s.path, &text).unwrap();
    let ext = core.on_disk_text(text).expect("a change");
    assert!(ext.errors.is_empty());
    assert!(core.file_errors().is_empty());
    assert_eq!(ext.merged.len(), 1);
    let now = disk(&s);
    assert!(now.contains("bpm = 121"), "{now}");
    assert!(now.contains("\"acid303.cutoff\" = 820"), "{now}");
}

/// An invalid save reverted to the last good text clears the errors.
#[test]
fn invalid_then_reverted_clears_errors() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    assert!(
        !core
            .on_disk_text("[song\n".to_owned())
            .unwrap()
            .errors
            .is_empty()
    );

    let ext = core
        .on_disk_text(FIXTURE.to_owned())
        .expect("the errors cleared");
    assert!(ext.errors.is_empty() && ext.changes.is_empty());
    assert!(core.file_errors().is_empty());
    assert_eq!(core.pending().len(), 1);
    fs::write(&s.path, FIXTURE).unwrap();
    core.flush().unwrap();
    assert!(disk(&s).contains("\"acid303.cutoff\" = 820"));
}

#[test]
fn flush_detects_unseen_external() {
    let s = song();
    let mut core = open(&s);
    core.apply_edit(&cutoff(820.0), Instant::now()).unwrap();
    fs::write(&s.path, bpm_121(FIXTURE)).unwrap();

    core.flush().unwrap();
    let now = disk(&s);
    assert!(now.contains("bpm = 121"), "{now}");
    assert!(now.contains("\"acid303.cutoff\" = 820"), "{now}");
    assert_eq!(core.song().bpm, 121.0);
    let sources: Vec<Source> = log(&s).iter().map(|e| e.source).collect();
    assert_eq!(sources, [Source::External, Source::Human]);
}
