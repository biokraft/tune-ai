//! Section operations over placements and lane stretches: every item that
//! crosses an edit's edge is split there first, so everything outside the
//! edited section plays as before.

use tune_song::{
    ArrangeError, Arranged, Clip, LaneOrigin, Song, delete_section, duplicate_section,
    move_section, parse_song, resize_section, set_section_play,
};

use LaneOrigin::{Copy, Kept, Split};

/// `c` is a 4-bar clip (so a placement's offset shows), `d` a 1-bar one.
const CLIPS: &str = r#"
[clips.c]
kind = "drum"
steps = "x...............x.x.............x.x.x...........x.x.x.x........."

[clips.d]
kind = "drum"
steps = "x...x...x...x..."
"#;

/// A v2 song with sections of `sections` bars (named `s0`, `s1`, …), the
/// track tables `tracks`, the clips `c` and `d`, and the `[[automation]]`
/// tables `automation`.
fn song(sections: &[u32], tracks: &str, automation: &str) -> Song {
    let mut text = String::from("[song]\nversion = 2\nname = \"t\"\nbpm = 120\n\n");
    text += tracks;
    text += CLIPS;
    for (i, bars) in sections.iter().enumerate() {
        text += &format!("\n[[sections]]\nname = \"s{i}\"\nbars = {bars}\n");
    }
    text += automation;
    parse_song(&text).unwrap_or_else(|e| panic!("{e:?}\n{text}"))
}

/// A placement as `(0-based start bar, bars, clip, offset bars)`.
type P = (u32, u32, &'static str, u32);

/// The table of track `id` playing `instrument` at `play`.
fn track(id: &str, instrument: &str, play: &[P]) -> String {
    let play: Vec<String> = play
        .iter()
        .map(|(at, bars, clip, offset)| {
            format!(
                "  {{ at = {}, clip = \"{clip}\", bars = {bars}, offset = {offset} }},\n",
                at + 1
            )
        })
        .collect();
    let play = if play.is_empty() {
        String::new()
    } else {
        format!("play = [\n{}]\n", play.concat())
    };
    format!("[tracks.{id}]\ninstrument = \"{instrument}\"\n{play}\n")
}

/// A kick track and a closed-hat track.
fn kick_hat(kick: &[P], hat: &[P]) -> String {
    track("kick", "kick", kick) + &track("hat", "hat_closed", hat)
}

/// One `[[automation]]` table of `param` at 0-based bar `start`.
fn lane(param: &str, start: u32, bars: u32, points: &[(f64, f64)]) -> String {
    let points: Vec<String> = points.iter().map(|(b, v)| format!("[{b}, {v}]")).collect();
    format!(
        "\n[[automation]]\nparam = \"{param}\"\nat = {}\nbars = {bars}\npoints = [{}]\n",
        start + 1,
        points.join(", ")
    )
}

/// Track `id`'s placements as [`P`] tuples.
fn plays(song: &Song, id: &str) -> Vec<(u32, u32, String, u32)> {
    song.tracks[id]
        .play
        .iter()
        .map(|p| (p.start_bar, p.bars, p.clip.clone(), p.offset_bars))
        .collect()
}

fn ps(expected: &[P]) -> Vec<(u32, u32, String, u32)> {
    expected
        .iter()
        .map(|&(s, n, c, o)| (s, n, c.to_owned(), o))
        .collect()
}

/// Every lane stretch as `(param, start bar, bars, points)`.
type L = (String, u32, u32, Vec<(f64, f64)>);

fn lanes(song: &Song) -> Vec<L> {
    song.automation
        .iter()
        .map(|l| (l.param.clone(), l.start_bar, l.bars, l.points.clone()))
        .collect()
}

fn l(param: &str, start: u32, bars: u32, points: &[(f64, f64)]) -> L {
    (param.to_owned(), start, bars, points.to_vec())
}

fn bars(song: &Song) -> Vec<(String, u32)> {
    song.sections
        .iter()
        .map(|s| (s.name.clone(), s.bars))
        .collect()
}

fn names(expected: &[(&str, u32)]) -> Vec<(String, u32)> {
    expected.iter().map(|&(n, b)| (n.to_owned(), b)).collect()
}

#[test]
fn split_keeps_the_clip_phase() {
    let s = song(&[2, 2], &kick_hat(&[(0, 4, "c", 0)], &[]), "");
    let a = resize_section(&s, 0, 2).unwrap();
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(0, 2, "c", 0), (2, 2, "c", 2)])
    );
}

