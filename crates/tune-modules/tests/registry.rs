use tune_modules::{
    ACID303, BUILTIN, CLAP, Cell, ClipKind, DISTORTION, EQ, HAT_CLOSED, HAT_OPEN, KICK, RIDE,
    Registry, StepCtx,
};

#[test]
fn builtin_lists_the_eight_modules_in_order() {
    let registry = Registry::builtin();
    let instruments: Vec<_> = registry.instruments().map(|m| m.name).collect();
    assert_eq!(
        instruments,
        ["acid303", "kick", "clap", "hat_closed", "hat_open", "ride"]
    );
    let effects: Vec<_> = registry.effects().map(|m| m.name).collect();
    assert_eq!(effects, ["distortion", "eq"]);
    assert_eq!(BUILTIN.len(), 8);
}

#[test]
fn default_ids_are_names_and_prefixes() {
    assert_eq!(ACID303.prefix, "acid303");
    assert_eq!(KICK.prefix, "kick");
    assert_eq!(DISTORTION.prefix, "dist");
    assert_eq!(EQ.prefix, "eq");
}

#[test]
fn params_by_local_name() {
    assert_eq!(
        ACID303.param("cutoff").map(|(i, s)| (i, s.id)),
        Some((2, "acid303.cutoff"))
    );
    assert_eq!(DISTORTION.param("drive").unwrap().1.id, "dist.drive");
    assert!(EQ.param("dist.drive").is_none());
    assert_eq!(ACID303.local_name(2), "cutoff");
}

#[test]
fn every_builtin_param_id_carries_its_prefix() {
    for m in BUILTIN {
        for spec in m.params {
            assert!(
                spec.id.starts_with(&format!("{}.", m.prefix)),
                "{} in {m:?}",
                spec.id
            );
        }
    }
}

#[test]
fn clip_kinds_and_duck_default() {
    assert!(ACID303.plays(ClipKind::Acid) && !ACID303.plays(ClipKind::Drum));
    for m in [&KICK, &CLAP, &HAT_CLOSED, &HAT_OPEN, &RIDE] {
        assert!(m.plays(ClipKind::Drum) && !m.plays(ClipKind::Acid), "{m:?}");
    }
    assert!(!DISTORTION.plays(ClipKind::Drum));
    let duck: Vec<_> = BUILTIN
        .iter()
        .filter(|m| m.default_duck_source())
        .map(|m| m.name)
        .collect();
    assert_eq!(duck, ["kick"]);
}

#[test]
fn distortion_has_a_mode_setting() {
    let (index, s) = DISTORTION.setting("mode").unwrap();
    assert_eq!(index, 0);
    assert_eq!(s.values, ["soft", "hard", "overdrive"]);
    assert_eq!(s.default, "soft");
    assert_eq!(DISTORTION.default_settings(), ["soft"]);
    assert!(EQ.default_settings().is_empty());
}

#[test]
fn factories_build_their_voices() {
    let mut kick = KICK.make_instrument(48_000.0, &[]).unwrap();
    let ctx = StepCtx {
        step: 0,
        next_has_gate: false,
        gate_samples: 3000,
        seed: 1,
    };
    kick.on_step(&Cell::Hit { accent: false }, &ctx);
    assert!((0..100).any(|_| kick.process() != 0.0));

    assert!(DISTORTION.make_effect(48_000.0, &["hard"]).is_some());
    assert!(DISTORTION.make_instrument(48_000.0, &["hard"]).is_none());

    let mut eq = EQ.make_effect(48_000.0, &[]).unwrap();
    assert_eq!(eq.process(0.25).to_bits(), 0.25_f64.to_bits());
}

#[test]
#[should_panic(expected = "already registered")]
fn with_rejects_a_duplicate_name() {
    let _ = Registry::builtin().with(&KICK);
}
