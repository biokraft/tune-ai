//! A hot reload that changes a track's signal path crossfades that track
//! over 10 ms instead of switching at once (spec §7.1): on a steady test
//! tone, the largest sample-to-sample jump in the 20 ms after the swap is no
//! bigger than 1.5 times the largest in the 100 ms before it. A swap that
//! changes only values switches at the step, and a device that cannot copy
//! its state switches at once.

use std::ops::Range;
use std::sync::Arc;

use tune_dsp::{Curve, ParamSpec};
use tune_engine::{
    CompiledSong, Engine, LiveProcessor, RenderOptions, SWAP_FADE_MS, SongUpdate, live, render,
};
use tune_modules::{
    ClipKind, Effect, EventCtx, Instrument, ModuleDescriptor, NoteEvent, Role, SettingSpec,
    copy_state,
};
use tune_song::{Registry, parse_with};

const RATE: u32 = 48_000;
/// One bar at 120 BPM and 48 kHz.
const BAR: u64 = 96_000;
/// One step at 120 BPM and 48 kHz.
const STEP: u64 = 6000;
/// The fade length at 48 kHz.
const FADE: u64 = 480;

static TONE_PARAMS: &[ParamSpec] = &[ParamSpec {
    id: "tone.freq",
    label: "Frequency",
    unit: "Hz",
    min: 20.0,
    max: 2000.0,
    curve: Curve::Exponential,
    default: 440.0,
    help: "Pitch of the tone.",
}];

/// A steady tone: the first On starts `0.25 * sin|cos(2π f n / rate)`, `n`
/// counting samples from that On; it never stops and ignores Offs.
struct Tone {
    rate: f64,
    freq: f64,
    cos: bool,
    n: Option<u64>,
}

impl Instrument for Tone {
    fn params(&self) -> &'static [ParamSpec] {
        TONE_PARAMS
    }
    fn set_param_at(&mut self, _index: usize, plain: f64) {
        self.freq = plain;
    }
    fn note(&mut self, event: &NoteEvent, _ctx: &EventCtx) {
        if matches!(event, NoteEvent::On { .. }) && self.n.is_none() {
            self.n = Some(0);
        }
    }
    fn process(&mut self) -> f64 {
        let Some(n) = self.n else {
            return 0.0;
        };
        self.n = Some(n + 1);
        let phase = std::f64::consts::TAU * self.freq * n as f64 / self.rate;
        0.25 * if self.cos { phase.cos() } else { phase.sin() }
    }
    fn reset(&mut self) {
        self.n = None;
    }
}

/// `y = -x`.
#[derive(Clone)]
struct Flip;

impl Effect for Flip {
    fn params(&self) -> &'static [ParamSpec] {
        &[]
    }
    fn set_param_at(&mut self, _: usize, _: f64) {}
    fn process(&mut self, x: f64) -> f64 {
        -x
    }
    fn reset(&mut self) {}
    fn copy_state_to(&self, to: &mut dyn Effect) -> bool {
        copy_state(self, to)
    }
}

/// `y = x + count * 1e-6`, then `count += 1`: a fresh one restarts at 0.
#[derive(Clone)]
struct Ramp {
    count: u64,
}

impl Effect for Ramp {
    fn params(&self) -> &'static [ParamSpec] {
        &[]
    }
    fn set_param_at(&mut self, _: usize, _: f64) {}
    fn process(&mut self, x: f64) -> f64 {
        let y = x + self.count as f64 * 1e-6;
        self.count += 1;
        y
    }
    fn reset(&mut self) {
        self.count = 0;
    }
    fn copy_state_to(&self, to: &mut dyn Effect) -> bool {
        copy_state(self, to)
    }
}

/// `y = x`, with the default `copy_state_to` (it cannot copy).
struct NoCopy;

impl Effect for NoCopy {
    fn params(&self) -> &'static [ParamSpec] {
        &[]
    }
    fn set_param_at(&mut self, _: usize, _: f64) {}
    fn process(&mut self, x: f64) -> f64 {
        x
    }
    fn reset(&mut self) {}
}

static TONE: ModuleDescriptor = ModuleDescriptor {
    name: "tone",
    prefix: "tone",
    params: TONE_PARAMS,
    settings: &[SettingSpec {
        name: "phase",
        values: &["sin", "cos"],
        default: "sin",
        help: "Whether the tone starts as a sine or a cosine.",
    }],
    role: Role::Instrument {
        plays: &[ClipKind::Drum],
        default_duck_source: false,
        make: |rate, settings| {
            Box::new(Tone {
                rate,
                freq: 440.0,
                cos: settings[0] == "cos",
                n: None,
            })
        },
    },
};

