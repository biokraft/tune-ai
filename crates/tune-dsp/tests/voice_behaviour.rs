//! Musical behaviour of the Acid303 voice: slides, accents, extreme
//! settings, tuning across sample rates and parameter validation.

use tune_dsp::ParamError;
use tune_dsp::acid303::Acid303;

const FS: f64 = 48_000.0;

/// Samples per 1/16 note at `bpm`.
fn sixteenth(fs: f64, bpm: f64) -> f64 {
    fs * 60.0 / bpm / 4.0
}

fn render(v: &mut Acid303, n: usize) -> Vec<f64> {
    (0..n).map(|_| v.get_sample()).collect()
}

/// Brightness proxy: mean absolute first difference.
fn mean_abs_diff(x: &[f64]) -> f64 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (x.len() - 1) as f64
}

fn peak(x: &[f64]) -> f64 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// A voice with a strong, short filter envelope, so a retrigger is audible.
/// (At the defaults, env mod 25 % and decay 1000 ms, the envelope has barely
/// decayed after 6000 samples and a retrigger brightens by only ~9 %.)
fn plucky() -> Acid303 {
    let mut v = Acid303::new(FS);
    v.set_waveform(0.0);
    v.set_env_mod(80.0);
    v.set_decay(200.0);
    v.set_resonance(50.0);
    v
}

/// Brightness ratio (500 samples after / 500 samples before) when the second
/// note at sample 6000 is played with `second`.
fn brightness_jump(second: impl FnOnce(&mut Acid303)) -> f64 {
    let mut v = plucky();
    v.trigger_note(36, false);
    let before = render(&mut v, 6_000);
    second(&mut v);
    let after = render(&mut v, 500);
    mean_abs_diff(&after) / mean_abs_diff(&before[5_500..])
}

#[test]
fn slide_does_not_retrigger_the_filter_envelope() {
    let slide = brightness_jump(|v| v.slide_to_note(38, false));
    let retrigger = brightness_jump(|v| v.trigger_note(38, false));
    println!("brightness ratio: slide {slide:.4}, retrigger {retrigger:.4}");
    assert!(
        (slide - 1.0).abs() <= 0.2,
        "slide changed brightness by {:.1} %",
        100.0 * (slide - 1.0)
    );
    assert!(
        retrigger > 1.5,
        "retrigger only changed brightness by {:.1} %",
        100.0 * (retrigger - 1.0)
    );
}

#[test]
fn back_to_back_accents_stack() {
    let mut v = Acid303::new(FS);
    v.set_waveform(0.0);
    v.set_accent(100.0);
    let step = sixteenth(FS, 145.0);
    let mut peaks = Vec::new();
    for k in 0..3 {
        let start = (k as f64 * step).round() as usize;
        let end = ((k + 1) as f64 * step).round() as usize;
        let gate = ((k as f64 + 0.5) * step).round() as usize;
        v.trigger_note(36, true);
        let mut out = render(&mut v, gate - start);
        v.release_note();
        out.extend(render(&mut v, end - gate));
        peaks.push(peak(&out));
    }
    println!("accent step peaks: {peaks:?}");
    // The plan asked for strictly rising peaks, but the C++ Open303 does not
    // do that: run through `RefOpen303` (direct triggerNote and its own
    // AcidSequencer alike) the three peaks are 11.761, 12.723, 12.491. Every
    // accent after the first hits harder than the first (+8.2 %, +6.2 %); the
    // third dips 1.8 % below the second. The C++ behaviour wins, so the test
    // checks that the stacked accents beat the opening one.
    assert!(
        peaks[1] > peaks[0] && peaks[2] > peaks[0],
        "stacked accents do not hit harder than the first: {peaks:?}"
    );
}

/// Peak over 64 accented 1/16 steps at 145 BPM with resonance 100 %, cutoff
/// 2394 Hz, env mod 100 %, accent 100 %, decay 200 ms and the given volume.
fn extreme_peak(fs: f64, waveform: f64, volume: f64) -> f64 {
    let mut v = Acid303::new(fs);
    v.set_waveform(waveform);
    v.set_resonance(100.0);
    v.set_cutoff(2394.0);
    v.set_env_mod(100.0);
    v.set_accent(100.0);
    v.set_decay(200.0);
    v.set_volume(volume);
    let step = sixteenth(fs, 145.0);
    let notes = [24u8, 36, 48, 60, 31, 43, 55, 67];
    let mut worst = 0.0f64;
    for k in 0..64 {
        let start = (k as f64 * step).round() as usize;
        let end = ((k + 1) as f64 * step).round() as usize;
        let gate = ((k as f64 + 0.5) * step).round() as usize;
        v.trigger_note(notes[k % notes.len()], true);
        let mut out = render(&mut v, gate - start);
        v.release_note();
        out.extend(render(&mut v, end - gate));
        assert!(out.iter().all(|x| x.is_finite()), "non-finite at step {k}");
        worst = worst.max(peak(&out));
    }
    println!("extreme fs {fs} waveform {waveform} volume {volume} dB: peak {worst:.4}");
    worst
}

