//! The mixer: track strips (sends, ducking), the delay and reverb buses and
//! the master stage, on variants of the shared `mixed.toml` fixture.

mod common;

use std::sync::Arc;

use common::{MINI, MIXED, bits, compile_song, compiled, drop_lane, run, song, variant_of};
use tune_analysis::{Audio, measure_loudness};
use tune_dsp::acid303::ACID303_PARAMS;
use tune_dsp::{MASTER_CEILING, MASTER_GAIN, STRIP_SEND_DELAY, db2amp};
use tune_engine::{
    CLIP_KNEE, CompiledSong, Engine, MIX_TRACK, ParamTarget, RenderOptions, Rendered, pan_gains,
    render,
};
use tune_song::{MasterMode, MixUnit, Song};

/// `songs/first-acid` as first mixed (limit mode, `master.gain` 8.5,
/// `master.ceiling` -1.5, release 70 ms), frozen so the limiter test keeps
/// the exact material that once read 0.46 dB over the ceiling.
const FIRST_ACID_MIXED: &str = include_str!("../../tune-song/tests/fixtures/first-acid-mixed.toml");

/// Frames of the duck attack at 48 kHz (2 ms).
const ATTACK: u64 = 96;
/// Frames of the default duck release at 48 kHz (80 ms).
const RELEASE: u64 = 3840;

/// `mixed.toml` changed by `edit`, checked by a write and re-parse.
fn variant(edit: impl FnOnce(&mut Song)) -> Song {
    variant_of(MIXED, edit)
}

/// The clip variant: `mixed.toml` with `master = "clip"`, then `edit`.
fn clip_variant(edit: impl FnOnce(&mut Song)) -> Song {
    variant(|s| {
        s.mix.master = MasterMode::Clip;
        edit(s);
    })
}

fn set_strip(s: &mut Song, track: &str, id: &str, plain: f64) {
    s.tracks
        .get_mut(track)
        .unwrap()
        .strip
        .insert(id.into(), plain);
}

fn remove_strip(s: &mut Song, track: &str, id: &str) {
    s.tracks.get_mut(track).unwrap().strip.shift_remove(id);
}

/// The whole song with stems.
fn render_all(s: &Song) -> (Arc<CompiledSong>, Rendered) {
    let c = compile_song(s);
    let opts = RenderOptions {
        start_bar: 0,
        end_bar: c.total_bars,
        stems: true,
    };
    let r = render(c.clone(), &opts).unwrap();
    (c, r)
}

fn stem<'a>(r: &'a Rendered, id: &str) -> &'a [f32] {
    &r.stems.iter().find(|(t, _)| t == id).unwrap().1
}

/// Frame `f` of interleaved stereo `x`.
fn at(x: &[f32], f: u64) -> (f32, f32) {
    (x[2 * f as usize], x[2 * f as usize + 1])
}

/// First frame where the two interleaved signals differ in either channel.
fn first_difference(a: &[f32], b: &[f32]) -> Option<u64> {
    assert_eq!(a.len(), b.len());
    a.chunks_exact(2)
        .zip(b.chunks_exact(2))
        .position(|(x, y)| bits(x) != bits(y))
        .map(|f| f as u64)
}

#[test]
fn compiles_mix_and_strip() {
    let s = compiled(MIXED);
    assert_eq!(s.mix.master, MasterMode::Limit);
    assert_eq!(s.mix.duck_source, Some(0));
    let delay = s.mix.delay.as_ref().expect("delay bus");
    assert_eq!(delay[1], 40.0);
    assert!(s.mix.reverb.is_some());
    assert_eq!(s.mix.master_params[MASTER_GAIN], 6.0);

    let acid = &s.tracks[1];
    assert_eq!(acid.strip, vec![-12.0, -18.0, 6.0, 80.0]);
    assert_eq!(acid.instrument.params.len(), ACID303_PARAMS.len());
    assert_eq!(s.tracks[2].strip, vec![-60.0, -20.0, 0.0, 80.0]);

    let b: Vec<(usize, ParamTarget)> = s.sections[1]
        .lanes
        .iter()
        .map(|l| (l.track, l.target))
        .collect();
    assert!(
        b.contains(&(1, ParamTarget::Strip(STRIP_SEND_DELAY))),
        "{b:?}"
    );
    let reverb_size = ParamTarget::Mix {
        unit: MixUnit::Reverb,
        index: 0,
    };
    assert!(b.contains(&(MIX_TRACK, reverb_size)), "{b:?}");

    let (track, target, spec) = s.resolve_param("mix", "master.gain").unwrap();
    assert_eq!(
        (track, target),
        (
            MIX_TRACK,
            ParamTarget::Mix {
                unit: MixUnit::Master,
                index: 0
            }
        )
    );
    assert_eq!(spec.id, "master.gain");
    let (track, target, _) = s.resolve_param("acid", "send.delay").unwrap();
    assert_eq!((track, target), (1, ParamTarget::Strip(STRIP_SEND_DELAY)));
    let (track, target, _) = s.resolve_param("acid", "eq.low_cut").unwrap();
    assert_eq!((track, target), (1, ParamTarget::Fx { slot: 1, index: 0 }));
    assert!(s.resolve_param("mix", "send.delay").is_none());
    assert!(s.resolve_param("nope", "send.delay").is_none());
}

