//! Audio output: the default device through `cpal`, or a `null` device that
//! runs the processor on a plain thread paced at real time.
//!
//! The audio callback only calls [`LiveProcessor::process`] on preallocated
//! buffers: no allocation, locks or printing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tune_engine::LiveProcessor;

/// Frames per block on the null device.
const NULL_BLOCK: usize = 512;

/// Largest chunk of frames the callback renders in one `process` call when the
/// device is not stereo; the scratch buffer is sized for it.
const SCRATCH_FRAMES: usize = 16_384;

/// A device and the format picked for it, before a stream exists. The song
/// must be compiled at [`Output::sample_rate`] to play on it.
pub struct Output {
    device: cpal::Device,
    config: cpal::StreamConfig,
    /// A note for the user when the song's rate was not available.
    pub note: Option<String>,
}

impl Output {
    /// Picks the default output device. Asks for `wanted` Hz and falls back to
    /// the device default rate.
    pub fn default_device(wanted: u32) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let default = device
            .default_output_config()
            .map_err(|e| format!("cannot read the audio device format: {e}"))?;
        let channels = default.channels();
        let mut config = default.config();
        let mut note = None;
        if default.sample_rate() != wanted {
            let supported = device
                .supported_output_configs()
                .map(|mut ranges| {
                    ranges.any(|r| {
                        r.channels() == channels
                            && r.sample_format() == cpal::SampleFormat::F32
                            && r.contains_rate(wanted)
                    })
                })
                .unwrap_or(false);
            if supported {
                config.sample_rate = wanted;
            } else {
                note = Some(format!(
                    "note: playing at {} Hz (song asks {wanted})",
                    default.sample_rate()
                ));
            }
        }
        Ok(Self {
            device,
            config,
            note,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// Opens the stream and starts it. Dropping the result stops playback.
    pub fn start(self, mut processor: LiveProcessor) -> Result<Running, String> {
        let channels = usize::from(self.config.channels);
        let mut scratch = vec![0.0_f32; SCRATCH_FRAMES * 2];
        let stream = self
            .device
            .build_output_stream(
                self.config,
                move |data: &mut [f32], _| fill(&mut processor, &mut scratch, channels, data),
                |_| {},
                None,
            )
            .map_err(|e| format!("cannot open the audio device: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("cannot start the audio device: {e}"))?;
        Ok(Running::Device(stream))
    }
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

/// Starts the null device: runs `processor` on a thread paced at real time.
pub fn start_null(mut processor: LiveProcessor) -> Running {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let rate = f64::from(processor.sample_rate());
    let thread = std::thread::Builder::new()
        .name("null-audio".into())
        .spawn(move || {
            let mut block = vec![0.0_f32; NULL_BLOCK * 2];
            let start = Instant::now();
            let mut n: u64 = 0;
            while !flag.load(Ordering::Relaxed) {
                processor.process(&mut block);
                n += 1;
                let due = Duration::from_secs_f64((n * NULL_BLOCK as u64) as f64 / rate);
                let now = start.elapsed();
                if due > now {
                    std::thread::sleep(due - now);
                }
            }
        })
        .expect("spawn null audio thread");
    Running::Null {
        stop,
        thread: Some(thread),
    }
}

/// A running output. Dropping it stops playback.
pub enum Running {
    Device(cpal::Stream),
    Null {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    },
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Self::Null { stop, thread } = self {
            stop.store(true, Ordering::Relaxed);
            if let Some(t) = thread.take() {
                let _ = t.join();
            }
        }
    }
}
