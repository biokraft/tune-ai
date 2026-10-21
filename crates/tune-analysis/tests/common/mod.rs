#![allow(dead_code)]

use std::path::Path;
use tune_analysis::Audio;

pub fn sine(sr: u32, hz: f64, dbfs: f64, secs: f64, phase: f64) -> Vec<f32> {
    let amp = 10f64.powf(dbfs / 20.0);
    let n = (sr as f64 * secs).round() as usize;
    (0..n)
        .map(|i| {
            (amp * (2.0 * std::f64::consts::PI * hz * i as f64 / sr as f64 + phase).sin()) as f32
        })
        .collect()
}

/// Uniform noise in [-1, 1] scaled to `dbfs` peak, splitmix64-seeded.
pub fn noise(sr: u32, dbfs: f64, secs: f64, seed: u64) -> Vec<f32> {
    let amp = 10f64.powf(dbfs / 20.0);
    let n = (sr as f64 * secs).round() as usize;
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let u = (z >> 11) as f64 / (1u64 << 53) as f64;
            (amp * (2.0 * u - 1.0)) as f32
        })
        .collect()
}

pub fn mono(sr: u32, x: Vec<f32>) -> Audio {
    Audio {
        sample_rate: sr,
        channels: vec![x],
    }
}

pub fn stereo(sr: u32, l: Vec<f32>, r: Vec<f32>) -> Audio {
    Audio {
        sample_rate: sr,
        channels: vec![l, r],
    }
}

pub fn concat(a: &Audio, b: &Audio) -> Audio {
    assert_eq!(a.sample_rate, b.sample_rate);
    assert_eq!(a.channels.len(), b.channels.len());
    let channels = a
        .channels
        .iter()
        .zip(&b.channels)
        .map(|(x, y)| x.iter().chain(y).copied().collect())
        .collect();
    Audio {
        sample_rate: a.sample_rate,
        channels,
    }
}

/// Write `audio` as WAV. `bits` 8/16/24/32 are integer, 0 means 32-bit float.
pub fn write_wav(path: &Path, audio: &Audio, bits: u16) {
    let float = bits == 0;
    let spec = hound::WavSpec {
        channels: audio.channels.len() as u16,
        sample_rate: audio.sample_rate,
        bits_per_sample: if float { 32 } else { bits },
        sample_format: if float {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..audio.frames() {
        for ch in &audio.channels {
            if float {
                w.write_sample(ch[i]).unwrap();
            } else {
                let scale = (1i64 << (bits - 1)) as f64;
                let v = (ch[i] as f64 * scale).round().clamp(-scale, scale - 1.0) as i32;
                w.write_sample(v).unwrap();
            }
        }
    }
    w.finalize().unwrap();
}