static FLIP: ModuleDescriptor = ModuleDescriptor {
    name: "flip",
    prefix: "flip",
    params: &[],
    settings: &[],
    role: Role::Effect {
        make: |_, _| Box::new(Flip),
    },
};

static RAMP: ModuleDescriptor = ModuleDescriptor {
    name: "ramp",
    prefix: "ramp",
    params: &[],
    settings: &[],
    role: Role::Effect {
        make: |_, _| Box::new(Ramp { count: 0 }),
    },
};

static NOCOPY: ModuleDescriptor = ModuleDescriptor {
    name: "nocopy",
    prefix: "nocopy",
    params: &[],
    settings: &[],
    role: Role::Effect {
        make: |_, _| Box::new(NoCopy),
    },
};

const FLIP_FX: &str = r#"{ type = "flip" }"#;
const RAMP_FX: &str = r#"{ type = "ramp" }"#;
const NOCOPY_FX: &str = r#"{ type = "nocopy" }"#;
const EQ_200: &str = r#"{ type = "eq", params = { "eq.low_cut" = 200 } }"#;
const EQ_FLAT: &str = r#"{ type = "eq" }"#;
const HARD_18: &str = r#"{ type = "distortion", mode = "hard", params = { "dist.drive" = 18 } }"#;
const SOFT_18: &str = r#"{ type = "distortion", mode = "soft", params = { "dist.drive" = 18 } }"#;

/// A `tone` track `id` with `phase`, the chain `fx` and `extra` keys, playing
/// `x` (a hit every step) over both bars.
fn track(id: &str, phase: &str, fx: &[&str], extra: &str) -> String {
    format!(
        r#"
[tracks.{id}]
instrument = {{ type = "tone", phase = "{phase}" }}
fx = [{}]
play = [{{ at = 1, clip = "x", bars = 2 }}]
{extra}
"#,
        fx.join(", ")
    )
}

/// The song of `tracks` at 120 BPM, two bars long.
fn song(tracks: &[String]) -> Arc<CompiledSong> {
    let text = format!(
        r#"
[song]
version = 2
name = "crossfade"
bpm = 120
{}
[clips.x]
kind = "drum"
steps = "xxxxxxxxxxxxxxxx"

[[sections]]
name = "a"
bars = 2
"#,
        tracks.concat()
    );
    let registry = Registry::builtin()
        .with(&TONE)
        .with(&FLIP)
        .with(&RAMP)
        .with(&NOCOPY);
    let parsed = parse_with(&text, &registry).unwrap_or_else(|e| panic!("song parses: {e:?}"));
    Arc::new(CompiledSong::new(&parsed.song, RATE))
}

/// Track `t` (phase `cos`) through `fx`.
fn t(fx: &[&str]) -> Arc<CompiledSong> {
    song(&[track("t", "cos", fx, "")])
}

fn offline(song: &Arc<CompiledSong>) -> Vec<f32> {
    render(
        song.clone(),
        &RenderOptions {
            start_bar: 0,
            end_bar: 2,
            stems: false,
        },
    )
    .unwrap()
    .mix
}

fn play(p: &mut LiveProcessor, frames: u64, block: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; 2 * frames as usize];
    for chunk in out.chunks_mut(2 * block) {
        p.process(chunk);
    }
    out
}

/// The step start after sample `pos` of `song`.
fn next_step_start(song: &CompiledSong, pos: u64) -> u64 {
    song.clock.step_start(song.clock.step_at(pos) + 1)
}

/// Plays `a` live for one bar and 1000 frames, queues `b` and plays two more
/// steps. Returns the mix and the swap sample `S` (the next step start).
fn swap_at(a: &Arc<CompiledSong>, b: &Arc<CompiledSong>) -> (Vec<f32>, u64) {
    let queued = BAR + 1000;
    let s = next_step_start(a, queued);
    let (mut ctl, mut p) = live(a.clone(), 0, None);
    let mut out = play(&mut p, queued, 500);
    ctl.swap(b.clone()).unwrap();
    out.extend(play(&mut p, 2 * STEP, 500));
    assert_eq!(ctl.swaps_applied(), 1);
    (out, s)
}

