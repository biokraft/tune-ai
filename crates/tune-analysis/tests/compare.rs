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
    assert_eq!(d.side_to_mid_db, Some(0.0));
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
            .contains(&"true peak -0.50 dBTP: over the -1 dBTP ceiling".to_string()),
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

#[test]
fn true_peak_just_over_shows_two_decimals() {
    let r = analyze(&six_sine(None), None).unwrap();
    let o = analyze(&mono(SR, sine(SR, 1000.0, -0.97, 3.0, 0.0)), None).unwrap();
    let c = compare(&o, &r);
    assert!(
        c.verdicts
            .contains(&"true peak -0.97 dBTP: over the -1 dBTP ceiling".to_string()),
        "{:?}",
        c.verdicts
    );
}

#[test]
fn empty_air_band_not_compared() {
    let mut x = vec![0.0f32; SR as usize * 5];
    for hz in [40.0, 150.0, 500.0, 2000.0, 6000.0] {
        for (o, v) in x.iter_mut().zip(sine(SR, hz, -20.0, 5.0, 0.0)) {
            *o += v;
        }
    }
    let r = analyze(&mono(SR, x), None).unwrap();
    let o = analyze(&six_sine(None), None).unwrap();
    let c = compare(&o, &r);
    assert_eq!(c.deltas.bands[5], None);
    assert!(
        c.verdicts
            .contains(&"air: one side has no content there, not compared".to_string()),
        "{:?}",
        c.verdicts
    );
    assert!(
        !c.verdicts
            .iter()
            .any(|v| v.starts_with("air") && (v.contains("+9") || v.contains("-9"))),
        "{:?}",
        c.verdicts
    );
    assert!(c.to_json()["deltas"]["bands"]["air"].is_null());
}

#[test]
fn mono_reference_width_not_compared() {
    let r = analyze(&six_sine(None), None).unwrap();
    let base = &six_sine(None).channels[0];
    let l = base.clone();
    let rr: Vec<f32> = base.iter().map(|v| v * 0.5).collect();
    let o = analyze(&common::stereo(SR, l, rr), None).unwrap();
    let c = compare(&o, &r);
    assert_eq!(c.deltas.side_to_mid_db, None);
    assert!(
        c.verdicts
            .contains(&"width: the reference is mono, not compared".to_string()),
        "{:?}",
        c.verdicts
    );
    assert!(!c.verdicts.iter().any(|v| v.starts_with("side/mid")));
    assert!(c.to_json()["deltas"]["side_to_mid_db"].is_null());
}

#[test]
fn near_mono_reference_width_not_compared() {
    // A mono master decoded from a lossy stereo file keeps a faint side
    // residue (about -90 dB below the mid). That is still mono to the ear,
    // so width must not be compared against it.
    let base = &six_sine(None).channels[0];
    let residue = common::noise(SR, -110.0, 5.0, 7);
    let rl = base.clone();
    let rr: Vec<f32> = base.iter().zip(&residue).map(|(v, n)| v + n).collect();
    let r = analyze(&common::stereo(SR, rl, rr), None).unwrap();
    assert!(
        r.spectral.stereo.side_to_mid_db > -120.0 && r.spectral.stereo.side_to_mid_db < -60.0,
        "{}",
        r.spectral.stereo.side_to_mid_db
    );
    let l = base.clone();
    let ours_r: Vec<f32> = base.iter().map(|v| v * 0.5).collect();
    let o = analyze(&common::stereo(SR, l, ours_r), None).unwrap();
    let c = compare(&o, &r);
    assert_eq!(c.deltas.side_to_mid_db, None);
    assert!(
        c.verdicts
            .contains(&"width: the reference is mono, not compared".to_string()),
        "{:?}",
        c.verdicts
    );
    assert!(!c.verdicts.iter().any(|v| v.starts_with("side/mid")));
}

/// Stereo from a mid and a side that is `side_db` below it, exactly.
fn width_at(side_db: f64) -> Audio {
    let m = &six_sine(None).channels[0];
    let k = 10f64.powf(side_db / 20.0) as f32;
    let l = m.iter().map(|v| v + v * k).collect();
    let r = m.iter().map(|v| v - v * k).collect();
    common::stereo(SR, l, r)
}

#[test]
fn mono_boundary_is_60_db_below_the_mid() {
    let wide = analyze(&width_at(-10.0), None).unwrap();
    let at_59 = analyze(&width_at(-59.0), None).unwrap();
    let at_61 = analyze(&width_at(-61.0), None).unwrap();
    assert!((at_59.spectral.stereo.side_to_mid_db + 59.0).abs() < 0.01);
    assert!((at_61.spectral.stereo.side_to_mid_db + 61.0).abs() < 0.01);

    // -59 dB is still (barely) stereo: width is compared.
    let c = compare(&wide, &at_59);
    let d = c.deltas.side_to_mid_db.expect("compared");
    assert!((d - 49.0).abs() < 0.05, "{d}");
    assert!(
        c.verdicts.iter().any(|v| v.starts_with("side/mid +49")),
        "{:?}",
        c.verdicts
    );

    // -61 dB is mono: not compared, and the verdict names the mono side.
    let c = compare(&wide, &at_61);
    assert_eq!(c.deltas.side_to_mid_db, None);
    assert!(
        c.verdicts
            .contains(&"width: the reference is mono, not compared".to_string()),
        "{:?}",
        c.verdicts
    );
    let c = compare(&at_61, &wide);
    assert_eq!(c.deltas.side_to_mid_db, None);
    assert!(
        c.verdicts
            .contains(&"width: ours is mono, not compared".to_string()),
        "{:?}",
        c.verdicts
    );
}

#[test]
fn anti_phase_is_not_called_mono() {
    // No mid at all (R = -L) reads +200 dB: out of phase, not mono.
    let x = six_sine(None).channels[0].clone();
    let neg = x.iter().map(|v| -v).collect();
    let anti = analyze(&common::stereo(SR, x, neg), None).unwrap();
    assert_eq!(anti.spectral.stereo.side_to_mid_db, 200.0);
    let wide = analyze(&width_at(-10.0), None).unwrap();
    let c = compare(&anti, &wide);
    assert_eq!(c.deltas.side_to_mid_db, None);
    assert!(
        c.verdicts
            .contains(&"width: ours has no mid (fully out of phase), not compared".to_string()),
        "{:?}",
        c.verdicts
    );
    let mono = analyze(&six_sine(None), None).unwrap();
    let c = compare(&anti, &mono);
    assert_eq!(c.deltas.side_to_mid_db, None);
    assert!(
        c.verdicts.contains(
            &"width: ours has no mid (fully out of phase) and the reference is mono, not compared"
                .to_string()
        ),
        "{:?}",
        c.verdicts
    );
}
