//! Backward-compatibility pin: a song with no `[mix]` table, no strip params
//! and no `eq` renders bitwise as it did before the effects and mixer.
//!
//! The constants were recorded from the engine before Plan 5 touched it
//! (the `feat/ears` head, merged as the Plan 4 merge commit). Never update
//! them to make this test pass: a difference is a regression.

mod common;

use common::{MINI, MIXED, compiled};
use tune_engine::{RenderOptions, render};
use tune_song::{Migration, migrate};

/// `songs/first-acid` as it was before the mixer existed, frozen so the pin
/// guards the engine while the live song keeps changing.
const FIRST_ACID: &str = include_str!("../../tune-song/tests/fixtures/first-acid-v1.toml");

/// `first-acid` with the Plan 5 mixer, frozen like [`FIRST_ACID`].
const FIRST_ACID_MIXED: &str = include_str!("../../tune-song/tests/fixtures/first-acid-mixed.toml");

/// The committed v2 copies of the fixtures (the migration's output).
const MINI_V2: &str = include_str!("../../tune-song/tests/fixtures/v2/mini.toml");
const MIXED_V2: &str = include_str!("../../tune-song/tests/fixtures/v2/mixed.toml");
const FIRST_ACID_V2: &str = include_str!("../../tune-song/tests/fixtures/v2/first-acid-v1.toml");
const FIRST_ACID_MIXED_V2: &str =
    include_str!("../../tune-song/tests/fixtures/v2/first-acid-mixed.toml");

/// What a whole-song render with stems is pinned by.
#[derive(Debug, PartialEq)]
struct Fingerprint {
    /// FNV-1a of the mix.
    mix: u64,
    /// `(track id, FNV-1a of its stem)` in song order.
    stems: Vec<(String, u64)>,
    peak_bits: u64,
    pre_clip_peak_bits: u64,
    knee_samples: u64,
}

impl Fingerprint {
    fn pinned(
        mix: u64,
        stems: &[(&str, u64)],
        peak_bits: u64,
        pre_clip_peak_bits: u64,
        knee_samples: u64,
    ) -> Self {
        Self {
            mix,
            stems: stems.iter().map(|&(id, h)| (id.to_owned(), h)).collect(),
            peak_bits,
            pre_clip_peak_bits,
            knee_samples,
        }
    }
}

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

/// Renders all of `src` with stems.
fn fingerprint(src: &str) -> Fingerprint {
    let song = compiled(src);
    let opts = RenderOptions {
        start_bar: 0,
        end_bar: song.total_bars,
        stems: true,
    };
    let r = render(song, &opts).unwrap();
    Fingerprint {
        mix: fnv1a(&r.mix),
        stems: r
            .stems
            .iter()
            .map(|(id, s)| (id.clone(), fnv1a(s)))
            .collect(),
        peak_bits: r.meter.peak.to_bits(),
        pre_clip_peak_bits: r.meter.pre_clip_peak.to_bits(),
        knee_samples: r.meter.knee_samples,
    }
}

#[test]
fn mini_renders_as_before() {
    assert_eq!(
        fingerprint(MINI),
        Fingerprint::pinned(
            0x3cb4_018b_5885_1398,
            &[
                ("kick", 0x8ef5_6289_e893_c6bd),
                ("acid", 0x58c6_1ef2_b9f0_8921),
                ("hat", 0xa834_18dc_bebd_21b6),
            ],
            0x3fe8_399f_f012_f8ba,
            0x3fe9_d80b_0101_da26,
            8440,
        )
    );
}

#[test]
fn first_acid_renders_as_before() {
    assert_eq!(
        fingerprint(FIRST_ACID),
        Fingerprint::pinned(
            0xcc4f_9320_a2f2_d1ff,
            &[
                ("kick", 0x313b_7bb2_7445_8115),
                ("clap", 0x5a70_dbd6_8c24_f84d),
                ("hat_closed", 0xe84f_482c_5657_c917),
                ("hat_open", 0xa6fc_7e2b_c125_476f),
                ("acid", 0x37a4_7856_7490_8b29),
            ],
            0x3fe7_cc36_17e4_de19,
            0x3fe9_1f17_6fea_b576,
            70718,
        )
    );
}

/// [`fingerprint`] of each of `srcs`, rendered side by side (a debug-build
/// render of first-acid takes about 20 s).
fn fingerprints<const N: usize>(srcs: [&str; N]) -> [Fingerprint; N] {
    std::thread::scope(|s| {
        srcs.map(|src| s.spawn(move || fingerprint(src)))
            .map(|h| h.join().expect("render thread"))
    })
}

/// `src` (a v1 file) migrated to v2 in memory.
fn migrated(src: &str) -> String {
    match migrate(src) {
        Ok(Migration::Migrated { text, .. }) => text,
        other => panic!("expected a v1 file to migrate, got {other:?}"),
    }
}

#[test]
fn mini_v2_and_migrated_render_as_before() {
    let pinned = || {
        Fingerprint::pinned(
            0x3cb4_018b_5885_1398,
            &[
                ("kick", 0x8ef5_6289_e893_c6bd),
                ("acid", 0x58c6_1ef2_b9f0_8921),
                ("hat", 0xa834_18dc_bebd_21b6),
            ],
            0x3fe8_399f_f012_f8ba,
            0x3fe9_d80b_0101_da26,
            8440,
        )
    };
    let [copy, in_memory] = fingerprints([MINI_V2, &migrated(MINI)]);
    assert_eq!(copy, pinned(), "v2 copy");
    assert_eq!(in_memory, pinned(), "migrated");
}

#[test]
fn first_acid_v2_and_migrated_render_as_before() {
    let pinned = || {
        Fingerprint::pinned(
            0xcc4f_9320_a2f2_d1ff,
            &[
                ("kick", 0x313b_7bb2_7445_8115),
                ("clap", 0x5a70_dbd6_8c24_f84d),
                ("hat_closed", 0xe84f_482c_5657_c917),
                ("hat_open", 0xa6fc_7e2b_c125_476f),
                ("acid", 0x37a4_7856_7490_8b29),
            ],
            0x3fe7_cc36_17e4_de19,
            0x3fe9_1f17_6fea_b576,
            70718,
        )
    };
    let [copy, in_memory] = fingerprints([FIRST_ACID_V2, &migrated(FIRST_ACID)]);
    assert_eq!(copy, pinned(), "v2 copy");
    assert_eq!(in_memory, pinned(), "migrated");
}

#[test]
fn mixed_fixtures_render_alike_in_v1_and_v2() {
    for (name, v1, v2) in [
        ("mixed", MIXED, MIXED_V2),
        ("first-acid-mixed", FIRST_ACID_MIXED, FIRST_ACID_MIXED_V2),
    ] {
        let [want, copy, in_memory] = fingerprints([v1, v2, &migrated(v1)]);
        assert_eq!(copy, want, "{name}: v2 copy");
        assert_eq!(in_memory, want, "{name}: migrated");
    }
}
