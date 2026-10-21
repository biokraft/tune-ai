use tune_dsp::{
    ACCENT_GAIN_DB, CLAP_PARAMS, Clap, HAT_CLOSED_PARAMS, HAT_OPEN_PARAMS, Hat, HatKind,
    KICK_PARAMS, Kick, ParamError, ParamSpec,
};

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

// ---- clap and hats ----

fn clap_render(accent: bool, seed: u64, n: usize) -> Vec<f64> {
    let mut c = Clap::new(SR);
    c.trigger(accent, seed);
    (0..n).map(|_| c.get_sample()).collect()
}

fn hat_render(kind: HatKind, accent: bool, seed: u64, n: usize) -> Vec<f64> {
    let mut h = Hat::new(kind, SR);
    h.trigger(accent, seed);
    (0..n).map(|_| h.get_sample()).collect()
}

/// Mean absolute first difference over mean absolute level of the first 50 ms.
fn brightness(x: &[f64]) -> f64 {
    let x = &x[..ms(50.0)];
    let d: f64 = x.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    d / x.iter().map(|v| v.abs()).sum::<f64>()
}

fn peak(x: &[f64]) -> f64 {
    x.iter().fold(0.0f64, |m, v| m.max(v.abs()))
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

#[test]
fn open_hat_rings_longer() {
    let energy = |kind| {
        let x = hat_render(kind, false, 1, ms(1000.0));
        x[ms(150.0)..].iter().map(|v| v * v).sum::<f64>()
    };
    let (open, closed) = (energy(HatKind::Open), energy(HatKind::Closed));
    assert!(open > 10.0 * closed, "open {open} closed {closed}");
}

#[test]
fn brightness_order() {
    let hat = brightness(&hat_render(HatKind::Closed, false, 1, ms(60.0)));
    let clap = brightness(&clap_render(false, 1, ms(60.0)));
    let mut k = Kick::new(SR);
    k.trigger(false, 1);
    let kick = brightness(&render(&mut k, ms(60.0)));
    assert!(
        hat > clap && clap > kick,
        "hat {hat} clap {clap} kick {kick}"
    );
}

#[test]
fn drum_levels() {
    let p = [
        ("clap", peak(&clap_render(false, 1, ms(500.0)))),
        (
            "closed",
            peak(&hat_render(HatKind::Closed, false, 1, ms(500.0))),
        ),
        (
            "open",
            peak(&hat_render(HatKind::Open, false, 1, ms(1500.0))),
        ),
    ];
    for (name, v) in p {
        assert!((0.2..=1.0).contains(&v), "{name} peak {v}");
    }
}

/// One voice under test: label, sample renderer, param table, setter.
type Voice = (&'static str, fn(bool, u64, usize) -> Vec<f64>);

fn voices() -> [Voice; 3] {
    [
        ("clap", clap_render),
        ("closed", |a, s, n| hat_render(HatKind::Closed, a, s, n)),
        ("open", |a, s, n| hat_render(HatKind::Open, a, s, n)),
    ]
}

#[test]
fn clap_hats_accent_louder_and_brighter() {
    for (name, run) in voices() {
        let (n, a) = (run(false, 5, ms(200.0)), run(true, 5, ms(200.0)));
        let db = 20.0 * (rms(&a) / rms(&n)).log10();
        assert!((db - ACCENT_GAIN_DB).abs() <= 1.5, "{name} accent {db} dB");
        assert!(brightness(&a) > brightness(&n), "{name} brightness");
    }
}

#[test]
fn clap_hats_seeded() {
    for (name, run) in voices() {
        let a = run(false, 3, ms(100.0));
        let b = run(false, 3, ms(100.0));
        assert!(
            a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
            "{name}"
        );
        assert_ne!(a, run(false, 4, ms(100.0)), "{name}");
    }
}

#[test]
fn clap_hats_reset_is_silent() {
    let mut c = Clap::new(SR);
    c.trigger(false, 1);
    (0..100).for_each(|_| {
        c.get_sample();
    });
    c.reset();
    assert!((0..1000).all(|_| c.get_sample() == 0.0));
    for kind in [HatKind::Closed, HatKind::Open] {
        let mut h = Hat::new(kind, SR);
        h.trigger(false, 1);
        (0..100).for_each(|_| {
            h.get_sample();
        });
        h.reset();
        assert!((0..1000).all(|_| h.get_sample() == 0.0));
    }
}

#[test]
fn clap_hats_params_validate() {
    let mut c = Clap::new(SR);
    assert!(matches!(
        c.set_param("clap.nope", 1.0),
        Err(ParamError::UnknownId(_))
    ));
    assert!(matches!(
        c.set_param("clap.tone", 5000.0),
        Err(ParamError::OutOfRange { .. })
    ));
    assert!(c.set_param("clap.tone", 1000.0).is_ok());
    let mut h = Hat::new(HatKind::Closed, SR);
    assert!(matches!(
        h.set_param("hat_open.tone", 5000.0),
        Err(ParamError::UnknownId(_))
    ));
    assert!(matches!(
        h.set_param("hat_closed.metal", 101.0),
        Err(ParamError::OutOfRange { .. })
    ));
    assert!(h.set_param("hat_closed.metal", 10.0).is_ok());
    assert_eq!(h.params().len(), HAT_CLOSED_PARAMS.len());
    assert_eq!(
        Hat::new(HatKind::Open, SR).params().len(),
        HAT_OPEN_PARAMS.len()
    );
}

#[test]
fn clap_hats_bounded_at_extremes() {
    let run_set = |specs: &[ParamSpec],
                   mut set: Box<dyn FnMut(usize, f64)>,
                   mut hit: Box<dyn FnMut(u64) -> Vec<f64>>| {
        for extreme in [true, false] {
            for (i, s) in specs.iter().enumerate() {
                set(i, if extreme { s.max } else { s.min });
            }
            let mut p = 0.0f64;
            for n in 0..20 {
                for v in hit(n) {
                    assert!(v.is_finite());
                    p = p.max(v.abs());
                }
            }
            assert!(p < 4.0, "extreme {extreme} peak {p}");
        }
    };
    use std::{cell::RefCell, rc::Rc};
    let c = Rc::new(RefCell::new(Clap::new(SR)));
    let (c1, c2) = (c.clone(), c.clone());
    run_set(
        CLAP_PARAMS,
        Box::new(move |i, v| c1.borrow_mut().set_param_at(i, v)),
        Box::new(move |n| {
            let mut c = c2.borrow_mut();
            c.trigger(true, n);
            (0..ms(50.0)).map(|_| c.get_sample()).collect()
        }),
    );
    for kind in [HatKind::Closed, HatKind::Open] {
        let h = Rc::new(RefCell::new(Hat::new(kind, SR)));
        let (h1, h2) = (h.clone(), h.clone());
        let specs = h.borrow().params();
        run_set(
            specs,
            Box::new(move |i, v| h1.borrow_mut().set_param_at(i, v)),
            Box::new(move |n| {
                let mut h = h2.borrow_mut();
                h.trigger(true, n);
                (0..ms(50.0)).map(|_| h.get_sample()).collect()
            }),
        );
    }
}

#[test]
fn clap_hats_tone_changes_mid_hit() {
    let at = ms(20.0);
    let n = ms(60.0);
    // Renders n samples, setting `tone` after `at` samples when `to` is Some.
    fn go(
        n: usize,
        at: usize,
        to: Option<f64>,
        mut set: impl FnMut(f64),
        mut next: impl FnMut() -> f64,
    ) -> Vec<f64> {
        (0..n)
            .map(|i| {
                if i == at
                    && let Some(t) = to
                {
                    set(t);
                }
                next()
            })
            .collect()
    }
    let check = |name: &str, a: Vec<f64>, b: Vec<f64>| {
        assert!(
            a[..at]
                .iter()
                .zip(&b[..at])
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "{name} changed before the tone change"
        );
        assert!(a[at..] != b[at..], "{name} ignored the tone change");
    };
    let clap = |to: Option<f64>| {
        let mut c = Clap::new(SR);
        c.trigger(false, 1);
        let cell = std::cell::RefCell::new(&mut c);
        go(
            n,
            at,
            to,
            |t| cell.borrow_mut().set_param("clap.tone", t).unwrap(),
            || cell.borrow_mut().get_sample(),
        )
    };
    check("clap", clap(None), clap(Some(2000.0)));
    for (kind, id) in [
        (HatKind::Closed, "hat_closed.tone"),
        (HatKind::Open, "hat_open.tone"),
    ] {
        let hat = |to: Option<f64>| {
            let mut h = Hat::new(kind, SR);
            h.trigger(false, 1);
            let cell = std::cell::RefCell::new(&mut h);
            go(
                n,
                at,
                to,
                |t| cell.borrow_mut().set_param(id, t).unwrap(),
                || cell.borrow_mut().get_sample(),
            )
        };
        check(id, hat(None), hat(Some(11000.0)));
    }
}

fn fnv1a(x: &[f64]) -> u64 {
    x.iter()
        .flat_map(|v| v.to_bits().to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

/// Pins the closed and open hats bit for bit, so adding the ride cannot move them.
#[test]
fn hats_unchanged_by_ride() {
    let hash = |kind, accent| fnv1a(&hat_render(kind, accent, 1, ms(1000.0)));
    assert_eq!(
        hash(HatKind::Closed, false),
        10007451823363822274,
        "closed plain"
    );
    assert_eq!(
        hash(HatKind::Closed, true),
        15593455684581083118,
        "closed accent"
    );
    assert_eq!(
        hash(HatKind::Open, false),
        15616646834503362329,
        "open plain"
    );
    assert_eq!(
        hash(HatKind::Open, true),
        11074481127065425725,
        "open accent"
    );
}