/// Review Focus 1, at the default volume (-12 dB, the `acid303.volume`
/// default): finite and |x| < 4.0.
#[test]
fn extreme_settings_stay_finite_and_bounded() {
    for fs in [44_100.0, 48_000.0, 96_000.0] {
        for waveform in [0.0, 1.0] {
            let worst = extreme_peak(fs, waveform, -12.0);
            assert!(worst < 4.0, "fs {fs} waveform {waveform}: peak {worst}");
        }
    }
}

/// Review Focus 1 at full volume (0 dB, which is also the gain of a voice
/// whose `set_volume` was never called: the C++ constructor sets
/// `level = -12` but `ampScaler = 1`). The < 4.0 bound does not hold for the
/// C++ here: through `RefOpen303` the saw peaks at 3.863 / 4.630 / 4.791 at
/// 44.1 / 48 / 96 kHz (square 2.540 / 3.034 / 3.019). The C++ behaviour
/// wins, so this checks finiteness and a 6.0 bound with ~25 % headroom over
/// the measured worst case.
#[test]
fn extreme_settings_at_full_volume_stay_finite() {
    for fs in [44_100.0, 48_000.0, 96_000.0] {
        for waveform in [0.0, 1.0] {
            let worst = extreme_peak(fs, waveform, 0.0);
            assert!(worst < 6.0, "fs {fs} waveform {waveform}: peak {worst}");
        }
    }
}

/// Fundamental of `x` by autocorrelation: coarse period search, then a
/// refined peak 20 periods out with parabolic interpolation.
fn fundamental(x: &[f64], fs: f64) -> f64 {
    let corr = |lag: usize| -> f64 {
        let n = x.len() - lag;
        let num: f64 = (0..n).map(|i| x[i] * x[i + lag]).sum();
        let e0: f64 = x[..n].iter().map(|s| s * s).sum();
        let e1: f64 = x[lag..].iter().map(|s| s * s).sum();
        num / (e0 * e1).sqrt()
    };
    let lo = (fs / 600.0) as usize;
    let hi = (fs / 300.0) as usize;
    let coarse = (lo..=hi)
        .max_by(|&a, &b| corr(a).total_cmp(&corr(b)))
        .unwrap();
    let k = 20;
    let centre = coarse * k;
    let best = (centre - 2 * k..=centre + 2 * k)
        .max_by(|&a, &b| corr(a).total_cmp(&corr(b)))
        .unwrap();
    let (ym, y0, yp) = (corr(best - 1), corr(best), corr(best + 1));
    let offset = 0.5 * (ym - yp) / (ym - 2.0 * y0 + yp);
    fs * k as f64 / (best as f64 + offset)
}

/// Review Focus 2: note 69 at tuning 440 plays 440 Hz at every sample rate.
#[test]
fn a4_plays_440_hz_at_every_sample_rate() {
    for fs in [44_100.0, 48_000.0, 96_000.0] {
        let mut v = Acid303::new(fs);
        v.set_waveform(0.0);
        v.set_resonance(0.0);
        v.set_env_mod(0.0);
        v.set_cutoff(2394.0);
        v.set_tuning(440.0);
        v.trigger_note(69, false);
        let out = render(&mut v, (0.6 * fs) as usize);
        let f = fundamental(&out[(0.1 * fs) as usize..], fs);
        println!("fs {fs}: fundamental {f:.4} Hz");
        assert!((f - 440.0).abs() <= 1.0, "fs {fs}: {f} Hz");
    }
}

#[test]
fn set_param_rejects_unknown_ids_and_out_of_range_values() {
    let mut v = Acid303::new(FS);
    match v.set_param("acid303.nope", 1.0) {
        Err(ParamError::UnknownId(id)) => assert_eq!(id, "acid303.nope"),
        other => panic!("expected UnknownId, got {other:?}"),
    }
    match v.set_param("acid303.cutoff", 5000.0) {
        Err(ParamError::OutOfRange {
            id,
            value,
            min,
            max,
        }) => {
            assert_eq!(id, "acid303.cutoff");
            assert_eq!((value, min, max), (5000.0, 314.0, 2394.0));
        }
        other => panic!("expected OutOfRange, got {other:?}"),
    }
    assert!(v.set_param("acid303.cutoff", f64::NAN).is_err());
    // A rejected value leaves the voice untouched.
    assert_eq!(v.cutoff(), 1000.0);
    // Range ends are inclusive.
    v.set_param("acid303.cutoff", 2394.0).unwrap();
    v.set_param("acid303.cutoff", 314.0).unwrap();
    assert_eq!(v.cutoff(), 314.0);
}

