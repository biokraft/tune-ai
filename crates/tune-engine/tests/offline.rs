mod common;

use common::{MINI, compile_song, compiled, song};
use tune_engine::{CLIP_CEILING, Engine, RenderError, RenderOptions, render, render_to_files};

const FULL_FRAMES: usize = 384_000;

fn full() -> RenderOptions {
    RenderOptions {
        start_bar: 0,
        end_bar: 4,
        stems: false,
    }
}

fn read_spec(path: &std::path::Path) -> (hound::WavSpec, u32) {
    let r = hound::WavReader::open(path).unwrap();
    (r.spec(), r.duration())
}

#[test]
fn full_render_length() {
    let r = render(compiled(MINI), &full()).unwrap();
    assert_eq!(r.sample_rate, 48_000);
    assert_eq!(r.mix.len(), 2 * FULL_FRAMES);
    assert!(r.stems.is_empty());
    assert!(r.mix.iter().any(|&x| x != 0.0));
}

#[test]
fn bar_range_length() {
    let opts = RenderOptions {
        start_bar: 1,
        end_bar: 3,
        stems: false,
    };
    let r = render(compiled(MINI), &opts).unwrap();
    assert_eq!(r.mix.len(), 2 * 192_000);
}

#[test]
fn stems_named_in_song_order_with_mix_length() {
    let opts = RenderOptions {
        stems: true,
        ..full()
    };
    let r = render(compiled(MINI), &opts).unwrap();
    let names: Vec<&str> = r.stems.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["kick", "acid", "hat"]);
    for (_, s) in &r.stems {
        assert_eq!(s.len(), r.mix.len());
    }
}

#[test]
fn muted_track_still_has_a_stem_and_leaves_the_mix() {
    let mut muted = song(MINI);
    muted.tracks.get_mut("kick").unwrap().mute = true;
    let opts = RenderOptions {
        stems: true,
        ..full()
    };
    let r = render(compile_song(&muted), &opts).unwrap();
    let kick = &r.stems.iter().find(|(n, _)| n == "kick").unwrap().1;
    assert!(kick.iter().any(|&x| x != 0.0));

    let mut without = song(MINI);
    without.tracks.shift_remove("kick");
    without.clips.shift_remove("k4");
    let other = render(compile_song(&without), &full()).unwrap();
    assert_eq!(r.mix, other.mix);
}

#[test]
fn render_is_deterministic() {
    let opts = RenderOptions {
        stems: true,
        ..full()
    };
    let a = render(compiled(MINI), &opts).unwrap();
    let b = render(compiled(MINI), &opts).unwrap();
    assert_eq!(a.mix, b.mix);
    assert_eq!(a.stems, b.stems);
}

#[test]
fn block_size_independent() {
    let s = compiled(MINI);
    let reference = render(s.clone(), &full()).unwrap();
    let mut engine = Engine::new(s);
    let mut out = vec![0.0f32; 2 * FULL_FRAMES];
    let sizes = [1usize, 7, 64, 333, 4096];
    let (mut pos, mut i) = (0, 0);
    while pos < FULL_FRAMES {
        let n = sizes[i % sizes.len()].min(FULL_FRAMES - pos);
        engine.process(&mut out[2 * pos..2 * (pos + n)]);
        pos += n;
        i += 1;
    }
    assert_eq!(out, reference.mix);
    let m = engine.meter();
    assert_eq!(m.peak, reference.meter.peak);
    assert_eq!(m.pre_clip_peak, reference.meter.pre_clip_peak);
    assert_eq!(m.knee_samples, reference.meter.knee_samples);
}

#[test]
fn hot_mix_is_clipped_not_broken() {
    let mut hot = song(MINI);
    for t in hot.tracks.values_mut() {
        t.gain_db = 12.0;
    }
    let r = render(compile_song(&hot), &full()).unwrap();
    assert!(r.mix.iter().all(|x| x.is_finite()));
    assert!(r.meter.pre_clip_peak > 1.0, "{:?}", r.meter);
    assert!(r.meter.peak <= CLIP_CEILING, "{:?}", r.meter);
}

