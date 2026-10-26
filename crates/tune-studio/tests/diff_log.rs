//! Song diffs by edit path and the per-song edit log.

use std::fs;

use jiff::Timestamp;
use serde_json::{Value, json};
use tune_studio::{
    Change, EditEntry, EditLog, SongDoc, SongPath, Source, diff_songs, is_whole_replace, text_hash,
};

const MINI: &str = include_str!("../../tune-song/tests/fixtures/mini.toml");
const FIRST_ACID: &str = include_str!("../../../songs/first-acid/song.toml");

fn song(text: &str) -> tune_song::Song {
    SongDoc::parse(text).expect("parses").song().clone()
}

fn path(s: &str) -> SongPath {
    s.parse().expect("path")
}

fn change(p: &str, old: Value, new: Value) -> Change {
    Change {
        path: path(p),
        old,
        new,
    }
}

#[test]
fn diff_param_change() {
    let mut d = SongDoc::parse(MINI).unwrap();
    d.set(&path("tracks.acid.acid303.cutoff"), &json!(820))
        .unwrap();
    assert_eq!(
        diff_songs(&song(MINI), d.song()),
        vec![change("tracks.acid.acid303.cutoff", json!(500), json!(820))]
    );
}

#[test]
fn diff_removed_param_shows_default() {
    let text = MINI.replace("\"acid303.resonance\" = 70, ", "");
    assert_ne!(text, MINI);
    assert_eq!(
        diff_songs(&song(MINI), &song(&text)),
        vec![change("tracks.acid.acid303.resonance", json!(70), json!(0))]
    );
}

#[test]
fn diff_steps() {
    let text = MINI.replace("C2 C2! C3~ C2 . D#2 C2~ G2!", "C2 C2! C3~ C2 . D#2 C2~ A2!");
    let d = diff_songs(&song(MINI), &song(&text));
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].path.to_string(), "patterns.a1.steps");
    assert!(d[0].old.is_string() && d[0].new.is_string());
    assert_ne!(d[0].old, d[0].new);
}

fn swapped() -> String {
    let a = "[[sections]]\nname = \"a\"\nbars = 2\nplay = { kick = \"k4\", hat = \"h\" }\n\n";
    let b = "[[sections]]\nname = \"b\"\nbars = 2\nplay = { kick = \"k4\", acid = \"a1\", hat = \"h\" }\n\n[[sections.automation]]\nparam = \"acid.acid303.cutoff\"\npoints = [[0, 0.1], [2, 0.8]]\n";
    assert!(MINI.contains(&format!("{a}{b}")));
    MINI.replace(&format!("{a}{b}"), &format!("{b}\n{a}"))
}

#[test]
fn diff_sections_reorder() {
    let text = swapped();
    assert_eq!(
        diff_songs(&song(MINI), &song(&text)),
        vec![change(
            "sections",
            json!(["a:2", "b:2"]),
            json!(["b:2", "a:2"])
        )]
    );
}

#[test]
fn diff_bars_only() {
    let text = MINI.replacen("bars = 2", "bars = 4", 1);
    // Section `b` and its lane stretch move back with it; within `b` the
    // lane is the same, so only the bars changed.
    assert_eq!(
        diff_songs(&song(MINI), &song(&text)),
        vec![change("sections[0].bars", json!(2), json!(4))]
    );
}

const MINI_V2: &str = include_str!("../../tune-song/tests/fixtures/v2/mini.toml");

/// Resizing a section moves the stretches after it (their `at` is
/// absolute), which is not an edit of those sections' lanes. A real edit of
/// a later section's lane still is.
#[test]
fn shifted_lanes_are_not_lane_changes() {
    let old = song(MINI_V2);
    assert_eq!(
        old.lanes_in(1).len(),
        1,
        "the fixture has a lane in section 1"
    );
    let resized = tune_song::resize_section(&old, 0, 3).unwrap().song;
    assert_eq!(resized.automation[0].start_bar, 3);
    assert_eq!(
        diff_songs(&old, &resized),
        vec![change("sections[0].bars", json!(2), json!(3))]
    );

    let mut edited = resized.clone();
    edited.automation[0].points[1].1 = 0.9;
    let lane =
        |song: &tune_song::Song| tune_studio::effective_value(song, &path("sections[1].lanes"));
    assert_eq!(
        diff_songs(&old, &edited),
        vec![
            change("sections[0].bars", json!(2), json!(3)),
            Change {
                path: path("sections[1].lanes"),
                old: lane(&old).unwrap(),
                new: lane(&edited).unwrap(),
            },
        ]
    );
}

#[test]
fn diff_track_added() {
    let text = format!("{MINI}\n[tracks.clap]\ninstrument = \"clap\"\n");
    assert_eq!(
        diff_songs(&song(MINI), &song(&text)),
        vec![change(
            "tracks.clap",
            Value::Null,
            json!({"instrument": "clap", "fx": []})
        )]
    );
}

#[test]
fn reordered_fx_chain_is_a_track_change() {
    const CHAIN: &str = "fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }, { type = \"eq\", params = { \"eq.low_cut\" = 85 } }]";
    const REVERSED: &str = "fx = [{ type = \"eq\", params = { \"eq.low_cut\" = 85 } }, { type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }]";
    assert!(MIXED.contains(CHAIN));
    let reordered = MIXED.replace(CHAIN, REVERSED);
    let dist = json!({"id": "dist", "type": "distortion"});
    let eq = json!({"id": "eq", "type": "eq"});
    assert_eq!(
        diff_songs(&song(MIXED), &song(&reordered)),
        vec![change(
            "tracks.acid",
            json!({"instrument": "acid303", "fx": [dist, eq]}),
            json!({"instrument": "acid303", "fx": [eq, dist]})
        )]
    );
}