#[test]
fn buses_inactive_without_sends() {
    let mini = compiled(MINI);
    assert_eq!(mini.mix.master, MasterMode::Clip);
    assert!(mini.mix.delay.is_none());
    assert!(mini.mix.reverb.is_none());

    let src = MINI.replacen(
        "[tracks.kick]",
        "[mix]\nparams = { \"delay.feedback\" = 50 }\n\n[tracks.kick]",
        1,
    );
    let s = compiled(&src);
    assert!(s.mix.delay.is_none());
    assert!(s.mix.reverb.is_none());
}

#[test]
fn lane_only_send_activates_bus() {
    let lane_only = clip_variant(|s| remove_strip(s, "acid", "send.delay"));
    let no_lane = clip_variant(|s| {
        remove_strip(s, "acid", "send.delay");
        drop_lane(s, "acid", "send.delay");
    });
    let (c, with_lane) = render_all(&lane_only);
    assert!(c.mix.delay.is_some());
    let (c, without) = render_all(&no_lane);
    assert!(c.mix.delay.is_none());
    assert_ne!(bits(&with_lane.mix), bits(&without.mix));
}

#[test]
fn send_reaches_delay_bus() {
    // Only the acid is heard; the delay returns exactly what was sent,
    // three steps (18000 frames) later.
    let base = |s: &mut Song| {
        drop_lane(s, "acid", "send.delay");
        for t in ["kick", "hat", "ride"] {
            s.tracks.get_mut(t).unwrap().mute = true;
        }
    };
    let off = clip_variant(|s| {
        base(s);
        remove_strip(s, "acid", "send.delay");
    });
    let on = clip_variant(|s| {
        base(s);
        set_strip(s, "acid", "send.delay", 0.0);
        for (id, v) in [
            ("delay.feedback", 0.0),
            ("delay.low_cut", 20.0),
            ("delay.high_cut", 20_000.0),
        ] {
            s.mix.params.insert(id.into(), v);
        }
    });
    // Render from bar 2, where section b (and the acid) starts: before it
    // the idle 303 voice leaks a residue near 1e-39, which the delay would
    // echo too. Frame `f` here is song frame `bar_start(2) + f`.
    let from_b = |s: &Song| {
        let opts = RenderOptions {
            start_bar: 2,
            end_bar: 4,
            stems: false,
        };
        render(compile_song(s), &opts).unwrap().mix
    };
    let (off, on) = (from_b(&off), from_b(&on));
    let echo = 18_000 - 1;
    let first = first_difference(&off, &on).expect("the mixes differ");
    assert!(first >= echo, "first difference at {first}, echo at {echo}");
    assert!(
        first < echo + 6000,
        "first difference at {first}, echo at {echo}"
    );
}

#[test]
fn ducking_follows_kick_hits() {
    let depth = |db: f64| clip_variant(move |s| set_strip(s, "acid", "duck.depth", db));
    let (c, deep) = render_all(&depth(12.0));
    let (_, flat) = render_all(&depth(0.0));
    let (deep, flat) = (stem(&deep, "acid"), stem(&flat, "acid"));
    let ratio = db2amp(-12.0);
    let mut checked = 0;
    // Section b (bars 2-3) plays the acid; the kick hits every 4 steps.
    for step in (32..64).step_by(4) {
        let h = c.clock.step_start(step);
        let (dl, dr) = at(deep, h + ATTACK);
        let (fl, fr) = at(flat, h + ATTACK);
        for (d, f) in [(dl, fl), (dr, fr)] {
            if f.abs() > 1e-3 {
                let r = f64::from(d) / f64::from(f);
                assert!((r - ratio).abs() < 1e-5, "step {step}: ratio {r}");
                checked += 1;
            }
        }
        // From the end of the release to the next kick (over the three
        // off-beat steps without a hit) the acid is not ducked at all.
        let back = h + ATTACK + RELEASE;
        let next = c.clock.step_start(step + 4);
        let range = 2 * back as usize..2 * next as usize;
        assert_eq!(
            first_difference(&deep[range.clone()], &flat[range]),
            None,
            "step {step}: ducked between kicks (frame offset from {back})"
        );
    }
    assert!(checked > 0, "no loud acid frame at a kick");
    assert_ne!(bits(deep), bits(flat));
}