#[test]
fn bad_range() {
    for (start, end) in [(3, 2), (0, 5), (2, 2)] {
        let opts = RenderOptions {
            start_bar: start,
            end_bar: end,
            stems: false,
        };
        match render(compiled(MINI), &opts) {
            Err(RenderError::BadRange {
                start: s,
                end: e,
                total,
            }) => {
                assert_eq!((s, e, total), (start, end, 4));
            }
            other => panic!("{start}..{end}: {other:?}"),
        }
    }
}

#[test]
fn files_written() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("sub/mix.wav");
    let report = render_to_files(compiled(MINI), &full(), &out).unwrap();
    assert_eq!(report.files, vec![out.clone()]);
    assert_eq!(report.frames, FULL_FRAMES as u64);
    assert_eq!(report.seconds, 8.0);
    let (spec, frames) = read_spec(&out);
    assert_eq!(
        (
            spec.channels,
            spec.bits_per_sample,
            spec.sample_rate,
            frames
        ),
        (2, 24, 48_000, FULL_FRAMES as u32)
    );
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    assert!(!out.with_file_name("mix.wav.tmp").exists());

    let stems_dir = dir.path().join("stems");
    let opts = RenderOptions {
        stems: true,
        ..full()
    };
    let report = render_to_files(compiled(MINI), &opts, &stems_dir).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&stems_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["acid.wav", "hat.wav", "kick.wav", "mix.wav"]);
    assert_eq!(report.files.len(), 4);
    let (mix, _) = read_spec(&stems_dir.join("mix.wav"));
    assert_eq!(
        (mix.bits_per_sample, mix.sample_format),
        (24, hound::SampleFormat::Int)
    );
    for n in ["kick", "acid", "hat"] {
        let (spec, frames) = read_spec(&stems_dir.join(format!("{n}.wav")));
        assert_eq!(spec.sample_format, hound::SampleFormat::Float, "{n}");
        assert_eq!(
            (spec.bits_per_sample, spec.channels, frames),
            (32, 2, FULL_FRAMES as u32)
        );
    }
}

#[test]
fn streamed_files_match_in_memory_render() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("mix.wav");
    render_to_files(compiled(MINI), &full(), &out).unwrap();
    let mem = render(compiled(MINI), &full()).unwrap().mix;
    let got: Vec<i32> = hound::WavReader::open(&out)
        .unwrap()
        .samples::<i32>()
        .map(Result::unwrap)
        .collect();
    assert_eq!(got.len(), mem.len());
    for (g, m) in got.iter().zip(&mem) {
        let q = (f64::from(*m) * 8_388_607.0).round() as i32;
        assert_eq!(*g, q);
    }
}

#[test]
fn no_tmp_on_error() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, b"x").unwrap();
    let out = blocker.join("mix.wav");
    assert!(render_to_files(compiled(MINI), &full(), &out).is_err());
    let opts = RenderOptions {
        stems: true,
        ..full()
    };
    assert!(render_to_files(compiled(MINI), &opts, &blocker).is_err());
    for e in std::fs::read_dir(dir.path()).unwrap() {
        let name = e.unwrap().file_name().into_string().unwrap();
        assert!(!name.ends_with(".tmp"), "{name}");
    }
}

#[test]
fn no_tmp_when_rename_fails() {
    use tune_engine::wav::{WavFormat, write_wav};
    let dir = tempfile::tempdir().unwrap();
    // A directory at the target makes the final rename fail.
    let target = dir.path().join("out.wav");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep"), b"x").unwrap();
    let r = write_wav(&target, &[0.0; 8], 2, 48_000, WavFormat::Pcm24);
    assert!(r.is_err());
    assert!(!dir.path().join("out.wav.tmp").exists());
}
