use tune_dsp::ParamError;
use tune_dsp::acid303::AcidStep;
use tune_engine::{Cell, Clock, Instrument, StepCtx, hit_seed, track_seed};
use tune_modules::{ACID303, KICK, Registry};

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

fn note(slide_or_tie: bool) -> Cell {
    Cell::Note(AcidStep {
        note: Some(36),
        accent: false,
        slide: slide_or_tie,
    })
}

/// Drives `cells` at 120 BPM / 48 kHz, one per step, for `len` samples.
fn drive(inst: &mut dyn Instrument, cells: &[Cell], len: usize) -> Vec<f64> {
    let clock = Clock::new(120.0, 48_000);
    let mut out = Vec::with_capacity(len);
    for pos in 0..len as u64 {
        let k = clock.step_at(pos);
        if clock.step_start(k) == pos {
            let cell = &cells[k as usize % cells.len()];
            let next = &cells[(k as usize + 1) % cells.len()];
            let ctx = StepCtx {
                step: k,
                next_has_gate: matches!(next, Cell::Note(s) if s.note.is_some()),
                gate_samples: clock.gate_samples(),
                seed: hit_seed(track_seed(1, "t"), k),
            };
            inst.on_step(cell, &ctx);
        }
        out.push(inst.process());
    }
    out
}

#[test]
fn seeds_pinned() {
    assert_eq!(track_seed(7, "kick"), 0x33d8839d0e2386ea);
    assert_eq!(hit_seed(0x33d8839d0e2386ea, 16), 0x08653e8277ee190e);
    assert_ne!(track_seed(7, "kick"), track_seed(7, "clap"));
    assert_ne!(track_seed(7, "kick"), track_seed(8, "kick"));
    assert_ne!(hit_seed(5, 1), hit_seed(5, 2));
}

#[test]
fn acid_track_gate_closes_half_step() {
    let mut t = ACID303.make_instrument(48_000.0, &[]).unwrap();
    let out = drive(t.as_mut(), &[note(false), Cell::Rest], 12_000);
    assert!(rms(&out[3600..6000]) < 0.01 * rms(&out[0..3000]));
}

#[test]
fn acid_track_tie_holds_gate() {
    let mut t = ACID303.make_instrument(48_000.0, &[]).unwrap();
    // `C2~ C2`: the first step slides into a note, so the gate stays open.
    let out = drive(t.as_mut(), &[note(true), note(false)], 12_000);
    assert!(rms(&out[3600..6000]) > 0.3 * rms(&out[0..3000]));
}

#[test]
fn drum_track_hits_only() {
    let mut k = KICK.make_instrument(48_000.0, &[]).unwrap();
    let out = drive(k.as_mut(), &[Cell::Rest], 6000);
    assert!(out.iter().all(|&v| v == 0.0));
    let mut k = KICK.make_instrument(48_000.0, &[]).unwrap();
    let out = drive(k.as_mut(), &[Cell::Hit { accent: false }], 6000);
    assert!(out.iter().any(|&v| v != 0.0));
}

#[test]
fn set_param_by_id() {
    let mut k = KICK.make_instrument(48_000.0, &[]).unwrap();
    assert!(k.set_param("kick.tune", 60.0).is_ok());
    assert!(matches!(
        k.set_param("acid303.cutoff", 1000.0),
        Err(ParamError::UnknownId(_))
    ));
    assert!(matches!(
        k.set_param("kick.tune", 1e9),
        Err(ParamError::OutOfRange { .. })
    ));
}

#[test]
fn params_tables() {
    for m in Registry::builtin().instruments() {
        let a: Vec<_> = m
            .make_instrument(48_000.0, &m.default_settings())
            .unwrap()
            .params()
            .iter()
            .map(|p| p.id)
            .collect();
        let b: Vec<_> = m.params.iter().map(|p| p.id).collect();
        assert_eq!(a, b, "{m:?}");
    }
}