fn left(mix: &[f32], frame: u64) -> f64 {
    f64::from(mix[2 * frame as usize])
}

/// The largest jump `|x[n] - x[n - 1]|` of the left channel for `n` in
/// `frames`.
fn max_jump(mix: &[f32], frames: Range<u64>) -> f64 {
    frames
        .map(|n| (left(mix, n) - left(mix, n - 1)).abs())
        .fold(0.0, f64::max)
}

/// Every structural swap of the spec on the tone, by name. The fx pairs
/// play a cosine, which peaks at every step start (55 cycles a step), so an
/// instant switch there jumps.
fn structural_pairs() -> Vec<(&'static str, Arc<CompiledSong>, Arc<CompiledSong>)> {
    let added = track("u", "cos", &[], "gain_db = -6");
    vec![
        ("fx added", t(&[]), t(&[FLIP_FX])),
        ("fx removed", t(&[FLIP_FX]), t(&[])),
        ("fx reordered", t(&[EQ_200, HARD_18]), t(&[HARD_18, EQ_200])),
        ("mode changed", t(&[SOFT_18]), t(&[HARD_18])),
        (
            "instrument swapped",
            song(&[track("t", "sin", &[], "")]),
            song(&[track("t", "cos", &[], "")]),
        ),
        // A sine under the added cosine: in phase, the two would sum to a
        // louder tone with larger steady jumps.
        (
            "track added",
            song(&[track("t", "sin", &[], "")]),
            song(&[track("t", "sin", &[], ""), added.clone()]),
        ),
        (
            "track removed",
            song(&[track("t", "sin", &[], ""), added]),
            song(&[track("t", "sin", &[], "")]),
        ),
    ]
}

#[test]
fn the_fade_lasts_ten_ms() {
    assert_eq!(SWAP_FADE_MS, 10.0);
    assert_eq!(
        (SWAP_FADE_MS * 0.001 * f64::from(RATE)).round(),
        FADE as f64
    );
}

#[test]
fn no_click_at_a_structural_swap() {
    for (name, a, b) in structural_pairs() {
        let (mix, s) = swap_at(&a, &b);
        let before = max_jump(&mix, s - 4800..s);
        let after = max_jump(&mix, s..s + 960);
        assert!(before > 0.0, "{name}: the tone sounds");
        assert!(
            after <= 1.5 * before,
            "{name}: jump {after} after the swap, {before} before"
        );
    }
}

#[test]
fn a_hard_switch_would_click() {
    for (name, a, b) in structural_pairs().into_iter().take(2) {
        let (ra, rb) = (offline(&a), offline(&b));
        let s = next_step_start(&a, BAR + 1000);
        let before = max_jump(&ra, s - 4800..s);
        let jump = (left(&rb, s) - left(&ra, s - 1)).abs();
        assert!(
            jump > 1.5 * before,
            "{name}: an instant switch jumps {jump}, {before} before"
        );
    }
}

#[test]
fn the_fade_blends_old_and_new() {
    let (a, b) = (t(&[]), t(&[FLIP_FX]));
    let (ra, rb) = (offline(&a), offline(&b));
    let (mix, s) = swap_at(&a, &b);
    for n in 0..FADE {
        let w = (n + 1) as f64 / FADE as f64;
        for c in 0..2 {
            let i = 2 * (s + n) as usize + c;
            let (x, y) = (f64::from(ra[i]), f64::from(rb[i]));
            let want = x + w * (y - x);
            assert!(
                (f64::from(mix[i]) - want).abs() <= 1e-6,
                "frame S + {n}: {} vs {want}",
                mix[i]
            );
        }
    }
    let rest = 2 * (s + FADE) as usize..mix.len();
    let bits = |x: &[f32]| x.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(
        bits(&mix[rest.clone()]),
        bits(&rb[rest]),
        "b from S + 480 on"
    );
}

#[test]
fn kept_devices_keep_state_through_the_fade() {
    // A silent tone: the output is the ramp alone.
    let silent = |fx: &[&str]| {
        song(&[format!(
            r#"
[tracks.t]
instrument = "tone"
fx = [{}]
"#,
            fx.join(", ")
        )])
    };
    let a = silent(&[RAMP_FX, EQ_FLAT]);
    let b = silent(&[EQ_FLAT, RAMP_FX]);
    let ra = offline(&a);
    let (mix, s) = swap_at(&a, &b);
    let end = 2 * (s + 2000) as usize;
    let bits = |x: &[f32]| x.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert!(left(&ra, s) > 0.05, "the ramp runs");
    assert_eq!(bits(&mix[..end]), bits(&ra[..end]));
}