/// Lane breakpoints as `(bar offset, value)`.
type Points = &'static [(f64, f64)];

#[test]
fn split_keeps_lane_values() {
    // (sections, stretch bars, points, left piece, right piece)
    let cases: [(&[u32], u32, Points, Points, Points); 3] = [
        (
            &[1, 3],
            4,
            &[(0.0, 0.0), (4.0, 1.0)],
            &[(0.0, 0.0), (1.0, 0.25)],
            &[(0.0, 0.25), (3.0, 1.0)],
        ),
        (
            &[2, 2],
            4,
            &[(0.0, 0.1), (2.0, 0.2), (2.0, 0.3), (4.0, 0.4)],
            &[(0.0, 0.1), (2.0, 0.2)],
            &[(0.0, 0.3), (2.0, 0.4)],
        ),
        (
            &[1, 3],
            4,
            &[(3.0, 0.7)],
            &[(1.0, 0.7)],
            &[(0.0, 0.7), (2.0, 0.7)],
        ),
    ];
    for (sections, n, points, left, right) in cases {
        let s = song(
            sections,
            &kick_hat(&[], &[]),
            &lane("kick.kick.tune", 0, n, points),
        );
        let k = sections[0];
        let a = resize_section(&s, 0, k).unwrap();
        assert_eq!(
            lanes(&a.song),
            [l("kick.tune", 0, k, left), l("kick.tune", k, n - k, right),],
            "{points:?} at {k}"
        );
        assert_eq!(a.lanes, [Kept(0), Split(0)]);
    }
}

#[test]
fn move_section_carries_its_placements_and_lanes() {
    let s = song(
        &[1, 2, 3],
        &kick_hat(&[(0, 1, "c", 0), (1, 2, "d", 0), (3, 3, "c", 1)], &[]),
        &(lane("kick.kick.tune", 0, 1, &[(0.0, 0.5)])
            + &lane("kick.kick.decay", 1, 2, &[(0.0, 0.1), (2.0, 0.9)])),
    );
    let a = move_section(&s, 1, 0).unwrap();
    assert_eq!(bars(&a.song), names(&[("s1", 2), ("s0", 1), ("s2", 3)]));
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(0, 2, "d", 0), (2, 1, "c", 0), (3, 3, "c", 1)])
    );
    // Sorted by the section each stretch starts in.
    assert_eq!(
        lanes(&a.song),
        [
            l("kick.decay", 0, 2, &[(0.0, 0.1), (2.0, 0.9)]),
            l("kick.tune", 2, 1, &[(0.0, 0.5)]),
        ]
    );
    assert_eq!(a.lanes, [Kept(1), Kept(0)]);
}

#[test]
fn move_splits_at_the_insertion_edge() {
    // s0 = bar 0, s1 = bar 1, s2 = bars 2-3, s3 = bars 4-5; the `c` at
    // bar 3 crosses s2|s3.
    let s = song(
        &[1, 1, 2, 2],
        &kick_hat(&[(0, 1, "d", 0), (3, 2, "c", 1)], &[]),
        "",
    );
    let a = move_section(&s, 0, 2).unwrap();
    assert_eq!(
        bars(&a.song),
        names(&[("s1", 1), ("s2", 2), ("s0", 1), ("s3", 2)])
    );
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(2, 1, "c", 1), (3, 1, "d", 0), (4, 1, "c", 2)])
    );
}

