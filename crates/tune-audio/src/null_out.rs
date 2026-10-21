//! The null device: no sound, real-time pacing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tune_engine::{AudioOut, LiveProcessor};

/// Frames per block on the null device.
const NULL_BLOCK: usize = 512;

#[derive(Default)]
pub struct NullOut;

impl NullOut {
    pub fn new() -> Self {
        Self
    }
}

impl AudioOut for NullOut {
    fn sample_rate(&mut self, wanted: u32) -> Result<u32, String> {
        Ok(wanted)
    }

    fn start(&mut self, mut processor: LiveProcessor) -> Result<Box<dyn Send>, String> {
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
            .map_err(|e| format!("cannot start the null audio thread: {e}"))?;
        Ok(Box::new(NullHandle {
            stop,
            thread: Some(thread),
        }))
    }
}

/// Dropping it stops the thread and waits for it.
struct NullHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for NullHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
