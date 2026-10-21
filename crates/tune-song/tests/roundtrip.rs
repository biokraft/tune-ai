//! The canonical serializer: exact layout, round trips and number fidelity.

use tune_song::{Song, parse_song};

const MINI: &str = include_str!("fixtures/mini.toml");

const CANONICAL: &str = r#"[song]
name = "mini"
bpm = 120
seed = 7

[tracks.acid]
instrument = "acid303"
gain_db = 0
pan = 0
mute = false
solo = false
params = { "acid303.cutoff" = 500 }
fx = [{ type = "distortion", mode = "soft", params = { "dist.drive" = 12 } }]

[patterns.a1]
track = "acid"
steps = "C2 C2! C3~ ."

[[sections]]
name = "a"
bars = 2
play = { acid = "a1" }

[[sections.automation]]
param = "acid.acid303.cutoff"
points = [[0, 0.1], [2, 0.8]]
"#;

fn parse(src: &str) -> Song {
    parse_song(src).unwrap_or_else(|e| panic!("must parse: {e:?}\n{src}"))
}

#[test]
fn canonical_layout() {
    assert_eq!(parse(CANONICAL).to_toml(), CANONICAL);
}

#[test]
fn round_trip_mini() {
    let song = parse(MINI);
    assert_eq!(parse(&song.to_toml()), song);
}

#[test]
fn canonical_is_stable() {
    let once = parse(MINI).to_toml();
    let twice = parse(&once).to_toml();
    assert_eq!(once, twice);
}

#[test]
fn numbers_round_trip_exactly() {
    let mut song = parse(MINI);
    let acid = song.tracks.get_mut("acid").unwrap();
    acid.params.clear();
    acid.params.insert("acid303.cutoff".into(), 2394.0);
    acid.params.insert("acid303.resonance".into(), 1.0 / 3.0);
    acid.gain_db = 0.1;
    acid.pan = 1e-7;
    let back = parse(&song.to_toml());
    let got = &back.tracks["acid"];
    assert_eq!(got.params["acid303.cutoff"].to_bits(), 2394.0_f64.to_bits());
    assert_eq!(
        got.params["acid303.resonance"].to_bits(),
        (1.0_f64 / 3.0).to_bits()
    );
    assert_eq!(got.gain_db.to_bits(), 0.1_f64.to_bits());
    assert_eq!(got.pan.to_bits(), 1e-7_f64.to_bits());
}

#[test]
fn huge_and_tiny_numbers_stay_valid_toml() {
    let mut song = parse(MINI);
    song.bpm = 120.5;
    song.tracks.get_mut("acid").unwrap().params.clear();
    song.tracks
        .get_mut("acid")
        .unwrap()
        .params
        .insert("acid303.cutoff".into(), 1e300);
    let text = song.to_toml();
    assert!(text.parse::<toml::Table>().is_ok(), "{text}");
}

#[test]
fn strings_escaped() {
    let mut song = parse(MINI);
    song.name = "Acid \"Mk2\" ü \\ end".into();
    let back = parse(&song.to_toml());
    assert_eq!(back.name, song.name);
}
