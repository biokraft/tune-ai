mod common;

use common::{concat, mono, sine, stereo};
use tune_analysis::spectrogram::{
    BOUNDARY, PANEL_HEIGHT, WIDTH, compare_png, freq_to_y, label_step, spectrogram_png, x_of_frame,
};
use tune_analysis::{Audio, BarMap, SectionSpan};

const SR: u32 = 48_000;
const BAR: usize = 96_000;

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    (info.width, info.height, buf)
}

fn px(img: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * WIDTH + x) * 3) as usize;
    [img[i], img[i + 1], img[i + 2]]
}

fn luma(p: [u8; 3]) -> u32 {
    u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114
}

fn map4() -> BarMap {
    BarMap {
        first_bar: 0,
        bar_frames: (0..=4).map(|i| i * BAR).collect(),
        sections: vec![
            SectionSpan {
                name: "intro".into(),
                start_bar: 0,
                bars: 2,
            },
            SectionSpan {
                name: "drop".into(),
                start_bar: 2,
                bars: 2,
            },
        ],
    }
}

fn tone(dbfs: f64) -> Audio {
    mono(SR, sine(SR, 1000.0, dbfs, 10.0, 0.0))
}

#[test]
fn png_size_and_determinism() {
    let a = spectrogram_png(&tone(-12.0), "tone", None);
    let b = spectrogram_png(&tone(-12.0), "tone", None);
    assert_eq!(a, b);
    let (w, h, _) = decode(&a);
    assert_eq!((w, h), (WIDTH, PANEL_HEIGHT));
    assert_eq!((w, h), (1600, 816));
}

#[test]
fn sine_brightest_at_its_row() {
    let img = decode(&spectrogram_png(&tone(-30.0), "tone", None)).2;
    let target = freq_to_y(1000.0);
    let best = (48..688u32)
        .max_by_key(|&y| (luma(px(&img, 800, y)), u32::MAX - y))
        .unwrap();
    assert!(
        best.abs_diff(target) <= 3,
        "brightest row {best}, want {target}"
    );
}

#[test]
fn louder_is_brighter() {
    let loud = decode(&spectrogram_png(&tone(-10.0), "a", None)).2;
    let quiet = decode(&spectrogram_png(&tone(-40.0), "a", None)).2;
    let y = freq_to_y(1000.0);
    let best = |img: &[u8]| {
        (y - 3..=y + 3)
            .map(|r| luma(px(img, 800, r)))
            .max()
            .unwrap()
    };
    assert!(
        best(&loud) > best(&quiet),
        "{} vs {}",
        best(&loud),
        best(&quiet)
    );
}

#[test]
fn section_boundary_drawn() {
    let audio = stereo(
        SR,
        sine(SR, 1000.0, -20.0, 8.0, 0.0),
        sine(SR, 1000.0, -20.0, 8.0, 0.0),
    );
    let map = map4();
    let img = decode(&spectrogram_png(&audio, "song", Some(&map))).2;
    let x = x_of_frame(map.bar_frames[2], audio.frames());
    assert_eq!(px(&img, x, 60), BOUNDARY);
    assert_eq!(px(&img, x + 1, 60), BOUNDARY);
    assert_eq!(px(&img, x, 750), BOUNDARY);
}

#[test]
fn label_step_values() {
    assert_eq!(label_step(60.0), 1);
    assert_eq!(label_step(30.0), 2);
    assert_eq!(label_step(10.5), 8);
    assert_eq!(label_step(1.0), 64);
}

#[test]
fn compare_png_stacks_two_panels() {
    let a = tone(-12.0);
    let b = tone(-30.0);
    let bytes = compare_png((&a, "ours", None), (&b, "ref"));
    let (w, h, _) = decode(&bytes);
    assert_eq!((w, h), (1600, 1632));
}

/// Writes `<workspace>/scratch/sweep.png` for a human (or Claude) to read.
#[test]
#[ignore]
fn dump_sweep_png() {
    let a = sine(SR, 500.0, -12.0, 4.0, 0.0);
    let b = sine(SR, 4000.0, -12.0, 4.0, 0.0);
    let audio = concat(&mono(SR, a), &mono(SR, b));
    let mut map = map4();
    map.bar_frames = (0..=4).map(|i| i * 96_000).collect();
    let png = spectrogram_png(&audio, "sweep 500 Hz to 4 kHz", Some(&map));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sweep.png"), png).unwrap();
}