#[test]
fn duplicate_copies_contents_and_shifts_the_rest() {
    let s = song(
        &[1, 2, 1],
        &kick_hat(&[(0, 4, "c", 0)], &[]),
        &(lane("kick.kick.tune", 1, 2, &[(0.0, 0.0), (2.0, 1.0)])
            + &lane("kick.kick.decay", 0, 4, &[(0.0, 0.0), (4.0, 1.0)])),
    );
    let a = duplicate_section(&s, 1).unwrap();
    assert_eq!(
        bars(&a.song),
        names(&[("s0", 1), ("s1", 2), ("s1", 2), ("s2", 1)])
    );
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[
            (0, 1, "c", 0),
            (1, 2, "c", 1),
            (3, 2, "c", 1),
            (5, 1, "c", 3)
        ])
    );
    assert_eq!(
        lanes(&a.song),
        [
            l("kick.decay", 0, 1, &[(0.0, 0.0), (1.0, 0.25)]),
            l("kick.tune", 1, 2, &[(0.0, 0.0), (2.0, 1.0)]),
            l("kick.decay", 1, 2, &[(0.0, 0.25), (2.0, 0.75)]),
            l("kick.tune", 3, 2, &[(0.0, 0.0), (2.0, 1.0)]),
            l("kick.decay", 3, 2, &[(0.0, 0.25), (2.0, 0.75)]),
            l("kick.decay", 5, 1, &[(0.0, 0.75), (1.0, 1.0)]),
        ]
    );
    assert_eq!(
        a.lanes,
        [Kept(1), Kept(0), Split(1), Copy(0), Copy(1), Split(1)]
    );
}

#[test]
fn delete_removes_contents_and_shifts_left() {
    let s = song(
        &[1, 2, 1],
        &kick_hat(&[(0, 4, "c", 0)], &[(1, 2, "d", 0), (3, 1, "d", 0)]),
        &(lane("kick.kick.tune", 1, 2, &[(0.0, 0.3)])
            + &lane("kick.kick.tune", 3, 1, &[(0.0, 0.6)])),
    );
    let a = delete_section(&s, 1).unwrap();
    assert_eq!(bars(&a.song), names(&[("s0", 1), ("s2", 1)]));
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(0, 1, "c", 0), (1, 1, "c", 3)])
    );
    assert_eq!(plays(&a.song, "hat"), ps(&[(1, 1, "d", 0)]));
    assert_eq!(lanes(&a.song), [l("kick.tune", 1, 1, &[(0.0, 0.6)])]);
    assert_eq!(a.lanes, [Kept(1)]);
}

#[test]
fn delete_last_section_is_refused() {
    let s = song(&[4], &kick_hat(&[(0, 4, "c", 0)], &[]), "");
    assert_eq!(delete_section(&s, 0), Err(ArrangeError::LastSection));
    let s = song(&[1, 1], &kick_hat(&[], &[]), "");
    assert_eq!(
        delete_section(&s, 5),
        Err(ArrangeError::OutOfBounds { index: 5, len: 2 })
    );
}

#[test]
fn ops_refuse_bad_indices_and_tracks() {
    let s = song(&[1, 1], &kick_hat(&[], &[]), "");
    let oob = |index| Err(ArrangeError::OutOfBounds { index, len: 2 });
    assert_eq!(move_section(&s, 2, 0), oob(2));
    assert_eq!(move_section(&s, 0, 2), oob(2));
    assert_eq!(duplicate_section(&s, 2), oob(2));
    assert_eq!(resize_section(&s, 3, 4), oob(3));
    assert_eq!(set_section_play(&s, 2, "kick", None), oob(2));
    assert_eq!(
        set_section_play(&s, 0, "nope", Some("d")),
        Err(ArrangeError::NoSuchTrack("nope".to_owned()))
    );
}

#[test]
fn resize_grows_the_placement_that_ran_to_the_end() {
    let s = song(
        &[2, 2],
        &kick_hat(
            &[(0, 2, "c", 0), (2, 2, "d", 0)],
            &[(0, 1, "d", 0), (1, 2, "c", 0)],
        ),
        "",
    );
    let a = resize_section(&s, 0, 4).unwrap();
    assert_eq!(bars(&a.song), names(&[("s0", 4), ("s1", 2)]));
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(0, 4, "c", 0), (4, 2, "d", 0)])
    );
    // The `c` crossing the old end is split there: its first piece ran to the
    // end and grows, the rest moves with the next section.
    assert_eq!(
        plays(&a.song, "hat"),
        ps(&[(0, 1, "d", 0), (1, 3, "c", 0), (4, 1, "c", 1)])
    );
}

