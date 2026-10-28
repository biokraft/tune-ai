//! A module registered from outside the built-in list plays through the
//! whole path, parse to render, with no edits anywhere else.

use std::sync::Arc;

use tune_dsp::{Curve, ParamSpec};
use tune_engine::{CompiledSong, RenderOptions, render};
use tune_modules::{ClipKind, Effect, EventCtx, Instrument, ModuleDescriptor, NoteEvent, Role};
use tune_song::{Registry, parse_with};

#[test]
fn a_test_module_plays_without_edits_elsewhere() {
    const BURST_SECONDS: f64 = 0.05;
    const RATE: f64 = 48_000.0;

    static SINE_PARAMS: &[ParamSpec] = &[ParamSpec {
        id: "sine.freq",
        label: "Frequency",
        unit: "Hz",
        min: 20.0,
        max: 2000.0,
        curve: Curve::Exponential,
        default: 440.0,
        help: "Pitch of the burst.",
    }];
    static GAIN_PARAMS: &[ParamSpec] = &[ParamSpec {
        id: "gain.amount",
        label: "Amount",
        unit: "",
        min: 0.0,
        max: 1.0,
        curve: Curve::Linear,
        default: 1.0,
        help: "Linear gain.",
    }];

    /// A hit starts a 50 ms sine burst.
    struct Sine {
        sample_rate: f64,
        freq: f64,
        /// Samples since the last hit; `None` before the first.
        since_hit: Option<u64>,
    }
    impl Instrument for Sine {
        fn params(&self) -> &'static [ParamSpec] {
            SINE_PARAMS
        }
        fn set_param_at(&mut self, _index: usize, plain: f64) {
            self.freq = plain;
        }
        fn note(&mut self, event: &NoteEvent, _ctx: &EventCtx) {
            if matches!(event, NoteEvent::On { .. }) {
                self.since_hit = Some(0);
            }
        }
        fn process(&mut self) -> f64 {
            let Some(n) = self.since_hit else {
                return 0.0;
            };
            self.since_hit = Some(n + 1);
            let t = n as f64 / self.sample_rate;
            if t < BURST_SECONDS {
                (std::f64::consts::TAU * self.freq * t).sin()
            } else {
                0.0
            }
        }
        fn reset(&mut self) {
            self.since_hit = None;
        }
    }

    struct Gain(f64);
    impl Effect for Gain {
        fn params(&self) -> &'static [ParamSpec] {
            GAIN_PARAMS
        }
        fn set_param_at(&mut self, _index: usize, plain: f64) {
            self.0 = plain;
        }
        fn process(&mut self, x: f64) -> f64 {
            x * self.0
        }
        fn reset(&mut self) {}
    }

    static SINE: ModuleDescriptor = ModuleDescriptor {
        name: "sine",
        prefix: "sine",
        params: SINE_PARAMS,
        settings: &[],
        role: Role::Instrument {
            plays: &[ClipKind::Drum],
            default_duck_source: false,
            make: |sample_rate, _| {
                Box::new(Sine {
                    sample_rate,
                    freq: 440.0,
                    since_hit: None,
                })
            },
        },
    };
    static GAIN: ModuleDescriptor = ModuleDescriptor {
        name: "gain",
        prefix: "gain",
        params: GAIN_PARAMS,
        settings: &[],
        role: Role::Effect {
            make: |_, _| Box::new(Gain(1.0)),
        },
    };

    let registry = Registry::builtin().with(&SINE).with(&GAIN);
    let song_text = |amount: f64| {
        format!(
            r#"
[song]
version = 2
name = "sine"
bpm = 120

[tracks.tone]
instrument = "sine"
fx = [{{ type = "gain", params = {{ "gain.amount" = {amount} }} }}]
play = [{{ at = 1, clip = "x", bars = 1 }}]

[clips.x]
kind = "drum"
steps = "x..."

[[sections]]
name = "a"
bars = 1
"#
        )
    };
    let stem = |amount: f64| {
        let parsed = parse_with(&song_text(amount), &registry)
            .unwrap_or_else(|e| panic!("the sine song parses: {e:?}"));
        let compiled = Arc::new(CompiledSong::new(&parsed.song, RATE as u32));
        let rendered = render(
            compiled,
            &RenderOptions {
                start_bar: 0,
                end_bar: 1,
                stems: true,
            },
        )
        .expect("one bar renders");
        let (id, samples) = rendered.stems.into_iter().next().expect("one stem");
        assert_eq!(id, "tone");
        samples
    };

    let half = stem(0.5);
    // 120 BPM: a step is 6000 frames and `x...` hits every fourth step.
    const BURST: usize = (BURST_SECONDS * RATE) as usize;
    assert_eq!(half.len(), 2 * 96_000);
    for (frame, lr) in half.chunks(2).enumerate() {
        let since_hit = frame % 24_000;
        let loud = lr.iter().any(|&x| x != 0.0);
        if since_hit >= BURST {
            assert!(!loud, "frame {frame} should be silent");
        } else if since_hit != 0 {
            // The burst starts at sin(0) = 0 and is never exactly 0 again.
            assert!(loud, "frame {frame} should sound");
        }
    }
    let peak = |x: &[f32]| x.iter().fold(0.0f64, |m, &v| m.max(f64::from(v).abs()));
    let full = stem(1.0);
    assert!(peak(&full) > 0.1);
    assert!((peak(&half) - peak(&full) / 2.0).abs() <= 1e-12);
}