#[test]
fn diff_identical_is_empty() {
    assert!(diff_songs(&song(MINI), &song(MINI)).is_empty());
}

#[test]
fn whole_replace_threshold() {
    let edited = MINI
        .replace("bpm = 120", "bpm = 121")
        .replace("gain_db = -3", "gain_db = -4")
        .replace("pan = 0.3", "pan = 0.4");
    assert!(!is_whole_replace(MINI, &edited));
    assert!(is_whole_replace(MINI, FIRST_ACID));
}

fn entry(ts: &str, source: Source, hash: Option<&str>) -> EditEntry {
    EditEntry {
        ts: ts.parse().unwrap(),
        source,
        path: "song.bpm".into(),
        old: json!(120),
        new: json!(121),
        file_hash: hash.map(str::to_owned),
    }
}

fn log_in(dir: &tempfile_dir::Dir) -> EditLog {
    EditLog::for_song(&dir.0.join("song.toml"))
}

/// A tiny temp dir without a dev-dependency.
mod tempfile_dir {
    use std::path::PathBuf;
    pub struct Dir(pub PathBuf);
    impl Dir {
        pub fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("tune-studio-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn log_append_and_read_since() {
    let dir = tempfile_dir::Dir::new("since");
    let log = log_in(&dir);
    let t = [
        "2026-10-21T22:15:01.000Z",
        "2026-10-21T22:15:02.000Z",
        "2026-10-21T22:15:03.000Z",
    ];
    for s in t {
        log.append(&[entry(s, Source::Human, None)]).unwrap();
    }
    assert_eq!(log.read_since(None).unwrap().len(), 3);
    let since: Timestamp = t[0].parse().unwrap();
    let got = log.read_since(Some(since)).unwrap();
    assert_eq!(
        got.iter().map(|e| e.ts).collect::<Vec<_>>(),
        vec![t[1].parse::<Timestamp>().unwrap(), t[2].parse().unwrap()]
    );
    assert_eq!(
        EditLog::for_song(&dir.0.join("nope/song.toml"))
            .read_since(None)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn malformed_line_skipped() {
    let dir = tempfile_dir::Dir::new("bad");
    let log = log_in(&dir);
    log.append(&[entry("2026-10-21T22:15:01.000Z", Source::Human, None)])
        .unwrap();
    let file = dir.0.join("edits.jsonl");
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str("this is not json\n");
    fs::write(&file, text).unwrap();
    log.append(&[entry("2026-10-21T22:15:02.000Z", Source::Claude, None)])
        .unwrap();
    assert_eq!(log.read_since(None).unwrap().len(), 2);
}

#[test]
fn concurrent_appends_do_not_interleave() {
    let dir = tempfile_dir::Dir::new("conc");
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let log = log_in(&dir);
            std::thread::spawn(move || {
                for n in 0..200 {
                    let mut e = entry("2026-10-21T22:15:01.000Z", Source::Human, None);
                    e.path = format!("song.bpm{}", "x".repeat(200 + i * 50 + n));
                    log.append(&[e]).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let text = fs::read_to_string(dir.0.join("edits.jsonl")).unwrap();
    assert_eq!(text.lines().count(), 400);
    assert_eq!(log_in(&dir).read_since(None).unwrap().len(), 400);
}

#[test]
fn hash_pinned() {
    assert_eq!(text_hash(""), "cbf29ce484222325");
    assert_eq!(text_hash("a"), "af63dc4c8601ec8c");
}

#[test]
fn has_claude_hash() {
    let dir = tempfile_dir::Dir::new("hash");
    let log = log_in(&dir);
    assert!(!log.has_claude_hash("abc").unwrap());
    log.append(&[
        entry("2026-10-21T22:15:01.000Z", Source::Human, Some("h1")),
        entry("2026-10-21T22:15:02.000Z", Source::Claude, Some("c1")),
    ])
    .unwrap();
    assert!(log.has_claude_hash("c1").unwrap());
    assert!(!log.has_claude_hash("h1").unwrap());
    assert!(!log.has_claude_hash("zz").unwrap());
}

#[test]
fn entry_json_shape() {
    let e = entry("2026-10-21T22:15:03.120Z", Source::Human, None);
    assert_eq!(
        serde_json::to_string(&e).unwrap(),
        r#"{"ts":"2026-10-21T22:15:03.120Z","source":"human","path":"song.bpm","old":120,"new":121}"#
    );
    let back: EditEntry = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
    assert_eq!(back, e);
}

const MIXED: &str = include_str!("../../tune-song/tests/fixtures/mixed.toml");

#[test]
fn diff_reports_mix_changes() {
    let mut d = SongDoc::parse(MIXED).unwrap();
    d.set(&path("tracks.acid.send.delay"), &json!(-10)).unwrap();
    d.set(&path("mix.delay.feedback"), &json!(55)).unwrap();
    d.set(&path("mix.master"), &json!("clip")).unwrap();
    assert_eq!(
        diff_songs(&song(MIXED), d.song()),
        vec![
            change("mix.master", json!("limit"), json!("clip")),
            change("mix.delay.feedback", json!(40), json!(55)),
            change("tracks.acid.send.delay", json!(-12), json!(-10)),
        ]
    );

    // The effective duck source moves with the explicit key, and a mix
    // param dropped from the file shows its default.
    let text = MIXED
        .replacen(
            "master = \"limit\"\n",
            "master = \"limit\"\nduck_source = \"hat\"\n",
            1,
        )
        .replacen("\"reverb.size\" = 30, ", "", 1);
    assert_eq!(
        diff_songs(&song(MIXED), &song(&text)),
        vec![
            change("mix.duck_source", json!("kick"), json!("hat")),
            change("mix.reverb.size", json!(30), json!(25)),
        ]
    );
}
