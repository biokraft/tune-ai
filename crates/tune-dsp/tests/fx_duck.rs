//! Behaviour of the grid-locked kick ducker and the strip params.

use tune_dsp::fx::{
    Ducker, SEND_OFF_DB, STRIP_DUCK_DEPTH, STRIP_DUCK_RELEASE, STRIP_PARAMS, STRIP_SEND_DELAY,
    STRIP_SEND_REVERB, send_gain,
};
use tune_dsp::{Curve, db2amp, find_spec};

const SR: f64 = 48000.0;

fn make(depth: f64, release: f64) -> Ducker {
    let mut d = Ducker::new(SR);
    d.set_depth_db(depth);
    d.set_release_ms(release);
    d
}

fn run(d: &mut Ducker, n: usize) -> Vec<f64> {
    (0..n).map(|_| d.next_gain()).collect()
}

#[test]
fn idle_gain_is_exactly_one() {
    let mut d = make(6.0, 100.0);
    assert!(run(&mut d, 10_000).iter().all(|&g| g == 1.0));
}

#[test]
fn attack_ramps_linearly() {
    let k = db2amp(-6.0);
    let mut d = make(6.0, 100.0);
    d.trigger();
    let g = run(&mut d, 100);
    assert!((g[48] - (1.0 - (1.0 - k) * 0.5)).abs() < 1e-12);
    assert!((g[96] - k).abs() < 1e-12);
}

#[test]
fn release_is_a_raised_cosine() {
    let k = db2amp(-6.0);
    let mut d = make(6.0, 100.0);
    d.trigger();
    let g = run(&mut d, 96 + 4800 + 10);
    assert!((g[96 + 2400] - (1.0 - (1.0 - k) * 0.5)).abs() < 1e-12);
    assert_eq!(g[96 + 4800], 1.0);
    assert_eq!(g[96 + 4805], 1.0);
}

#[test]
fn retrigger_is_continuous() {
    let mut d = make(6.0, 100.0);
    d.trigger();
    let mut g = run(&mut d, 2400);
    d.trigger();
    g.extend(run(&mut d, 8000));
    for w in g.windows(2) {
        assert!((w[1] - w[0]).abs() <= 0.05, "step {} -> {}", w[0], w[1]);
    }
}

#[test]
fn depth_zero_never_dips() {
    let mut d = make(0.0, 100.0);
    assert!(!d.is_active());
    d.trigger();
    assert!(run(&mut d, 6000).iter().all(|&g| g == 1.0));
}

#[test]
fn send_gain_off_at_floor() {
    assert_eq!(send_gain(-60.0), 0.0);
    assert_eq!(send_gain(-80.0), 0.0);
    assert_eq!(send_gain(-6.0), db2amp(-6.0));
    assert_eq!(send_gain(0.0), 1.0);
    assert_eq!(SEND_OFF_DB, -60.0);
}

#[test]
fn reset_returns_to_idle() {
    let mut d = make(6.0, 100.0);
    d.trigger();
    run(&mut d, 50);
    d.reset();
    assert_eq!(d.next_gain(), 1.0);
}

#[test]
fn strip_params_are_listed() {
    let ids: Vec<_> = STRIP_PARAMS.iter().map(|p| p.id).collect();
    assert_eq!(
        ids,
        ["send.delay", "send.reverb", "duck.depth", "duck.release"]
    );
    assert!(find_spec(STRIP_PARAMS, "duck.release").is_ok());
}

#[test]
fn envelope_shape_at_quarter_points() {
    let k = db2amp(-6.0);
    let dip = |e: f64| 1.0 - (1.0 - k) * e;
    let mut d = make(6.0, 100.0);
    d.trigger();
    let g = run(&mut d, 96 + 4800 + 1);
    assert_eq!(g[0], 1.0);
    assert!((g[24] - dip(0.25)).abs() < 1e-12);
    assert!((g[72] - dip(0.75)).abs() < 1e-12);
    let c = std::f64::consts::PI / 4.0;
    assert!((g[96 + 1200] - dip(0.5 * (1.0 + c.cos()))).abs() < 1e-12);
    assert!((g[96 + 3600] - dip(0.5 * (1.0 - c.cos()))).abs() < 1e-12);
}

#[test]
fn retrigger_during_attack_continues_from_current_value() {
    let k = db2amp(-6.0);
    let mut d = make(6.0, 100.0);
    d.trigger();
    run(&mut d, 48);
    d.trigger();
    let g = run(&mut d, 100);
    // e0 = 0.5 at frame 48; 48 frames into the new attack: 0.5 + 0.5 * 0.5.
    assert!((g[0] - (1.0 - (1.0 - k) * 0.5)).abs() < 1e-12);
    assert!((g[48] - (1.0 - (1.0 - k) * 0.75)).abs() < 1e-12);
    assert!((g[96] - k).abs() < 1e-12);
}

#[test]
fn strip_param_specs_are_pinned() {
    let want = [
        (
            "send.delay",
            "Delay",
            "dB",
            -60.0,
            0.0,
            Curve::Linear,
            -60.0,
        ),
        (
            "send.reverb",
            "Reverb",
            "dB",
            -60.0,
            0.0,
            Curve::Linear,
            -60.0,
        ),
        ("duck.depth", "Duck", "dB", 0.0, 24.0, Curve::Linear, 0.0),
        (
            "duck.release",
            "Duck release",
            "ms",
            20.0,
            400.0,
            Curve::Exponential,
            80.0,
        ),
    ];
    assert_eq!(STRIP_PARAMS.len(), want.len());
    for (p, w) in STRIP_PARAMS.iter().zip(want) {
        assert_eq!(p.id, w.0);
        assert_eq!(p.label, w.1);
        assert_eq!(p.unit, w.2);
        assert_eq!(p.min, w.3);
        assert_eq!(p.max, w.4);
        assert!(p.curve == w.5, "{}", p.id);
        assert_eq!(p.default, w.6);
    }
    assert_eq!(STRIP_PARAMS[STRIP_SEND_DELAY].id, "send.delay");
    assert_eq!(STRIP_PARAMS[STRIP_SEND_REVERB].id, "send.reverb");
    assert_eq!(STRIP_PARAMS[STRIP_DUCK_DEPTH].id, "duck.depth");
    assert_eq!(STRIP_PARAMS[STRIP_DUCK_RELEASE].id, "duck.release");
    assert_eq!(
        (
            STRIP_SEND_DELAY,
            STRIP_SEND_REVERB,
            STRIP_DUCK_DEPTH,
            STRIP_DUCK_RELEASE
        ),
        (0, 1, 2, 3)
    );
}
