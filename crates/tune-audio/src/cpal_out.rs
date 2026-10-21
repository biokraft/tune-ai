//! The default output device through `cpal`.
//!
//! The device is opened at its current default rate: asking for another
//! rate would change the device's nominal rate system-wide. The song is
//! compiled at the rate [`AudioOut::sample_rate`] returns.
//!
//! cpal's error callback reports glitches as well as a gone device: on macOS
//! an overload comes as `Xrun` and a switch of the default output (the
//! stream follows it and keeps playing) as `DeviceChanged`. Only a device
//! that is gone ends playback; the rest is noted on stderr. The callback may
//! run on CoreAudio's realtime thread, so it only sets flags; the stream's
//! own thread prints.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tune_engine::{AudioOut, LiveProcessor};

/// Largest chunk of frames the callback renders in one `process` call when
/// the device is not stereo; the scratch buffer is sized for it.
const SCRATCH_FRAMES: usize = 16_384;
/// How often the stream's thread looks at the error flags.
const POLL: Duration = Duration::from_millis(100);

const LOST_LINE: &str = "audio device error: the output device is gone";
const XRUN_NOTE: &str = "note: the audio device glitched (buffer underrun); playback continues";
const REROUTE_NOTE: &str = "note: the default audio output changed; playback moved to it";

/// What one stream's error callback saw. Set from any thread, the realtime
/// one included; read by the stream's thread.
#[derive(Default)]
struct StreamFlags {
    lost: AtomicBool,
    glitch: AtomicBool,
    rerouted: AtomicBool,
}

/// Whether `e` means the output device is gone, so playback has ended.
fn is_device_lost(e: &cpal::Error) -> bool {
    matches!(
        e.kind(),
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::StreamInvalidated
    )
}

/// The error callback: flags only, no printing or allocation.
fn on_error(flags: &StreamFlags, e: &cpal::Error) {
    let flag = if is_device_lost(e) {
        &flags.lost
    } else if e.kind() == cpal::ErrorKind::DeviceChanged {
        &flags.rerouted
    } else {
        // An xrun, or a backend error inside one callback: a glitch.
        &flags.glitch
    };
    flag.store(true, Ordering::Relaxed);
}

/// What the stream's thread has printed so far.
#[derive(Default)]
struct Reported {
    lost: bool,
    glitch: bool,
}

impl Reported {
    /// The stderr lines the flags call for now: a gone device and a glitch
    /// once per stream, each reroute as it happens.
    fn take_notes(&mut self, flags: &StreamFlags) -> Vec<&'static str> {
        let mut lines = Vec::new();
        if flags.lost.load(Ordering::Relaxed) && !self.lost {
            self.lost = true;
            lines.push(LOST_LINE);
        }
        if flags.glitch.swap(false, Ordering::Relaxed) && !self.glitch {
            self.glitch = true;
            lines.push(XRUN_NOTE);
        }
        if flags.rerouted.swap(false, Ordering::Relaxed) {
            lines.push(REROUTE_NOTE);
        }
        lines
    }
}

pub struct CpalOut {
    /// The default config, read by `sample_rate`.
    config: Option<cpal::StreamConfig>,
    /// The current stream's flags; each `start` makes new ones.
    flags: Arc<StreamFlags>,
}

impl Default for CpalOut {
    fn default() -> Self {
        Self::new()
    }
}

impl CpalOut {
    pub fn new() -> Self {
        Self {
            config: None,
            flags: Arc::default(),
        }
    }

    /// Fresh flags for a new stream: a loss reported by an earlier stream
    /// (or late, by one being torn down) does not end this one.
    fn arm(&mut self) -> Arc<StreamFlags> {
        self.flags = Arc::default();
        self.flags.clone()
    }
}

impl AudioOut for CpalOut {
    fn sample_rate(&mut self, _wanted: u32) -> Result<u32, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no default audio output device")?;
        let default = device
            .default_output_config()
            .map_err(|e| format!("cannot read the audio device format: {e}"))?;
        if default.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "the audio device wants {:?} samples; only f32 is supported",
                default.sample_format()
            ));
        }
        let config = default.config();
        let rate = config.sample_rate;
        self.config = Some(config);
        Ok(rate)
    }

    fn start(&mut self, processor: LiveProcessor) -> Result<Box<dyn Send>, String> {
        let config = self
            .config
            .ok_or("sample_rate must be called before start")?;
        let flags = self.arm();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        // A `cpal::Stream` may not be `Send`: it lives and dies on this thread.
        let thread = std::thread::Builder::new()
            .name("cpal-audio".into())
            .spawn(move || {
                let stream = match open(config, processor, flags.clone()) {
                    Ok(s) => s,
                    Err(m) => {
                        let _ = ready_tx.send(Err(m));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                let mut reported = Reported::default();
                // Runs until the handle drops its sender.
                while let Err(mpsc::RecvTimeoutError::Timeout) = stop_rx.recv_timeout(POLL) {
                    for line in reported.take_notes(&flags) {
                        eprintln!("{line}");
                    }
                }
                drop(stream);
            })
            .map_err(|e| format!("cannot start the audio thread: {e}"))?;
        let handle = CpalHandle {
            stop: Some(stop_tx),
            thread: Some(thread),
        };
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(handle)),
            Ok(Err(m)) => Err(m),
            Err(_) => Err("the audio thread died while opening the device".into()),
        }
    }

    fn device_lost(&self) -> bool {
        self.flags.lost.load(Ordering::Relaxed)
    }
}

