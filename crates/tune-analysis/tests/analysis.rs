mod common;

use common::{concat, sine, stereo};
use tune_analysis::{
    ACID_LUFS, Audio, BarMap, SectionSpan, analyze, analyze_sections, classify_targets,
};

const SR: u32 = 48_000;
const BAR: usize = 96_000;

fn tone(dbfs: f64, bars: usize) -> Audio {
    let x = sine(SR, 1000.0, dbfs, 2.0 * bars as f64, 0.0);
    stereo(SR, x.clone(), x)
}

fn silence(bars: usize) -> Audio {
    stereo(SR, vec![0.0; BAR * bars], vec![0.0; BAR * bars])
}

fn map(first_bar: u32, bars: usize) -> BarMap {
    BarMap {
        first_bar,
        bar_frames: (0..=bars).map(|i| i * BAR).collect(),
        sections: vec![
            SectionSpan {
                name: "a".into(),
                start_bar: 0,
                bars: 2,
            },
            SectionSpan {
                name: "b".into(),
                start_bar: 2,
                bars: 2,
            },
        ],
    }
}

#[test]
fn targets_classify() {
    assert_eq!(ACID_LUFS, (-8.0, -6.0));
    assert_eq!(classify_targets(Some(-7.0), -3.0).integrated, "within");
    assert_eq!(classify_targets(Some(-10.0), -3.0).integrated, "below");
    assert_eq!(classify_targets(Some(-5.0), -3.0).integrated, "above");
    assert_eq!(classify_targets(None, -3.0).integrated, "silent");
    assert_eq!(classify_targets(Some(-7.0), -1.0).true_peak, "ok");
    assert_eq!(classify_targets(Some(-7.0), -0.5).true_peak, "over");
}

#[test]
fn sections_follow_the_bar_map() {
    let audio = concat(&tone(-20.0, 2), &tone(-30.0, 2));
    let r = analyze_sections(&audio, &map(0, 4)).unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(
        (r[0].name.as_str(), r[0].start_bar, r[0].end_bar),
        ("a", 1, 2)
    );
    assert_eq!(
        (r[1].name.as_str(), r[1].start_bar, r[1].end_bar),
        ("b", 3, 4)
    );
    assert!((r[0].integrated_lufs.unwrap() + 20.0).abs() < 0.2);
    assert!((r[1].integrated_lufs.unwrap() + 30.0).abs() < 0.2);
    assert!((r[1].delta_lu.unwrap() + 10.0).abs() < 0.3);
    assert!(r[0].delta_lu.is_none());
    assert!(!r[0].partial && !r[1].partial);
}

#[test]
fn unchanged_section_noted() {
    let audio = concat(&tone(-20.0, 2), &tone(-20.0, 2));
    let r = analyze_sections(&audio, &map(0, 4)).unwrap();
    assert_eq!(
        r[1].notes,
        vec!["same energy and balance as `a`: nothing changes here".to_string()]
    );
}

#[test]
fn partial_sections_flagged() {
    let audio = concat(&tone(-20.0, 1), &tone(-20.0, 2));
    let r = analyze_sections(&audio, &map(1, 3)).unwrap();
    assert_eq!((r[0].start_bar, r[0].end_bar), (2, 2));
    assert!(r[0].partial);
    assert!(!r[1].partial);
}

#[test]
fn silent_section_is_null_in_json() {
    let audio = concat(&tone(-20.0, 2), &silence(2));
    let a = analyze(&audio, Some(&map(0, 4))).unwrap();
    let j = a.to_json();
    let b = &j["sections"][1];
    assert!(b["integrated_lufs"].is_null());
    assert!(b["notes"].as_array().unwrap().iter().any(|n| n == "silent"));
    let s = serde_json::to_string(&j).unwrap();
    assert!(!s.contains("NaN") && !s.contains("inf"));
}

fn keys(v: &serde_json::Value) -> Vec<&str> {
    v.as_object().unwrap().keys().map(String::as_str).collect()
}

#[test]
fn json_keys_are_stable() {
    let audio = tone(-20.0, 4);
    let a = analyze(&audio, Some(&map(0, 4))).unwrap();
    let all = [
        "loudness",
        "targets",
        "bands",
        "low_end",
        "stereo",
        "brightness",
        "series",
        "sections",
    ];
    let j = a.to_json();
    assert_eq!(keys(&j), all);
    assert_eq!(keys(&a.summary_json()), all[..6]);
    assert_eq!(
        keys(&j["loudness"]),
        [
            "integrated_lufs",
            "short_term_max_lufs",
            "lra_lu",
            "sample_peak_dbfs",
            "true_peak_dbtp",
            "rms_dbfs",
            "crest_db",
            "plr_db"
        ]
    );
    assert_eq!(
        keys(&j["targets"]),
        [
            "genre",
            "integrated_lufs",
            "integrated",
            "true_peak_max_dbtp",
            "true_peak"
        ]
    );
    assert_eq!(
        keys(&j["bands"]),
        ["sub", "low", "low_mid", "mid", "high", "air"]
    );
    assert_eq!(
        keys(&j["low_end"]),
        ["kick_region_db", "bass_region_db", "ratio_db"]
    );
    assert_eq!(
        keys(&j["stereo"]),
        ["correlation", "side_to_mid_db", "low_correlation"]
    );
    assert_eq!(
        keys(&j["brightness"]),
        ["median_hz", "p10_hz", "p90_hz", "range_oct", "profile_hz"]
    );
    assert_eq!(
        keys(&j["series"]),
        ["hop_seconds", "short_term_lufs", "centroid_hz"]
    );
    assert_eq!(
        keys(&j["sections"][0]),
        [
            "name",
            "start_bar",
            "end_bar",
            "partial",
            "integrated_lufs",
            "short_term_max_lufs",
            "delta_lu",
            "bands",
            "centroid_median_hz",
            "notes"
        ]
    );
    assert!(analyze(&audio, None).unwrap().to_json()["sections"].is_null());
}
