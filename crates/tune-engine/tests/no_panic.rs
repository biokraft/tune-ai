//! Every song the reader accepts compiles and plays. Songs come from a
//! seeded generator that mixes the modules, chains, clips, placements and
//! lanes of format version 2; the generator does not try to stay valid, so
//! the reader's checks are what stands between it and the engine.

use std::sync::Arc;

use indexmap::IndexMap;
use tune_dsp::acid303::AcidPattern;
use tune_dsp::{DrumPattern, STRIP_PARAMS};
use tune_engine::{CompiledSong, RenderOptions, render};
use tune_song::{
    Clip, ClipKind, Device, Lane, MIX_TRACK_ID, Mix, MixUnit, ModuleDescriptor, Placement,
    Registry, Section, Song, Track, parse_song,
};

const SONGS: usize = 300;

/// splitmix64: small, seedable and good enough to pick test cases.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A whole number in `lo..=hi`.
    fn int(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next() % u64::from(hi - lo + 1)) as u32
    }

    /// A float in `lo..=hi`.
    fn float(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / ((1u64 << 53) - 1) as f64
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.int(1, 100) <= percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next() as usize % items.len()]
    }
}

/// Explicit values for a few of `device`'s params, each in range.
fn some_params(rng: &mut Rng, device: &mut Device) {
    for spec in device.module.params {
        if rng.chance(25) {
            let local = spec.id.split_once('.').expect("prefixed id").1;
            device
                .params
                .insert(local.to_owned(), rng.float(spec.min, spec.max));
        }
    }
    for (i, setting) in device.module.settings.iter().enumerate() {
        device.settings[i] = rng.pick(setting.values);
    }
}

fn clip(rng: &mut Rng) -> Clip {
    let steps = rng.int(1, 64) as usize;
    if rng.chance(50) {
        const TOKENS: [&str; 7] = ["C2", "C2!", "C3~", ".", "D#2", "A#1!~", "G2"];
        let text: Vec<&str> = (0..steps).map(|_| *rng.pick(&TOKENS)).collect();
        Clip::Acid(AcidPattern::parse(&text.join(" ")).expect("valid acid steps"))
    } else {
        const HITS: [char; 3] = ['x', 'X', '.'];
        let text: String = (0..steps).map(|_| *rng.pick(&HITS)).collect();
        Clip::Drum(DrumPattern::parse(&text).expect("valid drum steps"))
    }
}

/// A random lane param on `track` (`<device id>.<param>` or a strip id).
fn track_param(rng: &mut Rng, track: &Track) -> String {
    let devices: Vec<&Device> = std::iter::once(&track.instrument)
        .chain(&track.fx)
        .collect();
    if rng.chance(20) {
        return rng.pick(STRIP_PARAMS).id.to_owned();
    }
    let device = rng.pick(&devices);
    let spec = rng.pick(device.module.params);
    device.param_id(spec.id.split_once('.').expect("prefixed id").1)
}

