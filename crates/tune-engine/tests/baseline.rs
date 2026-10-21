//! Backward-compatibility pin: a song with no `[mix]` table, no strip params
//! and no `eq` renders bitwise as it did before the effects and mixer.
//!
//! The constants were recorded from the engine before Plan 5 touched it
//! (the `feat/ears` head, merged as the Plan 4 merge commit). Never update
//! them to make this test pass: a difference is a regression.

mod common;

use common::{MINI, compiled};
use tune_engine::{RenderOptions, render};

const MIX_HASH: u64 = 0x3cb4_018b_5885_1398;
const STEM_HASHES: [(&str, u64); 3] = [
    ("kick", 0x8ef5_6289_e893_c6bd),
    ("acid", 0x58c6_1ef2_b9f0_8921),
    ("hat", 0xa834_18dc_bebd_21b6),
];
const PEAK_BITS: u64 = 0x3fe8_399f_f012_f8ba;
const PRE_CLIP_PEAK_BITS: u64 = 0x3fe9_d80b_0101_da26;
const KNEE_SAMPLES: u64 = 8440;

/// 64-bit FNV-1a over the little-endian bytes of each sample's bits.
fn fnv1a(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

#[test]
fn mini_renders_as_before() {
    let song = compiled(MINI);
    let opts = RenderOptions {
        start_bar: 0,
        end_bar: song.total_bars,
        stems: true,
    };
    let r = render(song, &opts).unwrap();
    let stems: Vec<(&str, u64)> = r
        .stems
        .iter()
        .map(|(id, s)| (id.as_str(), fnv1a(s)))
        .collect();
    let got = (
        fnv1a(&r.mix),
        stems,
        r.meter.peak.to_bits(),
        r.meter.pre_clip_peak.to_bits(),
        r.meter.knee_samples,
    );
    assert_eq!(
        got,
        (
            MIX_HASH,
            STEM_HASHES.to_vec(),
            PEAK_BITS,
            PRE_CLIP_PEAK_BITS,
            KNEE_SAMPLES
        )
    );
}