fn open(
    config: cpal::StreamConfig,
    mut processor: LiveProcessor,
    flags: Arc<StreamFlags>,
) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no default audio output device")?;
    let channels = usize::from(config.channels);
    let mut scratch = vec![0.0_f32; SCRATCH_FRAMES * 2];
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _| fill(&mut processor, &mut scratch, channels, data),
            move |e| on_error(&flags, &e),
            None,
        )
        .map_err(|e| format!("cannot open the audio device: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("cannot start the audio device: {e}"))?;
    Ok(stream)
}

/// Renders one callback of interleaved `channels`-wide frames. Stereo goes
/// straight through; other widths get L/R on channels 0 and 1 (just L on a
/// mono device) and silence on the rest.
fn fill(processor: &mut LiveProcessor, scratch: &mut [f32], channels: usize, data: &mut [f32]) {
    if channels == 2 {
        processor.process(data);
        return;
    }
    if channels == 0 {
        return;
    }
    let max = scratch.len() / 2;
    for chunk in data.chunks_mut(max * channels) {
        let frames = chunk.len() / channels;
        processor.process(&mut scratch[..frames * 2]);
        for (frame, lr) in chunk
            .chunks_exact_mut(channels)
            .zip(scratch.chunks_exact(2))
        {
            for (c, out) in frame.iter_mut().enumerate() {
                *out = if c < 2 { lr[c] } else { 0.0 };
            }
        }
    }
}

/// Dropping it signals the audio thread and waits for it.
struct CpalHandle {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for CpalHandle {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::ErrorKind;

    fn err(kind: ErrorKind) -> cpal::Error {
        cpal::Error::new(kind)
    }

    #[test]
    fn only_a_gone_device_counts_as_lost() {
        assert!(is_device_lost(&err(ErrorKind::DeviceNotAvailable)));
        assert!(is_device_lost(&err(ErrorKind::StreamInvalidated)));
        for kind in [
            ErrorKind::Xrun,
            ErrorKind::DeviceChanged,
            ErrorKind::BackendError,
            ErrorKind::RealtimeDenied,
            ErrorKind::Other,
        ] {
            assert!(!is_device_lost(&err(kind)), "{kind:?}");
        }
    }

    #[test]
    fn an_xrun_or_a_reroute_keeps_playing() {
        let flags = StreamFlags::default();
        on_error(&flags, &err(ErrorKind::Xrun));
        on_error(&flags, &err(ErrorKind::DeviceChanged));
        assert!(!flags.lost.load(Ordering::Relaxed));
        assert!(flags.glitch.load(Ordering::Relaxed));
        assert!(flags.rerouted.load(Ordering::Relaxed));
        on_error(&flags, &err(ErrorKind::DeviceNotAvailable));
        assert!(flags.lost.load(Ordering::Relaxed));
    }

    #[test]
    fn xruns_are_reported_once_per_stream() {
        let flags = StreamFlags::default();
        let mut reported = Reported::default();
        assert_eq!(reported.take_notes(&flags), Vec::<&str>::new());
        on_error(&flags, &err(ErrorKind::Xrun));
        assert_eq!(reported.take_notes(&flags), vec![XRUN_NOTE]);
        on_error(&flags, &err(ErrorKind::Xrun));
        assert_eq!(reported.take_notes(&flags), Vec::<&str>::new());
        on_error(&flags, &err(ErrorKind::DeviceChanged));
        assert_eq!(reported.take_notes(&flags), vec![REROUTE_NOTE]);
    }

    #[test]
    fn each_start_begins_with_a_fresh_flag() {
        let mut out = CpalOut::new();
        let old = out.arm();
        old.lost.store(true, Ordering::Relaxed);
        assert!(out.device_lost());
        let new = out.arm();
        assert!(!out.device_lost());
        // A late error from the old stream does not reach the new one.
        on_error(&old, &err(ErrorKind::DeviceNotAvailable));
        assert!(!out.device_lost());
        assert!(!new.lost.load(Ordering::Relaxed));
    }
}
