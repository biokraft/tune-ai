mod common;

use common::*;
use std::path::Path;
use std::process::Command;
use tune_analysis::{Audio, decode_file};

fn max_err(a: &Audio, b: &Audio) -> f32 {
    assert_eq!(a.channels.len(), b.channels.len());
    let mut m = 0f32;
    for (x, y) in a.channels.iter().zip(&b.channels) {
        assert_eq!(x.len(), y.len());
        for (p, q) in x.iter().zip(y) {
            m = m.max((p - q).abs());
        }
    }
    m
}

fn rms(a: &Audio) -> f64 {
    let mut s = 0f64;
    let mut n = 0usize;
    for ch in &a.channels {
        for v in ch {
            s += (*v as f64) * (*v as f64);
            n += 1;
        }
    }
    (s / n as f64).sqrt()
}

/// 16-bit-exact stereo 440 Hz source, 3 s at 48 kHz, as an Audio quantised to 16 bits.
fn quantised_source() -> Audio {
    let l = sine(48000, 440.0, -6.0, 3.0, 0.0);
    let r = sine(48000, 440.0, -9.0, 3.0, 0.5);
    let q = |v: Vec<f32>| {
        v.into_iter()
            .map(|x| (x * 32768.0).round() / 32768.0)
            .collect()
    };
    stereo(48000, q(l), q(r))
}

#[test]
fn wav_pcm24_stereo_roundtrip() {
    let l = sine(48000, 1000.0, -6.0, 2.0, 0.0);
    let r: Vec<f32> = l.iter().map(|x| -x).collect();
    let src = stereo(48000, l, r);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.wav");
    write_wav(&p, &src, 24);
    let got = decode_file(&p).unwrap();
    assert_eq!(got.sample_rate, 48000);
    assert_eq!(got.channels.len(), 2);
    assert_eq!(got.frames(), 96000);
    assert!(max_err(&got, &src) < 1e-6);
}

#[test]
fn wav_pcm16_mono_44k1() {
    let src = mono(44100, sine(44100, 440.0, -6.0, 1.0, 0.0));
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.WAV");
    write_wav(&p, &src, 16);
    let got = decode_file(&p).unwrap();
    assert_eq!(got.sample_rate, 44100);
    assert_eq!(got.channels.len(), 1);
    assert!(max_err(&got, &src) <= 1.0 / 32768.0);
}

#[test]
fn wav_float32() {
    let src = mono(48000, vec![0.0, 1.5, -0.25, 0.5]);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("f.wav");
    write_wav(&p, &src, 0);
    let got = decode_file(&p).unwrap();
    assert_eq!(got.channels[0], vec![0.0, 1.5, -0.25, 0.5]);
}

#[test]
fn rejects_more_than_two_channels() {
    let ch = vec![sine(48000, 440.0, -6.0, 0.1, 0.0); 6];
    let src = Audio {
        sample_rate: 48000,
        channels: ch,
    };
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("six.wav");
    write_wav(&p, &src, 16);
    let err = decode_file(&p).unwrap_err();
    assert_eq!(
        err.to_string(),
        "only mono or stereo audio is supported (file has 6 channels)"
    );
}

#[test]
fn corrupt_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.mp3");
    std::fs::write(&p, "not audio").unwrap();
    let err = decode_file(&p).unwrap_err().to_string();
    assert!(err.starts_with("cannot decode"), "{err}");
}

#[test]
fn missing_file_fails() {
    let err = decode_file(Path::new("/nonexistent/none.wav"))
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("cannot open"), "{err}");
    let err = decode_file(Path::new("/nonexistent/none.mp3"))
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("cannot open"), "{err}");
}

#[cfg(target_os = "macos")]
#[test]
fn flac_and_aac_via_afconvert() {
    let src = quantised_source();
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("src.wav");
    write_wav(&wav, &src, 16);

    let flac = dir.path().join("a.flac");
    let st = Command::new("afconvert")
        .args(["-f", "flac", "-d", "flac"])
        .arg(&wav)
        .arg(&flac)
        .status()
        .unwrap();
    assert!(st.success());
    let got = decode_file(&flac).unwrap();
    assert_eq!(got.sample_rate, 48000);
    assert_eq!(got.frames(), src.frames());
    assert!(max_err(&got, &src) <= 1.0 / 32768.0);

    let aac = dir.path().join("a.m4a");
    let st = Command::new("afconvert")
        .args(["-f", "m4af", "-d", "aac"])
        .arg(&wav)
        .arg(&aac)
        .status()
        .unwrap();
    assert!(st.success());
    let got = decode_file(&aac).unwrap();
    assert_eq!(got.sample_rate, 48000);
    assert_eq!(got.channels.len(), 2);
    // symphonia 0.5.5 ignores the AAC edit list, so priming (2112) and the
    // final-packet remainder (320) from afconvert stay in: 2432 extra frames.
    assert!(
        (got.frames() as i64 - 144000).abs() <= 2432,
        "frames {}",
        got.frames()
    );
}

#[test]
fn mp3_via_lame() {
    let lame_ok = Command::new("lame").arg("--version").output().is_ok();
    if !lame_ok {
        println!("skipped: lame not installed");
        return;
    }
    let src = quantised_source();
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("src.wav");
    write_wav(&wav, &src, 16);
    let mp3 = dir.path().join("a.mp3");
    let st = Command::new("lame")
        .args(["--quiet", "-b", "320"])
        .arg(&wav)
        .arg(&mp3)
        .status()
        .unwrap();
    assert!(st.success());
    let got = decode_file(&mp3).unwrap();
    assert_eq!(got.sample_rate, 48000);
    assert_eq!(got.channels.len(), 2);
    assert!(
        (got.frames() as i64 - 144000).abs() <= 2304,
        "frames {}",
        got.frames()
    );
    let db = 20.0 * (rms(&got) / rms(&src)).log10();
    assert!(db.abs() <= 0.2, "rms delta {db} dB");
}

#[test]
fn slice_clamps() {
    let a = stereo(48000, vec![0.0; 96000], vec![0.0; 96000]);
    assert_eq!(a.slice(90000..200000).frames(), 6000);
    assert_eq!(a.slice(200000..300000).frames(), 0);
}