#[test]
fn resize_shrink_cuts_and_drops() {
    let s = song(
        &[4, 2],
        &kick_hat(&[(0, 1, "d", 0), (1, 2, "c", 0), (3, 3, "c", 0)], &[]),
        &(lane("kick.kick.tune", 0, 4, &[(0.0, 0.0), (4.0, 1.0)])
            + &lane("kick.kick.decay", 3, 1, &[(0.0, 0.5)])
            + &lane("kick.kick.tune", 4, 2, &[(0.0, 0.2)])),
    );
    let a = resize_section(&s, 0, 2).unwrap();
    assert_eq!(bars(&a.song), names(&[("s0", 2), ("s1", 2)]));
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(0, 1, "d", 0), (1, 1, "c", 0), (2, 2, "c", 1)])
    );
    assert_eq!(
        lanes(&a.song),
        [
            l("kick.tune", 0, 2, &[(0.0, 0.0), (2.0, 0.5)]),
            l("kick.tune", 2, 2, &[(0.0, 0.2)]),
        ]
    );
    assert_eq!(a.lanes, [Kept(0), Kept(2)]);
}

#[test]
fn resize_grow_holds_the_last_lane_value() {
    let s = song(
        &[2, 2],
        &kick_hat(&[], &[]),
        &(lane("kick.kick.tune", 0, 2, &[(0.0, 0.1), (1.0, 0.5)])
            + &lane("kick.kick.tune", 2, 2, &[(0.0, 0.9)])),
    );
    let a = resize_section(&s, 0, 3).unwrap();
    assert_eq!(
        lanes(&a.song),
        [
            l("kick.tune", 0, 3, &[(0.0, 0.1), (1.0, 0.5)]),
            l("kick.tune", 3, 2, &[(0.0, 0.9)]),
        ]
    );
    assert_eq!(a.lanes, [Kept(0), Kept(1)]);
}

#[test]
fn set_section_play_replaces_placements() {
    let s = song(
        &[1, 2, 1],
        &kick_hat(&[(0, 4, "c", 0)], &[(1, 2, "d", 0)]),
        &lane("kick.kick.tune", 0, 4, &[(0.0, 0.0), (4.0, 1.0)]),
    );
    let a = set_section_play(&s, 1, "kick", Some("d")).unwrap();
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(0, 1, "c", 0), (1, 2, "d", 0), (3, 1, "c", 3)])
    );
    assert_eq!(plays(&a.song, "hat"), ps(&[(1, 2, "d", 0)]));
    assert_eq!(a.song.automation, s.automation);
    assert_eq!(a.lanes, [Kept(0)]);

    let a = set_section_play(&s, 1, "kick", None).unwrap();
    assert_eq!(
        plays(&a.song, "kick"),
        ps(&[(0, 1, "c", 0), (3, 1, "c", 3)])
    );
}

/// The clip track `track` plays at song step `step` and the step of that
/// clip, as the engine finds them: the clip looped from the placement's
/// start, `offset` bars in.
fn step_of(song: &Song, track: &str, step: u64) -> Option<(String, u64)> {
    let bar = u32::try_from(step / 16).unwrap();
    let p = song.tracks[track]
        .play
        .iter()
        .find(|p| p.span().contains(&bar))?;
    let len = match &song.clips[&p.clip] {
        Clip::Acid(a) => a.steps().len(),
        Clip::Drum(d) => d.hits().len(),
    } as u64;
    let offset = step - 16 * u64::from(p.start_bar) + 16 * u64::from(p.offset_bars);
    Some((p.clip.clone(), offset % len))
}

/// Asserts that every old bar `b` that `map` sends to new bars plays the
/// same clip steps there, on every track.
fn assert_steps_kept(op: &str, old: &Song, new: &Arranged, map: impl Fn(u32) -> Vec<u32>) {
    for track in old.tracks.keys() {
        for bar in 0..old.total_bars() {
            for to in map(bar) {
                for i in 0..16 {
                    let before = step_of(old, track, 16 * u64::from(bar) + i);
                    let after = step_of(&new.song, track, 16 * u64::from(to) + i);
                    assert_eq!(before, after, "{op}: {track} bar {bar} -> {to}, step {i}");
                }
            }
        }
    }
}

