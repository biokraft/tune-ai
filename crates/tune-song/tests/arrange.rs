//! Section operations over placements and lane stretches: every item that
//! crosses an edit's edge is split there first, so everything outside the
//! edited section plays as before.

use tune_song::{
    ArrangeError, Arranged, Clip, LaneOrigin, Song, delete_section, duplicate_section, lane_value,
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
    song_at(120, sections, tracks, automation)
}

/// [`song`] at `bpm`.
fn song_at(bpm: u32, sections: &[u32], tracks: &str, automation: &str) -> Song {
    let mut text = format!("[song]\nversion = 2\nname = \"t\"\nbpm = {bpm}\n\n");
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
fn resize_refuses_out_of_range_bars() {
    let s = song(&[2, 2], &kick_hat(&[(0, 4, "c", 0)], &[]), "");
    for bars in [0, 1025, u32::MAX] {
        for index in [0, 1] {
            assert_eq!(
                resize_section(&s, index, bars),
                Err(ArrangeError::Bars { bars })
            );
        }
    }
    assert_eq!(
        ArrangeError::Bars { bars: 0 }.to_string(),
        "bars must be 1 to 1024, got 0"
    );
    assert!(resize_section(&s, 0, 1024).is_ok());
}

#[test]
fn ops_refuse_results_past_u32_bars() {
    // Only a song built in code gets here: the validator caps a song at
    // 2250 bars (30 minutes at 300 bpm).
    let mut s = song(&[1, 1], &kick_hat(&[], &[]), "");
    s.sections[1].bars = u32::MAX - 10;
    assert!(move_section(&s, 0, 1).is_ok());
    assert!(delete_section(&s, 0).is_ok());
    assert_eq!(duplicate_section(&s, 1), Err(ArrangeError::TooLong));
    assert_eq!(resize_section(&s, 0, 1024), Err(ArrangeError::TooLong));
    assert!(resize_section(&s, 0, 5).is_ok());

    let mut s = song(&[2], &kick_hat(&[(0, 2, "c", 0)], &[]), "");
    s.tracks["kick"].play[0].offset_bars = u32::MAX - 1;
    assert_eq!(resize_section(&s, 0, 1), Err(ArrangeError::TooLong));
}

#[test]
fn ops_on_a_song_at_the_validators_maximum() {
    // 2250 bars at 300 bpm is exactly 30 minutes; offsets at their maximum.
    let mut s = song_at(
        300,
        &[1024, 1024, 202],
        &kick_hat(
            &[(1000, 1100, "c", 1024), (2100, 150, "d", 1024)],
            &[(0, 2250, "c", 1024)],
        ),
        &(lane(
            "kick.kick.tune",
            500,
            1750,
            &[(0.0, 0.0), (1000.0, 0.5), (1000.0, 0.7), (1750.0, 1.0)],
        ) + &lane(
            "hat.hat_closed.decay",
            0,
            2250,
            &[(0.0, 0.2), (2250.0, 0.8)],
        )),
    );
    // Each round moves the edges, so the next round splits the pieces again.
    for round in 0..6 {
        for (op, a) in every_op(&s) {
            assert_eq!(a.lanes.len(), a.song.automation.len(), "{round} {op}");
        }
        let last = s.sections.len() - 1;
        let moved = move_section(&s, 0, last).unwrap().song;
        s = resize_section(&moved, 0, 1000 + round).unwrap().song;
    }
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

/// The value the lane of `param` on `track` has at 0-based song bar `x`, as
/// the engine finds it: from the stretch that covers `x`, at `x`'s offset
/// into it; `None` outside every stretch (the static value plays).
fn value_at(song: &Song, track: &str, param: &str, x: f64) -> Option<f64> {
    let lane = song.automation.iter().find(|l| {
        l.track == track
            && l.param == param
            && f64::from(l.start_bar) <= x
            && x < f64::from(l.start_bar + l.bars)
    })?;
    Some(lane_value(&lane.points, x - f64::from(lane.start_bar)))
}

/// Asserts that every old bar `b` that `map` sends to new bars plays the
/// same clip steps and lane values there, on every track and lane, and that
/// every stretch comes from a stretch of the same param.
fn assert_kept(op: &str, old: &Song, new: &Arranged, map: impl Fn(u32) -> Vec<u32>) {
    assert_eq!(new.lanes.len(), new.song.automation.len(), "{op}");
    for (lane, &origin) in new.song.automation.iter().zip(&new.lanes) {
        let (Kept(i) | Split(i) | Copy(i)) = origin;
        let from = &old.automation[i];
        assert_eq!(
            (&lane.track, &lane.param),
            (&from.track, &from.param),
            "{op}: {origin:?}"
        );
    }
    let mut params: Vec<(&str, &str)> = old
        .automation
        .iter()
        .map(|l| (l.track.as_str(), l.param.as_str()))
        .collect();
    params.sort_unstable();
    params.dedup();
    for bar in 0..old.total_bars() {
        for to in map(bar) {
            for track in old.tracks.keys() {
                for i in 0..16 {
                    let before = step_of(old, track, 16 * u64::from(bar) + i);
                    let after = step_of(&new.song, track, 16 * u64::from(to) + i);
                    assert_eq!(before, after, "{op}: {track} bar {bar} -> {to}, step {i}");
                }
            }
            for &(track, param) in &params {
                for q in 0..8 {
                    let f = f64::from(q) / 8.0;
                    let before = value_at(old, track, param, f64::from(bar) + f);
                    let after = value_at(&new.song, track, param, f64::from(to) + f);
                    let same = match (before, after) {
                        (Some(a), Some(b)) => (a - b).abs() < 1e-9,
                        (a, b) => a == b,
                    };
                    assert!(
                        same,
                        "{op}: {track}.{param} at bar {bar} + {f} -> {to}: {before:?} vs {after:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn ops_keep_steps_outside_the_edit() {
    // Placements and stretches that cross section edges (bars 2, 5 and 6):
    // placements with offsets, a stretch with jumps (two points on one bar)
    // on the edge at bar 2 and inside at bar 4, another stretch touching it,
    // and one over the whole song on a second param.
    let sections = [2, 3, 1, 2];
    let s = song(
        &sections,
        &kick_hat(
            &[(0, 3, "c", 1), (3, 4, "c", 2), (7, 1, "d", 0)],
            &[(1, 6, "c", 3)],
        ),
        &(lane(
            "kick.kick.tune",
            1,
            5,
            &[
                (0.0, 0.1),
                (1.0, 0.4),
                (1.0, 0.8),
                (3.0, 0.6),
                (3.0, 0.3),
                (5.0, 0.2),
            ],
        ) + &lane("kick.kick.tune", 6, 2, &[(0.0, 0.5), (2.0, 0.9)])
            + &lane(
                "hat.hat_closed.decay",
                0,
                8,
                &[(0.0, 0.0), (4.0, 0.9), (8.0, 0.1)],
            )),
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
            assert_kept(&format!("move {from} {to}"), &s, &a, |bar| {
                let j = (0..n).find(|&j| range(j).contains(&bar)).unwrap();
                vec![new_start(j) + bar - starts[j]]
            });
        }
    }

    for (i, &len) in sections.iter().enumerate() {
        let r = range(i);

        let a = duplicate_section(&s, i).unwrap();
        assert_kept(&format!("duplicate {i}"), &s, &a, |bar| {
            if bar < r.start {
                vec![bar]
            } else if r.contains(&bar) {
                vec![bar, bar + len]
            } else {
                vec![bar + len]
            }
        });

        let a = delete_section(&s, i).unwrap();
        assert_kept(&format!("delete {i}"), &s, &a, |bar| {
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
            assert_kept(&format!("resize {i} to {new}"), &s, &a, |bar| {
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
            assert_kept(&format!("play {i} {clip:?}"), &s, &a, |bar| {
                if r.contains(&bar) { vec![] } else { vec![bar] }
            });
        }
    }
}

/// Every op at every place in `s`, each with a name.
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
        for new in [1, (bars / 2).max(1), (bars + 8).min(1024)] {
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

/// The section ops and the validator refuse a section length the same
/// way, in one wording.
#[test]
fn bars_out_of_range_reads_like_the_validator() {
    assert_eq!(
        tune_song::section_bars_error(0),
        "bars must be 1 to 1024, got 0"
    );
    assert_eq!(
        tune_song::ArrangeError::Bars { bars: 2000 }.to_string(),
        tune_song::section_bars_error(2000)
    );
    let src =
        "[song]\nversion = 2\nname = \"x\"\nbpm = 120\n\n[[sections]]\nname = \"a\"\nbars = 0\n";
    let errors = tune_song::parse(src).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message == tune_song::section_bars_error(0)),
        "{errors:?}"
    );
}