#[test]
fn muted_source_still_ducks() {
    let depth12 = |s: &mut Song| set_strip(s, "acid", "duck.depth", 12.0);
    let (_, heard) = render_all(&clip_variant(depth12));
    let (_, muted) = render_all(&clip_variant(|s| {
        depth12(s);
        s.tracks.get_mut("kick").unwrap().mute = true;
    }));
    assert_eq!(bits(stem(&heard, "acid")), bits(stem(&muted, "acid")));
}

#[test]
fn no_source_hits_no_ducking() {
    // Section b stops playing the kick: no hits there, so no dip in it.
    // (Section a's kicks still duck the idle 303's ~1e-39 residue, and the
    // last release ends long before bar 2.)
    let depth = |db: f64| {
        clip_variant(move |s| {
            set_strip(s, "acid", "duck.depth", db);
            s.sections[1].play.shift_remove("kick");
        })
    };
    let (c, deep) = render_all(&depth(12.0));
    let (_, flat) = render_all(&depth(0.0));
    let b = 2 * c.clock.bar_start(2) as usize;
    let (deep, flat) = (&stem(&deep, "acid")[b..], &stem(&flat, "acid")[b..]);
    assert!(deep.iter().any(|v| v.abs() > 1e-2), "the acid plays in b");
    assert_eq!(first_difference(deep, flat), None, "ducked without a hit");
}

#[test]
fn delay_send_is_post_fader() {
    // Only the acid is heard, at -6 dB, through a clean delay (no
    // feedback, no filters) with the send at 0 dB. The master runs at
    // -12 dB so the clipper stays linear: the mix with the send minus the
    // mix without it is the delay's return, which must be the acid after
    // its fader (`y·gain`), three steps late, in the left channel.
    let base = |s: &mut Song| {
        drop_lane(s, "acid", "send.delay");
        for t in ["kick", "hat", "ride"] {
            s.tracks.get_mut(t).unwrap().mute = true;
        }
        s.tracks.get_mut("acid").unwrap().gain_db = -6.0;
        s.mix.params.insert("master.gain".into(), -12.0);
    };
    let off = clip_variant(|s| {
        base(s);
        remove_strip(s, "acid", "send.delay");
    });
    let on = clip_variant(|s| {
        base(s);
        set_strip(s, "acid", "send.delay", 0.0);
        for (id, v) in [
            ("delay.feedback", 0.0),
            ("delay.low_cut", 20.0),
            ("delay.high_cut", 20_000.0),
        ] {
            s.mix.params.insert(id.into(), v);
        }
    });
    let from_b = |s: &Song| {
        let opts = RenderOptions {
            start_bar: 2,
            end_bar: 4,
            stems: true,
        };
        render(compile_song(s), &opts).unwrap()
    };
    let (off, on) = (from_b(&off), from_b(&on));
    assert!(on.meter.pre_clip_peak <= CLIP_KNEE, "{:?}", on.meter);
    let master = db2amp(-12.0);
    let pan_left = pan_gains(0.0).0;
    let dry = stem(&on, "acid");
    let mut checked = 0;
    for f in 18_000..on.mix.len() as u64 / 2 {
        let ret = f64::from(at(&on.mix, f).0) - f64::from(at(&off.mix, f).0);
        // The stem is `y·gain·pan`; the send taps `y·gain`.
        let sent = f64::from(at(dry, f - 18_000).0) / pan_left;
        let want = sent * master;
        assert!(
            (ret - want).abs() < 1e-6,
            "frame {f}: return {ret}, want {want}"
        );
        if want.abs() > 1e-2 {
            checked += 1;
        }
    }
    assert!(checked > 1000, "only {checked} loud echo frames");
}

#[test]
fn mute_stops_sends() {
    let send = |db: f64| {
        clip_variant(move |s| {
            drop_lane(s, "acid", "send.delay");
            set_strip(s, "acid", "send.delay", db);
            s.tracks.get_mut("acid").unwrap().mute = true;
        })
    };
    let (_, loud) = render_all(&send(0.0));
    let (_, off) = render_all(&send(-60.0));
    assert_eq!(bits(&loud.mix), bits(&off.mix));
}

#[test]
fn solo_keeps_returns() {
    let send = |db: f64| {
        clip_variant(move |s| {
            drop_lane(s, "acid", "send.delay");
            set_strip(s, "acid", "send.delay", db);
            s.tracks.get_mut("acid").unwrap().solo = true;
        })
    };
    let (_, loud) = render_all(&send(0.0));
    let (_, off) = render_all(&send(-60.0));
    assert_ne!(bits(&loud.mix), bits(&off.mix));
}