fn song(rng: &mut Rng, registry: &Registry) -> Song {
    let instruments: Vec<&'static ModuleDescriptor> = registry.instruments().collect();
    let effects: Vec<&'static ModuleDescriptor> = registry.effects().collect();

    let sections: Vec<Section> = (0..rng.int(1, 3))
        .map(|i| Section {
            name: format!("s{i}"),
            bars: rng.int(1, 3),
        })
        .collect();
    let total: u32 = sections.iter().map(|s| s.bars).sum();

    let mut clips = IndexMap::new();
    for i in 0..rng.int(1, 4) {
        clips.insert(format!("c{i}"), clip(rng));
    }

    let mut tracks = IndexMap::new();
    for i in 0..rng.int(1, 4) {
        let module = *rng.pick(&instruments);
        let mut instrument = Device::new(module.prefix, module);
        some_params(rng, &mut instrument);
        let mut track = Track {
            instrument,
            fx: Vec::new(),
            strip: IndexMap::new(),
            gain_db: rng.float(-24.0, 0.0),
            pan: rng.float(-1.0, 1.0),
            mute: rng.chance(10),
            solo: rng.chance(5),
            play: Vec::new(),
        };
        for _ in 0..rng.int(0, 4) {
            let module = *rng.pick(&effects);
            let mut device = Device::new(&track.next_device_id(module), module);
            some_params(rng, &mut device);
            track.fx.push(device);
        }
        if rng.chance(30) {
            track
                .strip
                .insert("send.delay".into(), rng.float(-60.0, 0.0));
        }
        if rng.chance(20) {
            track
                .strip
                .insert("send.reverb".into(), rng.float(-60.0, 0.0));
        }
        let playable: Vec<&String> = clips
            .iter()
            .filter(|(_, c)| module.plays(c.kind()))
            .map(|(id, _)| id)
            .collect();
        let mut bar = 0;
        while !playable.is_empty() && bar < total {
            bar += rng.int(0, 2);
            let bars = rng.int(1, 4);
            // Now and then a placement runs past the end and is refused.
            if bar + bars > total && !rng.chance(5) {
                break;
            }
            track.play.push(Placement {
                start_bar: bar,
                bars,
                clip: (*rng.pick(&playable)).clone(),
                offset_bars: rng.int(0, 8),
            });
            bar += bars;
        }
        tracks.insert(format!("t{i}"), track);
    }

    let mut automation = Vec::new();
    for _ in 0..rng.int(0, 4) {
        let (track, param) = if rng.chance(20) {
            let unit = *rng.pick(&MixUnit::ALL);
            (
                MIX_TRACK_ID.to_owned(),
                rng.pick(unit.params()).id.to_owned(),
            )
        } else {
            let ids: Vec<&String> = tracks.keys().collect();
            let id = (*rng.pick(&ids)).clone();
            let param = track_param(rng, &tracks[&id]);
            (id, param)
        };
        let start_bar = rng.int(0, total - 1);
        let bars = rng.int(1, total - start_bar);
        let mut offsets: Vec<f64> = (0..rng.int(1, 4))
            .map(|_| rng.float(0.0, f64::from(bars)))
            .collect();
        offsets.sort_by(f64::total_cmp);
        automation.push(Lane {
            track,
            param,
            start_bar,
            bars,
            points: offsets
                .into_iter()
                .map(|b| (b, rng.float(0.0, 1.0)))
                .collect(),
        });
    }

    Song {
        name: "fuzz".into(),
        bpm: rng.float(120.0, 300.0),
        seed: rng.next(),
        sample_rate: Some(*rng.pick(&[8_000, 16_000, 22_050])),
        tracks,
        clips,
        sections,
        automation,
        mix: Mix::default(),
    }
}

fn render_bars(song: &Arc<CompiledSong>, start_bar: u32) {
    let rendered = render(
        song.clone(),
        &RenderOptions {
            start_bar,
            end_bar: start_bar + 1,
            stems: true,
        },
    )
    .expect("a bar of the song renders");
    assert!(rendered.mix.iter().all(|x| x.is_finite()));
}

#[test]
fn compile_never_panics_on_valid_songs() {
    let registry = Registry::builtin();
    let mut rng = Rng(1);
    let mut accepted = 0;
    let mut kinds = [0usize; 2];
    for _ in 0..SONGS {
        let text = song(&mut rng, registry).to_toml();
        let Ok(song) = parse_song(&text) else {
            continue;
        };
        accepted += 1;
        for c in song.clips.values() {
            kinds[usize::from(c.kind() == ClipKind::Drum)] += 1;
        }
        let compiled = Arc::new(CompiledSong::new(&song, song.sample_rate.unwrap()));
        render_bars(&compiled, 0);
        render_bars(&compiled, compiled.total_bars - 1);
    }
    // The generator mostly stays valid, so the engine sees most songs.
    assert!(
        accepted >= SONGS / 2,
        "only {accepted} of {SONGS} songs parsed"
    );
    assert!(kinds.iter().all(|&n| n > 0));
}