#[test]
fn idle_voice_is_silent_and_note_off_decays() {
    let mut v = Acid303::new(FS);
    v.set_waveform(0.0);
    assert!(render(&mut v, 1_000).iter().all(|&x| x == 0.0));
    v.note_on(48, 90);
    assert!(peak(&render(&mut v, 4_800)) > 1e-3);
    v.note_on(48, 0);
    let tail = render(&mut v, 96_000);
    assert!(tail[tail.len() - 4_800..].iter().all(|x| x.abs() < 1e-4));
}

#[test]
fn with_param_defaults_sets_every_documented_default() {
    use tune_dsp::acid303::ACID303_PARAMS;
    let v = Acid303::with_param_defaults(FS);
    let got = |id: &str| match id {
        "acid303.waveform" => v.waveform(),
        "acid303.tuning" => v.tuning(),
        "acid303.cutoff" => v.cutoff(),
        "acid303.resonance" => v.resonance(),
        "acid303.env_mod" => v.env_mod(),
        "acid303.decay" => v.decay(),
        "acid303.accent" => v.accent(),
        "acid303.volume" => v.volume(),
        "acid303.slide_time" => v.slide_time(),
        other => panic!("test lacks a getter for {other}"),
    };
    for p in ACID303_PARAMS {
        let g = got(p.id);
        assert!(
            (g - p.default).abs() <= 1e-9 * p.default.abs().max(1.0),
            "{}: got {g}, default {}",
            p.id,
            p.default
        );
    }
}

#[test]
fn with_param_defaults_differs_from_new_only_by_params() {
    // Same note through `new` + manual -12 dB volume (the only default that
    // changes the level) and through `with_param_defaults` at volume 0 dB.
    let note = |mut v: Acid303| {
        v.trigger_note(36, false);
        render(&mut v, 4_000)
    };
    let defaults = note(Acid303::with_param_defaults(FS));
    let mut quiet = Acid303::with_param_defaults(FS);
    quiet.set_volume(0.0);
    let full = note(quiet);
    let scale = tune_dsp::acid303::math::db2amp(-12.0);
    assert!(peak(&full) > 0.0);
    for (d, f) in defaults.iter().zip(&full) {
        assert!((d - f * scale).abs() <= 1e-12, "{d} vs {}", f * scale);
    }
}

/// Instantaneous pitch track: for each pair of successive rising zero
/// crossings (sub-sample interpolated), `(midpoint in samples, Hz)`.
fn pitch_track(x: &[f64], fs: f64) -> Vec<(f64, f64)> {
    let mut crossings = Vec::new();
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            crossings.push(i as f64 - 1.0 + (-x[i - 1]) / (x[i] - x[i - 1]));
        }
    }
    crossings
        .windows(2)
        .map(|w| ((w[0] + w[1]) / 2.0, fs / (w[1] - w[0])))
        .collect()
}

fn cents(f: f64, reference: f64) -> f64 {
    1200.0 * (f / reference).log2()
}

/// Pitch (Hz) at `at` samples after the slide starts: the estimate whose
/// midpoint is nearest.
fn pitch_at(track: &[(f64, f64)], at: f64) -> f64 {
    track
        .iter()
        .min_by(|a, b| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()))
        .unwrap()
        .1
}

/// Glide from C2 to C3 with the given slide time; returns the pitch track
/// with sample 0 = the `slide_to_note` call.
fn glide_track(slide_time_ms: f64, len: usize) -> Vec<(f64, f64)> {
    let mut v = Acid303::with_param_defaults(FS);
    v.set_param("acid303.resonance", 0.0).unwrap();
    v.set_param("acid303.env_mod", 0.0).unwrap();
    v.set_param("acid303.cutoff", 2394.0).unwrap();
    v.set_param("acid303.slide_time", slide_time_ms).unwrap();
    v.trigger_note(36, false);
    render(&mut v, 12_000);
    v.slide_to_note(48, false);
    let out = render(&mut v, len);
    pitch_track(&out, FS)
}

