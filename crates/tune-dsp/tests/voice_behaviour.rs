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
