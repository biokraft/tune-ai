//! The null device runs at real-time pace. No audio plays.

use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tune_audio::{Device, NullOut};
use tune_engine::{AudioOut, CompiledSong, live};

fn processor() -> (tune_engine::LiveController, tune_engine::LiveProcessor) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tune-song/tests/fixtures/mini.toml");
    let src = std::fs::read_to_string(path).unwrap();
    let song = tune_song::parse_song(&src).unwrap();
    live(Arc::new(CompiledSong::new(&song, 48_000)), 0, None)
}

#[test]
fn null_out_runs_in_real_time() {
    let (controller, processor) = processor();
    let mut out = NullOut::new();
    let _handle = out.start(processor).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let pos = controller.position();
    assert!((19_200..=28_800).contains(&pos), "position {pos}");
}

#[test]
fn drop_stops_output() {
    let (controller, processor) = processor();
    let mut out = NullOut::new();
    let handle = out.start(processor).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    drop(handle);
    let before = controller.position();
    std::thread::sleep(Duration::from_millis(200));
    assert!(controller.position() - before < 960);
}

#[test]
fn null_rate_echoes() {
    assert_eq!(NullOut::new().sample_rate(44_100), Ok(44_100));
}

#[test]
fn null_never_loses_device() {
    assert!(!NullOut::new().device_lost());
}

#[test]
fn device_from_str() {
    assert_eq!(Device::from_str("null"), Ok(Device::Null));
    assert_eq!(Device::from_str("default"), Ok(Device::Default));
    assert!(
        Device::from_str("usb")
            .unwrap_err()
            .contains("unknown device")
    );
}
