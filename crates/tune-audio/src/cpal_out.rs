//! The default output device through `cpal`.
//!
//! The device is opened at its current default rate: asking for another
//! rate would change the device's nominal rate system-wide. The song is
//! compiled at the rate [`AudioOut::sample_rate`] returns.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tune_engine::{AudioOut, LiveProcessor};

/// Largest chunk of frames the callback renders in one `process` call when
/// the device is not stereo; the scratch buffer is sized for it.
const SCRATCH_FRAMES: usize = 16_384;

pub struct CpalOut {
    /// The default config, read by `sample_rate`.
    config: Option<cpal::StreamConfig>,
    lost: Arc<AtomicBool>,
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
            lost: Arc::new(AtomicBool::new(false)),
        }
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
        let lost = self.lost.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        // A `cpal::Stream` may not be `Send`: it lives and dies on this thread.
        let thread = std::thread::Builder::new()
            .name("cpal-audio".into())
            .spawn(move || {
                let stream = match open(config, processor, lost) {
                    Ok(s) => s,
                    Err(m) => {
                        let _ = ready_tx.send(Err(m));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                // Blocks until the handle drops its sender.
                let _ = stop_rx.recv();
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
        self.lost.load(Ordering::Relaxed)
    }
}

fn open(
    config: cpal::StreamConfig,
    mut processor: LiveProcessor,
    lost: Arc<AtomicBool>,
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
            // Not the realtime callback: printing is fine here.
            move |e| {
                if !lost.swap(true, Ordering::Relaxed) {
                    eprintln!("audio device error: {e}");
                }
            },
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