#[test]
fn slide_glides_exponentially_to_the_target_pitch() {
    const C2: f64 = 65.406_391_325_149_66;
    const C3: f64 = 130.812_782_650_299_3;
    // The pitch slew is a one-pole filter with time constant 0.2 * slide time
    // (60 ms default -> 12 ms), so five time constants take 60 ms.
    let tau = 0.2 * 60.0e-3 * FS;
    let track = glide_track(60.0, 9_600);
    let early = pitch_at(&track, 0.25 * tau);
    let late = pitch_at(&track, 5.0 * tau + 480.0);
    // Measured: early 93.98 Hz (about 570 cents below C3), late 130.48 Hz
    // (4.4 cents below C3); the pitch estimate lags the true pitch slightly.
    println!("early {early:.2} Hz, late {late:.2} Hz");
    assert!(
        cents(early, C2) >= -50.0 && cents(early, C3) < -400.0,
        "pitch should still be far below the target early in the glide: {early}"
    );
    assert!(
        cents(late, C3).abs() <= 25.0,
        "pitch after 5 time constants is {:.1} cents from C3",
        cents(late, C3)
    );
}

#[test]
fn longer_slide_time_glides_slower() {
    const C3: f64 = 130.812_782_650_299_3;
    let fast = glide_track(20.0, 9_600);
    let slow = glide_track(200.0, 9_600);
    // 30 ms after the slide starts.
    let at = 0.030 * FS;
    let (f, s) = (pitch_at(&fast, at), pitch_at(&slow, at));
    // Measured: 130.83 Hz for a 20 ms slide, 100.80 Hz for a 200 ms slide.
    println!("pitch 30 ms into the glide: slide 20 ms {f:.2} Hz, 200 ms {s:.2} Hz");
    assert!(
        cents(f, C3).abs() < 25.0,
        "short slide should have arrived: {f}"
    );
    assert!(
        cents(s, C3) < -300.0,
        "long slide should still be well below the target: {s}"
    );
}

fn notes_output(v: &mut Acid303) -> Vec<u64> {
    let mut out = Vec::new();
    for note in [45u8, 52, 57] {
        v.trigger_note(note, false);
        out.extend((0..(FS / 3.0) as usize).map(|_| v.get_sample().to_bits()));
    }
    out
}

#[test]
fn set_param_at_matches_set_param() {
    for (i, spec) in tune_dsp::acid303::ACID303_PARAMS.iter().enumerate() {
        let mut a = Acid303::new(FS);
        let mut b = Acid303::new(FS);
        a.set_param(spec.id, spec.max).unwrap();
        b.set_param_at(i, spec.max);
        assert_eq!(notes_output(&mut a), notes_output(&mut b), "{}", spec.id);
    }
}

#[test]
fn set_param_at_clamps() {
    let mut v = Acid303::new(FS);
    v.set_param_at(2, 1e9);
    assert_eq!(v.cutoff(), 2394.0);
}

/// Plays the fixed probe used by the reset tests: trigger, slide at 6000,
/// release at 12 000, 48 000 samples in all.
fn probe(v: &mut Acid303) -> Vec<f64> {
    v.trigger_note(36, true);
    let mut out = Vec::with_capacity(48_000);
    for i in 0..48_000 {
        if i == 6000 {
            v.slide_to_note(48, false);
        }
        if i == 12_000 {
            v.release_note();
        }
        out.push(v.get_sample());
    }
    out
}

fn configured() -> Acid303 {
    let mut v = Acid303::with_param_defaults(FS);
    v.set_resonance(85.0);
    v.set_env_mod(60.0);
    v.set_accent(100.0);
    v
}

#[test]
fn reset_matches_fresh_voice() {
    let mut a = configured();
    // 1 s of accented and sliding notes to dirty every piece of state:
    for i in 0..48_000 {
        match i {
            0 => a.trigger_note(36, true),
            8000 => a.slide_to_note(43, true),
            16_000 => a.release_note(),
            24_000 => a.trigger_note(31, false),
            32_000 => a.slide_to_note(55, false),
            _ => {}
        }
        a.get_sample();
    }
    a.reset();
    let mut b = configured();
    let (ra, rb) = (probe(&mut a), probe(&mut b));
    assert!(ra.iter().any(|x| x.abs() > 1e-3), "probe is silent");
    assert!(
        ra.iter().zip(&rb).all(|(x, y)| x.to_bits() == y.to_bits()),
        "reset voice differs from a fresh one"
    );
}

#[test]
fn reset_is_silent() {
    let mut a = configured();
    a.trigger_note(36, true);
    render(&mut a, 5000);
    a.reset();
    assert!(render(&mut a, 4800).iter().all(|&x| x == 0.0));
}