/// An engine playing `a` with `b` queued one bar and 1000 frames in,
/// stopped at the swap sample `S`; returns it and `S`.
fn engine_at_swap(a: &Arc<CompiledSong>, b: &Arc<CompiledSong>) -> (Engine, u64) {
    let queued = BAR + 1000;
    let s = next_step_start(a, queued);
    let mut e = Engine::new(a.clone());
    let mut out = vec![0.0f32; 2 * 1000];
    run(&mut e, &mut out, queued);
    assert!(e.queue_swap(SongUpdate::new(b.clone())).is_none());
    run(&mut e, &mut out, s - queued);
    assert_eq!(e.position(), s);
    assert_eq!(e.swaps_applied(), 0);
    (e, s)
}

fn run(e: &mut Engine, out: &mut [f32], frames: u64) {
    let mut left = frames;
    while left > 0 {
        let n = left.min((out.len() / 2) as u64);
        e.process(&mut out[..2 * n as usize]);
        left -= n;
    }
}

fn frame(e: &mut Engine) {
    e.process(&mut [0.0; 2]);
}

#[test]
fn the_old_runtime_returns_after_the_fade() {
    let (mut e, _) = engine_at_swap(&t(&[]), &t(&[FLIP_FX]));
    for n in 0..FADE - 1 {
        frame(&mut e);
        assert_eq!(e.swaps_applied(), 1);
        assert!(e.swap_fading(), "fading after frame S + {n}");
        assert!(e.has_retired_swap());
        assert!(e.take_retired_swap().is_none(), "held after frame S + {n}");
    }
    // Frame S + 479 is the fade's last.
    frame(&mut e);
    assert!(!e.swap_fading());
    assert!(e.take_retired_swap().is_some());
}

#[test]
fn a_value_swap_does_not_fade() {
    let (mut e, _) = engine_at_swap(
        &t(&[FLIP_FX]),
        &song(&[track("t", "cos", &[FLIP_FX], "gain_db = -3")]),
    );
    frame(&mut e);
    assert_eq!(e.swaps_applied(), 1);
    assert!(!e.swap_fading());
    assert!(e.take_retired_swap().is_some());
}

#[test]
fn a_swap_during_a_fade_waits_for_it() {
    let (a, b, c) = (t(&[]), t(&[FLIP_FX]), t(&[]));
    let (mut e, s) = engine_at_swap(&a, &b);
    let mut out = vec![0.0f32; 2 * 100];
    run(&mut e, &mut out, 100);
    assert!(e.swap_fading());
    assert!(e.queue_swap(SongUpdate::new(c)).is_none());
    // The first swap's garbage comes back once its fade is over.
    let mut taken = None;
    while e.position() < s + STEP {
        frame(&mut e);
        assert_eq!(e.swaps_applied(), 1, "the second swap waits");
        if taken.is_none() && e.take_retired_swap().is_some() {
            taken = Some(e.position());
        }
    }
    assert_eq!(taken, Some(s + FADE));
    // It lands at the first step boundary after that, and fades in turn.
    assert!(e.has_pending_swap());
    frame(&mut e);
    assert_eq!(e.swaps_applied(), 2);
    assert!(e.swap_fading());
    assert!(e.take_retired_swap().is_none());
}

#[test]
fn a_device_that_cannot_copy_switches_at_once() {
    let (a, b) = (t(&[NOCOPY_FX]), t(&[NOCOPY_FX, FLIP_FX]));
    let (mut e, _) = engine_at_swap(&a, &b);
    frame(&mut e);
    assert_eq!(e.swaps_applied(), 1);
    assert!(!e.swap_fading());
    assert!(e.take_retired_swap().is_some());

    // Live, the switch is the instant one: `b` from the swap on.
    let rb = offline(&b);
    let (mix, s) = swap_at(&a, &b);
    let from = 2 * s as usize..mix.len();
    let bits = |x: &[f32]| x.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&mix[from.clone()]), bits(&rb[from]));
}

#[test]
fn a_seek_ends_the_fade() {
    let (mut e, _) = engine_at_swap(&t(&[]), &t(&[FLIP_FX]));
    frame(&mut e);
    assert!(e.swap_fading());
    e.seek(0);
    assert!(!e.swap_fading());
    assert!(e.take_retired_swap().is_some());
}
