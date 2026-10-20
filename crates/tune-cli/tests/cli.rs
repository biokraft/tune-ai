use assert_cmd::Command;
use std::path::Path;
use tune_cli::render::{RenderSpec, Sweep, render_acid};
use tune_dsp::DistortionMode;
use tune_dsp::acid303::AcidPattern;

fn tune() -> Command {
    Command::cargo_bin("tune").unwrap()
}

fn dir_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

fn demo_to(path: &Path) {
    tune()
        .args(["render-303", "--demo", "--out"])
        .arg(path)
        .assert()
        .success();
}

#[test]
fn demo_writes_expected_wav() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let assert = tune()
        .args(["render-303", "--demo", "--out"])
        .arg(&out)
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.starts_with("rendered "), "{stdout}");
    assert!(!stdout.contains("clipped"), "{stdout}");
    let mut r = hound::WavReader::open(&out).unwrap();
    let spec = r.spec();
    assert_eq!(spec.sample_rate, 48_000);
    assert_eq!(spec.channels, 2);
    assert_eq!(spec.bits_per_sample, 24);
    let expected = (8.0_f64 * 4.0 * 60.0 / 145.0 * 48_000.0).round() as u32;
    assert_eq!(r.duration(), expected);
    let peak = r
        .samples::<i32>()
        .map(|s| s.unwrap().unsigned_abs())
        .max()
        .unwrap() as f64
        / 8_388_608.0;
    assert!(peak > 0.05, "demo should not be silent, peak {peak}");
    assert!(
        20.0 * peak.log10() <= -1.0 + 0.01,
        "demo peak {} dBFS",
        20.0 * peak.log10()
    );
}

#[test]
fn demo_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.wav");
    let b = dir.path().join("b.wav");
    demo_to(&a);
    demo_to(&b);
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
}

fn spec() -> RenderSpec {
    RenderSpec {
        pattern: AcidPattern::parse("C2 C2! C3~ C2 . D#2 C2~ G2!").unwrap(),
        bpm: 130.0,
        bars: 1,
        sample_rate: 44_100,
        params: vec![("acid303.resonance".into(), 70.0)],
        sweeps: vec![Sweep {
            id: "acid303.cutoff".into(),
            from_norm: 0.1,
            to_norm: 0.9,
        }],
        distortion: Some(DistortionMode::Hard),
    }
}

#[test]
fn render_is_bitwise_deterministic() {
    let a = render_acid(&spec()).unwrap();
    let b = render_acid(&spec()).unwrap();
    assert_eq!(
        a.len(),
        (4.0_f64 * 60.0 / 130.0 * 44_100.0).round() as usize
    );
    assert!(a.iter().any(|x| *x != 0.0));
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}

#[test]
fn render_rejects_bad_specs() {
    let mut s = spec();
    s.sweeps[0].to_norm = 1.5;
    assert!(render_acid(&s).is_err());
    let mut s = spec();
    s.params.push(("synth.foo".into(), 1.0));
    assert!(
        render_acid(&s)
            .unwrap_err()
            .to_string()
            .contains("unknown parameter")
    );
    let mut s = spec();
    s.distortion = None;
    s.params.push(("dist.drive".into(), 999.0));
    assert!(
        render_acid(&s)
            .unwrap_err()
            .to_string()
            .contains("out of range")
    );
}

fn assert_fails(args: &[&str], needle: &str, dir: &Path) {
    let assert = tune().arg("render-303").args(args).assert().code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains(needle),
        "stderr {stderr:?} lacks {needle:?}"
    );
    assert!(dir_is_empty(dir), "files left behind in {}", dir.display());
}

#[test]
fn bad_input_exits_2_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let o = out.to_str().unwrap();
    assert_fails(
        &["--demo", "--set", "acid303.nope=1", "--out", o],
        "unknown parameter",
        dir.path(),
    );
    assert_fails(
        &["--demo", "--set", "acid303.cutoff=9000", "--out", o],
        "out of range",
        dir.path(),
    );
    assert_fails(&["--pattern", "C2 H2", "--out", o], "bad token", dir.path());
    assert_fails(
        &["--demo", "--set", "dist.nope=1", "--out", o],
        "unknown parameter",
        dir.path(),
    );
    assert_fails(
        &["--demo", "--sweep", "acid303.cutoff=0:2", "--out", o],
        "sweep",
        dir.path(),
    );
    let bad = "/nonexistent-dir/x.wav";
    let assert = tune()
        .args(["render-303", "--demo", "--out", bad])
        .assert()
        .code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains(bad), "{stderr}");
    assert!(dir_is_empty(dir.path()));
}

#[test]
fn explicit_flags_override_demo() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("y.wav");
    tune()
        .args([
            "render-303",
            "--demo",
            "--bars",
            "1",
            "--bpm",
            "120",
            "--out",
        ])
        .arg(&out)
        .assert()
        .success();
    let r = hound::WavReader::open(&out).unwrap();
    assert_eq!(r.duration(), 96_000);
}

#[test]
fn bad_sample_rate_and_length_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let o = out.to_str().unwrap();
    assert_fails(
        &["--demo", "--sample-rate", "48", "--out", o],
        "sample rate",
        dir.path(),
    );
    assert_fails(
        &["--demo", "--sample-rate", "1", "--out", o],
        "sample rate",
        dir.path(),
    );
    assert_fails(
        &["--demo", "--sample-rate", "500000", "--out", o],
        "8000..=384000",
        dir.path(),
    );
    assert_fails(
        &["--demo", "--bpm", "0.0001", "--out", o],
        "too long",
        dir.path(),
    );
}
