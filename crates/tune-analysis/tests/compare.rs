mod common;

use common::{concat, mono, sine};
use tune_analysis::{Audio, analyze, compare};

const SR: u32 = 48_000;

fn six_sine(extra_500_dbfs: Option<f64>) -> Audio {
    let mut x = vec![0.0f32; SR as usize * 5];
    for hz in [40.0, 150.0, 500.0, 2000.0, 6000.0, 14000.0] {
        for (o, v) in x.iter_mut().zip(sine(SR, hz, -20.0, 5.0, 0.0)) {
            *o += v;
        }
    }
    if let Some(db) = extra_500_dbfs {
        for (o, v) in x.iter_mut().zip(sine(SR, 500.0, db, 5.0, 0.0)) {
            *o += v;
        }
    }
    mono(SR, x)
}

fn scaled(a: &Audio, k: f32) -> Audio {
    Audio {
        sample_rate: a.sample_rate,
        channels: a
            .channels
            .iter()
            .map(|c| c.iter().map(|v| v * k).collect())
            .collect(),
    }
}

#[test]
fn identical_inputs_match() {
    let a = analyze(&six_sine(None), None).unwrap();
    let c = compare(&a, &a);
    let d = &c.deltas;
    assert_eq!(d.integrated_lu, Some(0.0));
    assert_eq!(d.true_peak_db, 0.0);
    assert_eq!(d.lra_lu, a.loudness.lra_lu.map(|_| 0.0));
    assert_eq!(d.crest_db, Some(0.0));
    assert!(d.bands.iter().all(|b| *b == Some(0.0)));
    assert_eq!(d.low_end_ratio_db, Some(0.0));
    assert_eq!(d.side_to_mid_db, 0.0);
    assert_eq!(d.centroid_median_oct, Some(0.0));
    assert_eq!(d.centroid_range_oct, Some(0.0));
    assert_eq!(c.verdicts, vec!["matches the reference within tolerance"]);
    let j = c.to_json();
    assert_eq!(j["deltas"]["bands"]["low_mid"], 0.0);
    assert_eq!(j["verdicts"][0], "matches the reference within tolerance");
}

#[test]
fn muddier_low_mid() {
    let r = analyze(&six_sine(None), None).unwrap();
    let o = analyze(&six_sine(Some(-14.0)), None).unwrap();
    let c = compare(&o, &r);
    assert!(c.deltas.bands[2].unwrap() > 3.0, "{:?}", c.deltas.bands);
    let hits: Vec<_> = c
        .verdicts
        .iter()
        .filter(|v| {
            v.starts_with("low-mid +")
                && v.ends_with(" dB vs reference: muddier")
                && v["low-mid +".len()..v.len() - " dB vs reference: muddier".len()]
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '.')
        })
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", c.verdicts);
}

#[test]
fn quieter_than_reference() {
    let base = six_sine(None);
    let r = analyze(&base, None).unwrap();
    let o = analyze(&scaled(&base, 0.5012), None).unwrap();
    let c = compare(&o, &r);
    assert!((c.deltas.integrated_lu.unwrap() + 6.0).abs() < 0.1);
    assert!(
        c.verdicts
            .contains(&"loudness -6.0 LU vs reference: quieter".to_string()),
        "{:?}",
        c.verdicts
    );
    for b in c.deltas.bands {
        assert!(b.unwrap().abs() <= 0.05, "{:?}", c.deltas.bands);
    }
}

#[test]
fn true_peak_over() {
    let r = analyze(&six_sine(None), None).unwrap();
    let o = analyze(&mono(SR, sine(SR, 1000.0, -0.5, 3.0, 0.0)), None).unwrap();
    let c = compare(&o, &r);
    assert!(
        c.verdicts
            .contains(&"true peak -0.5 dBTP: over the -1 dBTP ceiling".to_string()),
        "{:?} tp {}",
        c.verdicts,
        o.loudness.true_peak_dbtp
    );
}

#[test]
fn less_sweep() {
    let sweep = concat(
        &mono(SR, sine(SR, 500.0, -12.0, 10.0, 0.0)),
        &mono(SR, sine(SR, 4000.0, -12.0, 10.0, 0.0)),
    );
    let r = analyze(&sweep, None).unwrap();
    let o = analyze(&mono(SR, sine(SR, 1000.0, -12.0, 20.0, 0.0)), None).unwrap();
    let c = compare(&o, &r);
    assert!(
        c.verdicts
            .iter()
            .any(|v| v.starts_with("filter movement 0.0 oct vs 3.0 oct")
                && v.ends_with("less sweep")),
        "{:?}",
        c.verdicts
    );
}

#[test]
fn silent_reference_tolerated() {
    let s = analyze(&mono(SR, vec![0.0; SR as usize * 3]), None).unwrap();
    let a = analyze(&six_sine(None), None).unwrap();
    let c = compare(&a, &s);
    assert_eq!(c.deltas.integrated_lu, None);
    assert_eq!(c.deltas.bands[0], None);
    assert_eq!(
        c.to_json()["deltas"]["bands"]["sub"],
        serde_json::Value::Null
    );
}
