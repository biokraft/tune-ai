mod common;

use common::{concat, mono, sine, stereo};
use tune_analysis::measure_loudness;

fn close(v: Option<f64>, want: f64, tol: f64) {
    let v = v.expect("value present");
    assert!((v - want).abs() <= tol, "{v} not within {tol} of {want}");
}

fn stereo_sine(sr: u32, hz: f64, dbfs: f64, secs: f64) -> tune_analysis::Audio {
    let x = sine(sr, hz, dbfs, secs, 0.0);
    stereo(sr, x.clone(), x)
}

#[test]
fn minus_23_stereo_reads_minus_23() {
    let (l, _) = measure_loudness(&stereo_sine(48000, 1000.0, -23.0, 20.0)).unwrap();
    close(l.integrated_lufs, -23.0, 0.1);
    close(l.short_term_max_lufs, -23.0, 0.1);
}

#[test]
fn mono_counts_as_dual_mono() {
    let a = mono(48000, sine(48000, 1000.0, -23.0, 20.0, 0.0));
    let (l, _) = measure_loudness(&a).unwrap();
    close(l.integrated_lufs, -23.0, 0.1);
    close(l.short_term_max_lufs, -23.0, 0.1);
}

#[test]
fn rate_44k1_reads_the_same() {
    let (l, _) = measure_loudness(&stereo_sine(44100, 1000.0, -23.0, 20.0)).unwrap();
    close(l.integrated_lufs, -23.0, 0.1);
}

#[test]
fn lra_ten_lu() {
    let a = concat(
        &stereo_sine(48000, 1000.0, -20.0, 20.0),
        &stereo_sine(48000, 1000.0, -30.0, 20.0),
    );
    let (l, _) = measure_loudness(&a).unwrap();
    close(l.lra_lu, 10.0, 1.0);
}

#[test]
fn true_peak_between_samples() {
    let x = sine(48000, 12000.0, 0.0, 5.0, std::f64::consts::FRAC_PI_4);
    let (l, _) = measure_loudness(&stereo(48000, x.clone(), x)).unwrap();
    assert!(
        (l.sample_peak_dbfs + 3.01).abs() <= 0.05,
        "{}",
        l.sample_peak_dbfs
    );
    assert!(
        (-0.4..=0.2).contains(&l.true_peak_dbtp),
        "{}",
        l.true_peak_dbtp
    );
}

#[test]
fn full_scale_sine_crest() {
    let x = sine(48000, 997.0, 0.0, 5.0, 0.0);
    let (l, _) = measure_loudness(&mono(48000, x)).unwrap();
    assert!((l.rms_dbfs + 3.01).abs() <= 0.05, "{}", l.rms_dbfs);
    close(l.crest_db, 3.01, 0.05);
}

#[test]
fn series_is_on_a_one_second_grid() {
    let (_, s) = measure_loudness(&stereo_sine(48000, 1000.0, -23.0, 20.0)).unwrap();
    assert_eq!(s.len(), 20);
    assert!(s[0].is_none() && s[1].is_none());
    for v in &s[2..] {
        close(*v, -23.0, 0.1);
    }
}

#[test]
fn short_passage_has_no_short_term() {
    let (l, s) = measure_loudness(&stereo_sine(48000, 1000.0, -23.0, 1.7)).unwrap();
    assert!(l.integrated_lufs.is_some());
    assert_eq!(l.short_term_max_lufs, None);
    assert_eq!(l.lra_lu, None);
    assert_eq!(s, vec![None]);
}

#[test]
fn silence_is_none() {
    let z = vec![0.0f32; 48000 * 5];
    let (l, _) = measure_loudness(&stereo(48000, z.clone(), z)).unwrap();
    assert_eq!(l.integrated_lufs, None);
    assert_eq!(l.short_term_max_lufs, None);
    assert_eq!(l.crest_db, None);
    assert_eq!(l.plr_db, None);
    assert_eq!(l.sample_peak_dbfs, -200.0);
    assert_eq!(l.true_peak_dbtp, -200.0);
}
