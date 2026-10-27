//! Instruments driven by note events: the acid303 follows today's gate bit
//! for bit, ignores an Off for a note it is not sounding, and the drums hit
//! on an On and ignore every Off.

use tune_dsp::acid303::math::round_to_int;
use tune_dsp::acid303::{Acid303, AcidGate, AcidStep};
use tune_modules::{
    ACID303, CLAP, DRUM_PITCH, EventCtx, HAT_CLOSED, HAT_OPEN, Instrument, KICK, NoteEvent, NoteId,
    RIDE,
};

const RATE: f64 = 48_000.0;

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let z = (*state ^ (*state >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 16 steps of rests, notes, accents, slides and ties (a slide into the
/// same pitch).
fn random_pattern(rng: &mut u64) -> Vec<AcidStep> {
    let mut steps: Vec<AcidStep> = Vec::with_capacity(16);
    for k in 0..16 {
        let r = splitmix64(rng);
        let note = match r % 8 {
            0 | 1 => None,
            // A tie: the previous step's pitch again.
            2 if k > 0 && steps[k - 1].note.is_some() => steps[k - 1].note,
            _ => Some(24 + ((r >> 8) % 37) as u8),
        };
        steps.push(AcidStep {
            note,
            accent: (r >> 16).is_multiple_of(3),
            slide: (r >> 24) % 5 < 2,
        });
    }
    steps
}

fn step_start(k: u64, samples_per_step: f64) -> u64 {
    (k as f64 * samples_per_step).round() as u64
}

/// Today's acid track: per sample the gate ticks, a step start plays its
/// step, then the voice gives its sample.
fn gate_reference(pattern: &[AcidStep], bpm: f64, len: u64) -> Vec<f64> {
    let sps = RATE * 15.0 / bpm;
    let gate_samples = round_to_int(0.5 * sps);
    let mut voice = Acid303::with_param_defaults(RATE);
    let mut gate = AcidGate::new();
    let mut k = 0;
    (0..len)
        .map(|pos| {
            gate.tick(&mut voice);
            if pos == step_start(k, sps) {
                let n = pattern.len() as u64;
                let step = &pattern[(k % n) as usize];
                let next = &pattern[((k + 1) % n) as usize];
                gate.on_step(&mut voice, step, next.note.is_some(), gate_samples);
                k += 1;
            }
            voice.get_sample()
        })
        .collect()
}

/// The engine's step events for an acid pattern, delivered to `inst`.
fn step_events(inst: &mut dyn Instrument, pattern: &[AcidStep], bpm: f64, len: u64) -> Vec<f64> {
    let sps = RATE * 15.0 / bpm;
    let gate_samples = u64::try_from(round_to_int(0.5 * sps)).unwrap();
    let n = pattern.len() as u64;
    let mut held: Option<(NoteId, u8)> = None;
    let mut off: Option<(u64, NoteId, u8)> = None;
    let mut k: u64 = 0;
    let mut out = Vec::with_capacity(len as usize);
    for pos in 0..len {
        let ctx = |step| EventCtx {
            pos,
            step,
            seed: step,
        };
        if let Some((at, id, pitch)) = off
            && pos >= at
        {
            inst.note(&NoteEvent::Off { id, pitch }, &ctx(k.saturating_sub(1)));
            off = None;
        }
        if pos == step_start(k, sps) {
            let step = &pattern[(k % n) as usize];
            let next = &pattern[((k + 1) % n) as usize];
            if let Some(p) = step.note {
                let id = NoteId(k as u32);
                let into = step.slide && next.note.is_some();
                let velocity = if step.accent { 127 } else { 100 };
                let on = NoteEvent::On {
                    id,
                    pitch: p,
                    velocity,
                    accent: step.accent,
                    slide: into,
                };
                inst.note(&on, &ctx(k));
                if let Some((h, hp)) = held.take() {
                    inst.note(&NoteEvent::Off { id: h, pitch: hp }, &ctx(k));
                }
                if into {
                    held = Some((id, p));
                } else {
                    off = Some((pos + gate_samples, id, p));
                }
            } else if let Some((h, hp)) = held.take() {
                inst.note(&NoteEvent::Off { id: h, pitch: hp }, &ctx(k));
            }
            k += 1;
        }
        out.push(inst.process());
    }
    out
}

#[test]
fn acid_track_follows_the_gate_bitwise() {
    let mut rng = 1;
    for p in 0..100 {
        let pattern = random_pattern(&mut rng);
        for bpm in [120.0, 145.0] {
            let len = step_start(64, RATE * 15.0 / bpm);
            let want = gate_reference(&pattern, bpm, len);
            let mut inst = ACID303.make_instrument(RATE, &[]).unwrap();
            let got = step_events(inst.as_mut(), &pattern, bpm, len);
            let first = want
                .iter()
                .zip(&got)
                .position(|(a, b)| a.to_bits() != b.to_bits());
            assert_eq!(first, None, "pattern {p} at {bpm} BPM: {pattern:?}");
        }
    }
}

/// Delivers each `(pos, event)` just before `process` of sample `pos`.
fn drive(inst: &mut dyn Instrument, events: &[(u64, NoteEvent)], len: u64) -> Vec<f64> {
    (0..len)
        .map(|pos| {
            for (_, e) in events.iter().filter(|(at, _)| *at == pos) {
                inst.note(
                    e,
                    &EventCtx {
                        pos,
                        step: 0,
                        seed: 5,
                    },
                );
            }
            inst.process()
        })
        .collect()
}

fn on(id: u32) -> NoteEvent {
    NoteEvent::On {
        id: NoteId(id),
        pitch: 36,
        velocity: 100,
        accent: false,
        slide: false,
    }
}

fn off(id: u32) -> NoteEvent {
    NoteEvent::Off {
        id: NoteId(id),
        pitch: 36,
    }
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

fn acid(events: &[(u64, NoteEvent)]) -> Vec<f64> {
    let mut inst = ACID303.make_instrument(RATE, &[]).unwrap();
    drive(inst.as_mut(), events, 9000)
}

#[test]
fn off_for_another_note_is_ignored() {
    let other = acid(&[(0, on(1)), (3000, off(2))]);
    assert!(rms(&other[6000..9000]) > 0.1 * rms(&other[0..3000]));
    let own = acid(&[(0, on(1)), (3000, off(1))]);
    assert!(rms(&own[6000..9000]) < 0.01 * rms(&own[0..3000]));
}

#[test]
fn all_off_releases() {
    let all = acid(&[(0, on(1)), (3000, NoteEvent::AllOff)]);
    let own = acid(&[(0, on(1)), (3000, off(1))]);
    assert_eq!(all, own);
    assert!(rms(&all[6000..9000]) < 0.01 * rms(&all[0..3000]));
}

#[test]
fn drums_hit_on_note_on_and_ignore_offs() {
    let hit = NoteEvent::On {
        id: NoteId(0),
        pitch: DRUM_PITCH,
        velocity: 100,
        accent: false,
        slide: false,
    };
    let off = NoteEvent::Off {
        id: NoteId(0),
        pitch: DRUM_PITCH,
    };
    for m in [&KICK, &CLAP, &HAT_CLOSED, &HAT_OPEN, &RIDE] {
        let mut a = m.make_instrument(RATE, &[]).unwrap();
        let only_on = drive(a.as_mut(), &[(0, hit)], 9600);
        let mut b = m.make_instrument(RATE, &[]).unwrap();
        let with_offs = drive(
            b.as_mut(),
            &[(0, hit), (100, off), (200, NoteEvent::AllOff)],
            9600,
        );
        assert!(only_on.iter().any(|&v| v != 0.0), "{} hits", m.name);
        let bits = |x: &[f64]| x.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&only_on), bits(&with_offs), "{}", m.name);
    }
}
