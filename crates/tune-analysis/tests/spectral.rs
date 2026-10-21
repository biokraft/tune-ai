mod common;

use common::{concat, mono, noise, sine, stereo};
use tune_analysis::{BANDS, measure_spectral};

const SR: u32 = 48_000;

fn band(s: &tune_analysis::Spectral, i: usize) -> f64 {
    s.bands[i].expect("band present")
}

#[test]
fn sine_lands_in_its_band() {
    let s = measure_spectral(&mono(SR, sine(SR, 2000.0, -12.0, 4.0, 0.0)));
    assert!((band(&s, 3) - 0.0).abs() < 0.1, "mid {}", band(&s, 3));
    for (i, b) in s.bands.iter().enumerate().filter(|(i, _)| *i != 3) {
        assert!(b.unwrap() <= -40.0, "band {i} = {b:?}");
    }
    assert_eq!(BANDS[3].key, "mid");
    assert_eq!(BANDS[2].label, "low-mid");
}

#[test]
fn band_edges() {
    for (hz, idx) in [(40.0, 0), (100.0, 1), (500.0, 2), (6000.0, 4), (14000.0, 5)] {
        let s = measure_spectral(&mono(SR, sine(SR, hz, -12.0, 4.0, 0.0)));
        assert!(
            band(&s, idx).abs() < 0.2,
            "{hz} Hz -> band {idx} = {}",
            band(&s, idx)
        );
    }
}

#[test]
fn centroid_of_sine() {
    let s = measure_spectral(&mono(SR, sine(SR, 1000.0, -12.0, 5.0, 0.0)));
    let b = &s.brightness;
    assert!(
        (b.median_hz.unwrap() - 1000.0).abs() < 15.0,
        "{:?}",
        b.median_hz
    );
    assert!(b.range_oct.unwrap() < 0.05, "{:?}", b.range_oct);
}

#[test]
fn centroid_follows_a_sweep() {
    let a = mono(SR, sine(SR, 500.0, -12.0, 10.0, 0.0));
    let b = mono(SR, sine(SR, 4000.0, -12.0, 10.0, 0.0));
    let s = measure_spectral(&concat(&a, &b));
    let br = &s.brightness;
    assert_eq!(br.centroid_hz.len(), 20);
    assert!((br.p10_hz.unwrap() - 500.0).abs() < 25.0, "{:?}", br.p10_hz);
    assert!(
        (br.p90_hz.unwrap() - 4000.0).abs() < 100.0,
        "{:?}",
        br.p90_hz
    );
    assert!(
        (br.range_oct.unwrap() - 3.0).abs() < 0.1,
        "{:?}",
        br.range_oct
    );
    assert!((br.profile_hz[0].unwrap() / 500.0 - 1.0).abs() < 0.05);
    assert!((br.profile_hz[7].unwrap() / 4000.0 - 1.0).abs() < 0.05);
}

#[test]
fn same_signal_same_numbers_at_44k1_and_48k() {
    let make = |sr: u32| {
        let mut x = vec![0.0f32; (sr as usize) * 5];
        for hz in [40.0, 150.0, 500.0, 2000.0, 6000.0, 14000.0] {
            for (o, v) in x.iter_mut().zip(sine(sr, hz, -20.0, 5.0, 0.0)) {
                *o += v;
            }
        }
        measure_spectral(&mono(sr, x))
    };
    let (a, b) = (make(44_100), make(48_000));
    for i in 0..6 {
        assert!(
            (band(&a, i) - band(&b, i)).abs() < 0.2,
            "band {i}: {} vs {}",
            band(&a, i),
            band(&b, i)
        );
    }
    let (ma, mb) = (
        a.brightness.median_hz.unwrap(),
        b.brightness.median_hz.unwrap(),
    );
    assert!((ma / mb - 1.0).abs() < 0.01, "{ma} vs {mb}");
}

#[test]
fn low_end_ratio() {
    let mut x = sine(SR, 50.0, -12.0, 4.0, 0.0);
    for (o, v) in x.iter_mut().zip(sine(SR, 150.0, -12.0, 4.0, 0.0)) {
        *o += v;
    }
    let s = measure_spectral(&mono(SR, x));
    assert!(s.low_end.ratio_db.unwrap().abs() < 0.3, "{:?}", s.low_end);
}

#[test]
fn stereo_cases() {
    let x = sine(SR, 440.0, -12.0, 2.0, 0.0);
    let same = measure_spectral(&stereo(SR, x.clone(), x.clone()));
    assert!((same.stereo.correlation.unwrap() - 1.0).abs() < 1e-6);
    assert_eq!(same.stereo.side_to_mid_db, -200.0);

    let neg: Vec<f32> = x.iter().map(|v| -v).collect();
    let anti = measure_spectral(&stereo(SR, x.clone(), neg));
    assert!((anti.stereo.correlation.unwrap() + 1.0).abs() < 1e-6);

    let ind = measure_spectral(&stereo(
        SR,
        noise(SR, -12.0, 5.0, 1),
        noise(SR, -12.0, 5.0, 2),
    ));
    assert!(ind.stereo.correlation.unwrap().abs() < 0.05);

    let m = measure_spectral(&mono(SR, x));
    assert_eq!(m.stereo.correlation, Some(1.0));
}

#[test]
fn low_end_mono_check() {
    let lo = sine(SR, 50.0, -12.0, 4.0, 0.0);
    let hi = sine(SR, 3000.0, -12.0, 4.0, 0.0);
    let l: Vec<f32> = lo.iter().zip(&hi).map(|(a, b)| a + b).collect();
    let r: Vec<f32> = lo.iter().zip(&hi).map(|(a, b)| a - b).collect();
    let s = measure_spectral(&stereo(SR, l, r));
    assert!(s.stereo.low_correlation.unwrap() > 0.99, "{:?}", s.stereo);
    assert!(s.stereo.correlation.unwrap().abs() < 0.05, "{:?}", s.stereo);
}

#[test]
fn silence_has_no_balance() {
    let s = measure_spectral(&mono(SR, vec![0.0; SR as usize * 3]));
    assert!(s.bands.iter().all(Option::is_none));
    assert_eq!(s.brightness.median_hz, None);
    assert_eq!(s.brightness.p10_hz, None);
    assert_eq!(s.brightness.range_oct, None);
    assert!(s.brightness.centroid_hz.iter().all(Option::is_none));
    assert_eq!(s.stereo.correlation, None);
    assert_eq!(s.low_end.ratio_db, None);
}