#[test]
fn ops_keep_steps_outside_the_edit() {
    // Placements that cross section edges, with offsets.
    let sections = [2, 3, 1, 2];
    let s = song(
        &sections,
        &kick_hat(
            &[(0, 3, "c", 1), (3, 4, "c", 2), (7, 1, "d", 0)],
            &[(1, 6, "c", 3)],
        ),
        "",
    );
    let n = sections.len();
    let starts = s.section_start_bars();
    let range = |i: usize| starts[i]..starts[i] + sections[i];

    for from in 0..n {
        for to in 0..n {
            let a = move_section(&s, from, to).unwrap();
            let mut order: Vec<usize> = (0..n).collect();
            let moved = order.remove(from);
            order.insert(to, moved);
            let new_start = |j: usize| -> u32 {
                let pos = order.iter().position(|&o| o == j).unwrap();
                order[..pos].iter().map(|&o| sections[o]).sum()
            };
            assert_steps_kept(&format!("move {from} {to}"), &s, &a, |bar| {
                let j = (0..n).find(|&j| range(j).contains(&bar)).unwrap();
                vec![new_start(j) + bar - starts[j]]
            });
        }
    }

    for (i, &len) in sections.iter().enumerate() {
        let r = range(i);

        let a = duplicate_section(&s, i).unwrap();
        assert_steps_kept(&format!("duplicate {i}"), &s, &a, |bar| {
            if bar < r.start {
                vec![bar]
            } else if r.contains(&bar) {
                vec![bar, bar + len]
            } else {
                vec![bar + len]
            }
        });

        let a = delete_section(&s, i).unwrap();
        assert_steps_kept(&format!("delete {i}"), &s, &a, |bar| {
            if bar < r.start {
                vec![bar]
            } else if r.contains(&bar) {
                vec![]
            } else {
                vec![bar - len]
            }
        });

        for new in 1..=5 {
            let a = resize_section(&s, i, new).unwrap();
            assert_steps_kept(&format!("resize {i} to {new}"), &s, &a, |bar| {
                if bar < r.start + new.min(len) {
                    vec![bar]
                } else if r.contains(&bar) {
                    vec![]
                } else {
                    vec![bar + new - len]
                }
            });
        }

        for clip in [Some("d"), None] {
            let a = set_section_play(&s, i, "kick", clip).unwrap();
            assert_steps_kept(&format!("play {i} {clip:?}"), &s, &a, |bar| {
                if r.contains(&bar) { vec![] } else { vec![bar] }
            });
        }
    }
}

/// Every op at every place in `src`, each with a name.
fn every_op(s: &Song) -> Vec<(String, Arranged)> {
    let n = s.sections.len();
    let mut out = Vec::new();
    for i in 0..n {
        for to in 0..n {
            out.push((format!("move {i} {to}"), move_section(s, i, to).unwrap()));
        }
        out.push((format!("duplicate {i}"), duplicate_section(s, i).unwrap()));
        out.push((format!("delete {i}"), delete_section(s, i).unwrap()));
        let bars = s.sections[i].bars;
        for new in [1, bars / 2, bars + 8] {
            out.push((
                format!("resize {i} to {new}"),
                resize_section(s, i, new).unwrap(),
            ));
        }
        for (id, t) in &s.tracks {
            let clip = t.play.first().map(|p| p.clip.as_str());
            for clip in [clip, None] {
                out.push((
                    format!("play {i} {id} {clip:?}"),
                    set_section_play(s, i, id, clip).unwrap(),
                ));
            }
        }
    }
    out
}

#[test]
fn ops_results_validate() {
    let fixtures = [
        include_str!("fixtures/v2/first-acid-v1.toml"),
        include_str!("fixtures/v2/first-acid-mixed.toml"),
    ];
    for src in fixtures {
        let s = parse_song(src).unwrap();
        for (op, a) in every_op(&s) {
            assert_eq!(a.lanes.len(), a.song.automation.len(), "{op}");
            let text = a.song.to_toml();
            let back = parse_song(&text).unwrap_or_else(|e| panic!("{op}: {e:?}\n{text}"));
            assert_eq!(back, a.song, "{op}");
        }
    }
}