#[test]
fn master_gain_drives_clipper() {
    let gain = |db: f64| {
        clip_variant(move |s| {
            s.mix.params.insert("master.gain".into(), db);
        })
    };
    let (_, hot) = render_all(&gain(6.0));
    let (_, unity) = render_all(&gain(0.0));
    let ratio = hot.meter.pre_clip_peak / unity.meter.pre_clip_peak;
    assert!((ratio - db2amp(6.0)).abs() < 1e-9, "ratio {ratio}");
}

#[test]
fn limit_mode_meets_ceiling() {
    let (_, r) = render_all(&song(MIXED));
    let ceiling = db2amp(-1.5);
    assert!(r.meter.peak <= ceiling, "{:?}", r.meter);
    assert!(r.meter.max_reduction_db > 0.0, "{:?}", r.meter);
    assert!(r.meter.knee_samples > 0, "{:?}", r.meter);
    assert!(r.meter.pre_clip_peak > r.meter.peak, "{:?}", r.meter);
    let audio = Audio::from_interleaved(r.sample_rate, 2, &r.mix).unwrap();
    let tp = measure_loudness(&audio).unwrap().0.true_peak_dbtp;
    assert!(tp <= -1.2, "true peak {tp} dBTP");
}

/// Real material, judged by the meter `tune analyze` reports (BS.1770 true
/// peak): at most 0.3 dB over the ceiling, the margin the limiter promises.
#[test]
fn limiter_meets_ceiling_on_first_acid() {
    let song = compiled(FIRST_ACID_MIXED);
    let ceiling = song.mix.master_params[MASTER_CEILING];
    assert_eq!(ceiling, -1.5);
    // Bars 1-96, intro to the end of the drop: the over sat in the drop
    // (bar 85) and depends on the state built up since bar 1.
    let opts = RenderOptions {
        start_bar: 0,
        end_bar: 96,
        stems: false,
    };
    let r = render(song, &opts).unwrap();
    assert!(r.meter.max_reduction_db > 6.0, "{:?}", r.meter);
    let audio = Audio::from_interleaved(r.sample_rate, 2, &r.mix).unwrap();
    let tp = measure_loudness(&audio).unwrap().0.true_peak_dbtp;
    eprintln!("first-acid bars 1-96: {tp:.3} dBTP at ceiling {ceiling} dB");
    assert!(
        tp <= ceiling + 0.3,
        "true peak {tp} dBTP, ceiling {ceiling} dB"
    );
}

#[test]
fn eq_in_chain_runs() {
    let (_, with) = render_all(&song(MIXED));
    let (_, without) = render_all(&variant(|s| {
        s.tracks.get_mut("acid").unwrap().fx.truncate(1);
    }));
    assert_ne!(bits(stem(&with, "acid")), bits(stem(&without, "acid")));
}

#[test]
fn ride_track_plays() {
    let (c, r) = render_all(&song(MIXED));
    let a_end = c.clock.bar_start(2) as usize;
    assert!(stem(&r, "ride")[..2 * a_end].iter().any(|&x| x != 0.0));
}

#[test]
fn overrides_reach_strip_and_mix() {
    // Live-only: a knob on the master gain and on the acid's duck depth.
    let s = compile_song(&clip_variant(|_| {}));
    let frames = s.end_sample();
    let plain = run(&mut Engine::new(s.clone()), frames);

    let mut e = Engine::new(s.clone());
    let gain = ParamTarget::Mix {
        unit: MixUnit::Master,
        index: MASTER_GAIN,
    };
    e.set_override(MIX_TRACK, gain, Some(-12.0));
    assert_eq!(e.overrides_active(), 1);
    let quiet = run(&mut e, frames);
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(
        peak(&quiet) < peak(&plain) * 0.3,
        "master gain override ignored"
    );
    e.clear_overrides();
    assert_eq!(e.overrides_active(), 0);

    let mut e = Engine::new(s.clone());
    let (track, depth, _) = s.resolve_param("acid", "duck.depth").unwrap();
    e.set_override(track, depth, Some(24.0));
    assert_eq!(e.overrides_active(), 1);
    assert_ne!(bits(&run(&mut e, frames)), bits(&plain));
    e.set_override(track, depth, None);
    assert_eq!(e.overrides_active(), 0);
}

#[test]
fn mixed_renders_alike_at_any_block_size() {
    let s = compiled(MIXED);
    let offline = render(
        s.clone(),
        &RenderOptions {
            start_bar: 0,
            end_bar: s.total_bars,
            stems: false,
        },
    )
    .unwrap()
    .mix;
    let mut e = Engine::new(s.clone());
    let mut live = vec![0.0f32; offline.len()];
    for block in live.chunks_mut(2 * 333) {
        e.process(block);
    }
    assert_eq!(bits(&offline), bits(&live));
}
