//! The canonical serializer: exact layout, round trips and number fidelity.

use tune_song::{Placement, Song, parse_song, placements_toml};

const MINI: &str = include_str!("fixtures/mini.toml");

const CANONICAL: &str = r#"[song]
version = 2
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
play = [{ at = 1, clip = "a1", bars = 2 }]

[clips.a1]
kind = "acid"
steps = "C2 C2! C3~ ."

[[sections]]
name = "a"
bars = 2

[[automation]]
param = "acid.acid303.cutoff"
at = 1
bars = 2
points = [[0, 0.1], [2, 0.8]]
"#;

/// Two eqs, an instrument with its own id, a placement starting mid-clip
/// and lanes with gaps, out of time order.
const V2_SHAPES: &str = r#"[song]
version = 2
name = "shapes"
bpm = 130
seed = 3

[tracks.kick]
instrument = "kick"
play = [
  { at = 1, clip = "k4", bars = 2 },
  { at = 4, clip = "k4", bars = 3, offset = 1 },
]

[tracks.acid]
instrument = { type = "acid303", id = "bass" }
params = { "send.delay" = -12, "bass.cutoff" = 700 }
fx = [{ type = "eq", params = { "eq.low_cut" = 85 } }, { type = "distortion", id = "drive", mode = "hard" }, { type = "eq", id = "eq2", params = { "eq2.mid_gain" = 3 } }]
play = [{ at = 2, clip = "a1", bars = 4, offset = 2 }]

[clips.k4]
kind = "drum"
steps = "x...x...x...X..."

[clips.a1]
kind = "acid"
steps = "C2 C2! C3~ ."

[[sections]]
name = "a"
bars = 3

[[sections]]
name = "b"
bars = 3

[[automation]]
param = "acid.bass.cutoff"
at = 5
bars = 2
points = [[0, 0.2], [2, 0.9]]

[[automation]]
param = "acid.eq2.mid_gain"
at = 2
bars = 1
points = [[0, 0.5]]

[[automation]]
param = "acid.bass.cutoff"
at = 1
bars = 2
points = [[0, 0.1], [1, 0.3], [1, 0.6]]

[[automation]]
param = "mix.reverb.size"
at = 1
bars = 6
points = [[0, 0.4]]
"#;

fn parse(src: &str) -> Song {
    parse_song(src).unwrap_or_else(|e| panic!("must parse: {e:?}\n{src}"))
}

/// The song `src` describes and the format version it is written in.
fn parse_v(src: &str) -> (Song, u32) {
    let p = tune_song::parse(src).unwrap_or_else(|e| panic!("must parse: {e:?}\n{src}"));
    (p.song, p.version)
}

#[test]
fn canonical_layout() {
    assert_eq!(parse(CANONICAL).to_toml(), CANONICAL);
}

#[test]
fn round_trip_mini() {
    let song = parse(MINI);
    let text = song.to_toml();
    assert_eq!(parse_v(&text), (song, 2));
}

#[test]
fn round_trip_v2_shapes() {
    let song = parse(V2_SHAPES);
    let acid = &song.tracks["acid"];
    assert_eq!(acid.instrument.id, "bass");
    let ids: Vec<&str> = acid.fx.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["eq", "drive", "eq2"]);
    assert_eq!(song.tracks["kick"].play[1].offset_bars, 1);
    let text = song.to_toml();
    assert_eq!(parse_v(&text), (song, 2));
    assert_eq!(parse(&text).to_toml(), text);
    assert!(
        text.contains("\n[tracks.acid]\ninstrument = { type = \"acid303\", id = \"bass\" }\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "fx = [{ type = \"eq\", params = { \"eq.low_cut\" = 85 } }, { type = \"distortion\", id = \"drive\", mode = \"hard\" }, { type = \"eq\", id = \"eq2\", params = { \"eq2.mid_gain\" = 3 } }]\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("play = [{ at = 2, clip = \"a1\", bars = 4, offset = 2 }]\n"),
        "{text}"
    );
}

#[test]
fn placements_layout() {
    let k4 = |at: u32| Placement {
        start_bar: at - 1,
        bars: 2,
        clip: "k4".into(),
        offset_bars: 0,
    };
    assert_eq!(
        placements_toml(&[k4(1)]),
        "[{ at = 1, clip = \"k4\", bars = 2 }]"
    );
    assert_eq!(
        placements_toml(&[k4(1), k4(3)]),
        "[\n  { at = 1, clip = \"k4\", bars = 2 },\n  { at = 3, clip = \"k4\", bars = 2 },\n]"
    );
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
    acid.instrument.params.clear();
    acid.instrument.params.insert("cutoff".into(), 2394.0);
    acid.instrument.params.insert("resonance".into(), 1.0 / 3.0);
    acid.gain_db = 0.1;
    acid.pan = 1e-7;
    let back = parse(&song.to_toml());
    let got = &back.tracks["acid"].instrument;
    assert_eq!(got.params["cutoff"].to_bits(), 2394.0_f64.to_bits());
    assert_eq!(got.params["resonance"].to_bits(), (1.0_f64 / 3.0).to_bits());
    let got = &back.tracks["acid"];
    assert_eq!(got.gain_db.to_bits(), 0.1_f64.to_bits());
    assert_eq!(got.pan.to_bits(), 1e-7_f64.to_bits());
}

#[test]
fn huge_and_tiny_numbers_stay_valid_toml() {
    let mut song = parse(MINI);
    song.bpm = 120.5;
    let acid = &mut song.tracks.get_mut("acid").unwrap().instrument;
    acid.params.clear();
    acid.params.insert("cutoff".into(), 1e300);
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

const MIXED: &str = include_str!("fixtures/mixed.toml");

#[test]
fn mixed_fixture_round_trips() {
    let song = parse(MIXED);
    let once = song.to_toml();
    assert_eq!(parse(&once), song);
    assert_eq!(parse(&once).to_toml(), once);
    assert!(once.contains(
        "\n[mix]\nmaster = \"limit\"\nparams = { \"delay.feedback\" = 40, \"reverb.size\" = 30, \"master.gain\" = 6 }\n\n[tracks.kick]"
    ), "{once}");
    assert!(once.contains(
        "fx = [{ type = \"distortion\", mode = \"soft\", params = { \"dist.drive\" = 12 } }, { type = \"eq\", params = { \"eq.low_cut\" = 85 } }]"
    ), "{once}");
}

#[test]
fn mix_writes_duck_source_and_bare_eq() {
    let mut song = parse(MINI);
    song.mix.duck_source = Some("hat".into());
    if let Some(acid) = song.tracks.get_mut("acid") {
        acid.fx
            .push(tune_song::Device::new("eq", &tune_modules::EQ));
    }
    let text = song.to_toml();
    assert!(
        text.contains("seed = 7\n\n[mix]\nmaster = \"clip\"\nduck_source = \"hat\"\n\n[tracks."),
        "{text}"
    );
    assert!(text.contains(", { type = \"eq\" }]"), "{text}");
    assert_eq!(parse(&text), song);
}

#[test]
fn no_mix_writes_no_mix_table() {
    let text = parse(MINI).to_toml();
    assert!(!text.contains("[mix]"), "{text}");
}
