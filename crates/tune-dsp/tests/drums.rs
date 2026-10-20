use tune_dsp::{ACCENT_GAIN_DB, Clap, KICK_PARAMS, Kick, ParamError};

const SR: f64 = 48_000.0;

fn render(k: &mut Kick, n: usize) -> Vec<f64> {
    (0..n).map(|_| k.get_sample()).collect()
}

fn ms(m: f64) -> usize {
    (m * SR / 1000.0).round() as usize
}

fn crossings(x: &[f64]) -> usize {
    x.windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count()
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

fn kick_no_click(tune: f64, punch: f64) -> Kick {
    let mut k = Kick::new(SR);
    k.set_param("kick.click", 0.0).unwrap();
    k.set_param("kick.tune", tune).unwrap();
    k.set_param("kick.punch", punch).unwrap();
    k
}

#[test]
fn kick_settles_on_tune() {
    for tune in [50.0, 80.0] {
        let mut k = kick_no_click(tune, 50.0);
        k.trigger(false, 1);
        let x = render(&mut k, ms(250.0));
        let seg = &x[ms(150.0)..];
        let hz = crossings(seg) as f64 / 2.0 / (seg.len() as f64 / SR);
        assert!((hz - tune).abs() <= 2.0, "tune {tune} measured {hz}");
    }
}

#[test]
fn kick_punch_sweeps_pitch() {
    let count = |punch: f64| {
        let mut k = kick_no_click(50.0, punch);
        k.trigger(false, 1);
        let x = render(&mut k, ms(10.0) + 1);
        crossings(&x[1..])
    };
    let (hi, lo) = (count(50.0), count(0.0));
    assert!(hi >= 3, "punch 50 crossings {hi}");
    assert!(lo <= 1, "punch 0 crossings {lo}");
}

#[test]
fn kick_decay_reaches_minus_60_db() {
    let mut k = kick_no_click(50.0, 50.0);
    k.set_param("kick.decay", 450.0).unwrap();
    k.set_param("kick.drive", 0.0).unwrap();
    k.trigger(false, 1);
    let x = render(&mut k, ms(470.0));
    let tail = x[ms(450.0)..].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let head = x[..ms(50.0)].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    assert!(tail <= 0.002, "tail {tail}");
    assert!(head >= 0.5, "head {head}");
}

#[test]
fn kick_bounded() {
    for extreme in [true, false] {
        let mut k = Kick::new(SR);
        for (i, s) in KICK_PARAMS.iter().enumerate() {
            k.set_param_at(i, if extreme { s.max } else { s.min });
        }
        let mut peak = 0.0f64;
        for n in 0..20 {
            k.trigger(true, n);
            for v in render(&mut k, ms(50.0)) {
                assert!(v.is_finite());
                peak = peak.max(v.abs());
            }
        }
        assert!(peak < 2.1, "extreme {extreme} peak {peak}");
    }
}

#[test]
fn kick_accent_louder_and_brighter() {
    let run = |accent: bool| {
        let mut k = Kick::new(SR);
        k.trigger(accent, 5);
        render(&mut k, ms(200.0))
    };
    let (n, a) = (run(false), run(true));
    let db = 20.0 * (rms(&a) / rms(&n)).log10();
    assert!((db - ACCENT_GAIN_DB).abs() <= 1.0, "accent {db} dB");
    let bright = |x: &[f64]| {
        let x = &x[..ms(50.0)];
        let d: f64 = x.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        d / x.iter().map(|v| v.abs()).sum::<f64>()
    };
    assert!(bright(&a) > bright(&n), "{} vs {}", bright(&a), bright(&n));
}

#[test]
fn kick_reset_is_silent() {
    let mut k = Kick::new(SR);
    k.trigger(false, 1);
    render(&mut k, 100);
    k.reset();
    assert!(render(&mut k, 1000).iter().all(|&v| v == 0.0));
}

#[test]
fn kick_params_validate() {
    let mut k = Kick::new(SR);
    assert!(matches!(
        k.set_param("kick.nope", 1.0),
        Err(ParamError::UnknownId(_))
    ));
    assert!(matches!(
        k.set_param("kick.tune", 100.0),
        Err(ParamError::OutOfRange { .. })
    ));
}

#[test]
fn kick_seeded() {
    let run = |seed: u64| {
        let mut k = Kick::new(SR);
        k.trigger(false, seed);
        render(&mut k, ms(100.0))
    };
    let a = run(3);
    assert!(
        a.iter()
            .zip(run(3))
            .all(|(x, y)| x.to_bits() == y.to_bits())
    );
    assert_ne!(a, run(4));
}

// ---- clap ----

fn clap_render(accent: bool, seed: u64, n: usize) -> Vec<f64> {
    let mut c = Clap::new(SR);
    c.trigger(accent, seed);
    (0..n).map(|_| c.get_sample()).collect()
}

#[test]
fn clap_has_three_slaps() {
    let x = clap_render(false, 1, ms(100.0));
    let s = ms(10.0);
    for b in 1..=2 {
        let after = rms(&x[b * s..b * s + ms(2.0)]);
        let before = rms(&x[b * s - ms(2.0)..b * s]);
        assert!(after > 2.0 * before, "slap {b}: {after} vs {before}");
    }
}

#[test]
fn clap_tail_follows_decay() {
    let x = clap_render(false, 1, ms(300.0));
    let late = rms(&x[ms(270.0)..ms(290.0)]);
    let early = rms(&x[..ms(30.0)]);
    assert!(late < 0.01 * early, "late {late} early {early}");
}
