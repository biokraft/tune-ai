//! Realtime playback: a [`LiveProcessor`] for the audio callback, steered by
//! a [`LiveController`] on another thread.
//!
//! The two halves talk through two lock-free SPSC queues ([`rtrb`]):
//!
//! - Commands (controller to processor): swap, seek, loop, knob override.
//!   The processor drains them at the start of each
//!   [`LiveProcessor::process`] call, so a seek, loop or override change
//!   takes effect at the next sample. A swap is handed to the [`Engine`],
//!   which applies it at the next step boundary.
//! - Garbage (processor to controller): every [`SongUpdate`] comes back in
//!   the same box, now holding the old song and every runtime the new song
//!   did not take, so nothing is freed on the audio thread.
//!   [`LiveController::collect_garbage`] drops them.
//!
//! A swap is popped from the command queue only when the garbage queue has
//! room for everything it will send back; otherwise it waits in the queue
//! (and the commands behind it with it) until a later block. A swap that
//! arrives while another is still pending replaces it, and the replaced
//! update goes straight back as garbage.
//!
//! Every swap the controller sends carries a generation, one higher than the
//! last. Every override carries the generation of the last swap sent, and the
//! processor drops it unless the engine is playing that generation, because
//! its track index may belong to another song.
//!
//! The processor publishes position, bar, finished, the number of applied
//! swaps, the playing generation and the number of overrides set through
//! atomics after each call.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::compile::{CompiledSong, ParamTarget};
use crate::engine::{Engine, TrackRuntime};
use crate::mixer::MixRuntime;

/// Capacity of each queue.
const QUEUE: usize = 16;

/// A song ready to be swapped in, with fresh runtimes built off the audio
/// thread. After the swap the same box returns as garbage holding what the
/// engine let go of.
pub struct SongUpdate {
    /// The new song; after the swap, the old one.
    pub(crate) song: Arc<CompiledSong>,
    /// One fresh runtime per new track; after the swap, whatever the engine
    /// did not use (fresh runtimes, or the half of a kept runtime it replaced).
    pub(crate) tracks: Vec<Option<TrackRuntime>>,
    /// Empty with room for one runtime per new track; becomes the engine's
    /// runtime list. After the swap it holds the old list.
    pub(crate) spare: Vec<TrackRuntime>,
    /// A fresh mix runtime for the new song; after the swap, the one the
    /// engine did not keep (this one, or the old song's).
    pub(crate) mix: MixRuntime,
    /// Assigned by the controller, one higher per swap it sends; the
    /// engine plays this generation once the update lands.
    pub(crate) generation: u64,
    /// Whether landing clears every knob override.
    pub(crate) clear_overrides: bool,
}

impl SongUpdate {
    /// Builds fresh runtimes for every track of `song` and for its mix
    /// (buses sized for its bpm). Allocates: call it off the audio thread.
    pub fn new(song: Arc<CompiledSong>) -> Box<Self> {
        let sample_rate = f64::from(song.clock.sample_rate);
        let tracks = song
            .tracks
            .iter()
            .map(|t| Some(TrackRuntime::build(t, sample_rate)))
            .collect();
        let spare = Vec::with_capacity(song.tracks.len());
        let mix = MixRuntime::build(&song);
        Box::new(Self {
            song,
            tracks,
            spare,
            mix,
            generation: 0,
            clear_overrides: false,
        })
    }

    pub fn song(&self) -> &Arc<CompiledSong> {
        &self.song
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LiveError {
    #[error("the command queue is full")]
    QueueFull,
    #[error("the song runs at {song} Hz but the engine at {engine} Hz")]
    SampleRateMismatch { song: u32, engine: u32 },
}

enum Command {
    Swap(Box<SongUpdate>),
    Seek(u32),
    Loop(Option<Range<u32>>),
    Override {
        /// The generation the controller had sent last: the song whose
        /// track indices `track` and `target` refer to.
        generation: u64,
        track: usize,
        target: ParamTarget,
        plain: Option<f64>,
    },
}

/// What the processor publishes after each block.
#[derive(Default)]
struct Shared {
    position: AtomicU64,
    bar: AtomicU32,
    finished: AtomicBool,
    swaps_applied: AtomicU64,
    generation: AtomicU64,
    overrides_active: AtomicU32,
}

impl Shared {
    fn publish(&self, engine: &Engine) {
        self.position.store(engine.position(), Ordering::Relaxed);
        self.bar.store(engine.bar(), Ordering::Relaxed);
        self.finished.store(engine.finished(), Ordering::Relaxed);
        self.swaps_applied
            .store(engine.swaps_applied(), Ordering::Release);
        self.generation
            .store(engine.generation(), Ordering::Release);
        self.overrides_active
            .store(engine.overrides_active(), Ordering::Relaxed);
    }
}

/// Starts live playback of `song` at 0-based `start_bar`, looping the
/// half-open bar range `looping` (see [`Engine::set_loop`]). Allocates: call
/// it off the audio thread, then move the processor into the callback.
pub fn live(
    song: Arc<CompiledSong>,
    start_bar: u32,
    looping: Option<Range<u32>>,
) -> (LiveController, LiveProcessor) {
    let sample_rate = song.clock.sample_rate;
    let mut engine = Engine::new(song);
    engine.seek(start_bar);
    engine.set_loop(looping);
    let shared = Arc::new(Shared::default());
    shared.publish(&engine);
    let (commands_tx, commands_rx) = RingBuffer::new(QUEUE);
    let (garbage_tx, garbage_rx) = RingBuffer::new(QUEUE);
    (
        LiveController {
            commands: commands_tx,
            garbage: garbage_rx,
            shared: shared.clone(),
            sample_rate,
            generation: 0,
        },
        LiveProcessor {
            engine,
            commands: commands_rx,
            garbage: garbage_tx,
            shared,
            sample_rate,
        },
    )
}

/// The non-realtime half: sends commands and collects garbage.
pub struct LiveController {
    commands: Producer<Command>,
    garbage: Consumer<Box<SongUpdate>>,
    shared: Arc<Shared>,
    sample_rate: u32,
    /// The generation of the last swap sent (0 before any).
    generation: u64,
}

impl LiveController {
    /// Swaps in `song` at the next step boundary, keeping the playhead and
    /// the knob overrides of every kept runtime (see [`Engine::queue_swap`]).
    /// Builds the update here, off the audio thread.
    pub fn swap(&mut self, song: Arc<CompiledSong>) -> Result<(), LiveError> {
        self.send_swap(song, false)
    }

    /// Like [`swap`](Self::swap), but every knob override is cleared when
    /// the swap lands (for instance once a knob's value is in the song).
    pub fn swap_clearing_overrides(&mut self, song: Arc<CompiledSong>) -> Result<(), LiveError> {
        self.send_swap(song, true)
    }

    /// Overrides param `target` of track `track` (an index into the song of
    /// the last swap sent, or the first song) with `plain`, or clears it
    /// with `None` (see [`Engine::set_override`]). Takes effect at the next
    /// block. The processor drops it if that song has not landed yet, since
    /// the index may name another track in the song still playing; send it
    /// again (the next pointer move does).
    pub fn set_override(
        &mut self,
        track: usize,
        target: ParamTarget,
        plain: Option<f64>,
    ) -> Result<(), LiveError> {
        self.send(Command::Override {
            generation: self.generation,
            track,
            target,
            plain,
        })
    }

    fn send_swap(
        &mut self,
        song: Arc<CompiledSong>,
        clear_overrides: bool,
    ) -> Result<(), LiveError> {
        let rate = song.clock.sample_rate;
        if rate != self.sample_rate {
            return Err(LiveError::SampleRateMismatch {
                song: rate,
                engine: self.sample_rate,
            });
        }
        if self.commands.is_full() {
            return Err(LiveError::QueueFull);
        }
        let mut update = SongUpdate::new(song);
        update.generation = self.generation + 1;
        update.clear_overrides = clear_overrides;
        self.send(Command::Swap(update))?;
        // Only a sent swap counts: a rejected one never lands.
        self.generation += 1;
        Ok(())
    }

    /// Jumps to the start of 0-based `bar` (see [`Engine::seek`]).
    pub fn seek(&mut self, bar: u32) -> Result<(), LiveError> {
        self.send(Command::Seek(bar))
    }

    /// Sets or clears the loop (see [`Engine::set_loop`]).
    pub fn set_loop(&mut self, bars: Option<Range<u32>>) -> Result<(), LiveError> {
        self.send(Command::Loop(bars))
    }

    fn send(&mut self, command: Command) -> Result<(), LiveError> {
        // A rejected command (and the update inside it) is dropped here, on
        // the controller thread.
        self.commands
            .push(command)
            .map_err(|_| LiveError::QueueFull)
    }

    /// Song sample position of the next sample, as of the last block.
    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Relaxed)
    }

    /// 0-based bar of the playhead (`total_bars` at the end).
    pub fn bar(&self) -> u32 {
        self.shared.bar.load(Ordering::Relaxed)
    }

    pub fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }

    /// Swaps applied so far (a swap replaced before its boundary is not one).
    pub fn swaps_applied(&self) -> u64 {
        self.shared.swaps_applied.load(Ordering::Acquire)
    }

    /// The generation of the song playing, as of the last block: 0 for the
    /// first song, then that of each swap as it lands (a replaced swap never
    /// plays, so its generation is skipped).
    pub fn playing_generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    /// Knob overrides set, as of the last block.
    pub fn overrides_active(&self) -> u32 {
        self.shared.overrides_active.load(Ordering::Relaxed)
    }

    /// Drops every update the processor sent back; returns how many.
    pub fn collect_garbage(&mut self) -> usize {
        let mut n = 0;
        while self.garbage.pop().is_ok() {
            n += 1;
        }
        n
    }
}

/// An audio device that plays a [`LiveProcessor`]. The engine crate holds
/// only this seam; device code (`cpal` or a null output) implements it
/// elsewhere.
pub trait AudioOut: Send {
    /// The rate the device will run at, given the rate the song `wanted`.
    /// It may differ; compile the song at the returned rate before
    /// [`live`].
    fn sample_rate(&mut self, wanted: u32) -> Result<u32, String>;

    /// Moves `processor` into the device's callback and starts output.
    /// Dropping the returned handle stops it.
    fn start(&mut self, processor: LiveProcessor) -> Result<Box<dyn Send>, String>;

    /// Whether the device went away after [`start`](Self::start) (unplugged,
    /// say). Poll it from a control loop; devices that cannot be lost keep
    /// the default `false`.
    fn device_lost(&self) -> bool {
        false
    }
}

/// The realtime half, owned by the audio callback. `process` never
/// allocates, frees, locks or blocks.
pub struct LiveProcessor {
    engine: Engine,
    commands: Consumer<Command>,
    garbage: Producer<Box<SongUpdate>>,
    shared: Arc<Shared>,
    sample_rate: u32,
}

impl LiveProcessor {
    /// Applies pending commands, then fills interleaved stereo `out`.
    pub fn process(&mut self, out: &mut [f32]) {
        self.return_retired();
        self.drain_commands();
        self.engine.process(out);
        self.return_retired();
        self.shared.publish(&self.engine);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn drain_commands(&mut self) {
        while let Ok(command) = self.commands.peek() {
            if let Command::Swap(_) = command {
                // Room for this update once applied, for a pending one it
                // replaces, and for an applied one still waiting to go back.
                let needed = 1
                    + usize::from(self.engine.has_pending_swap())
                    + usize::from(self.engine.has_retired_swap());
                if self.garbage.slots() < needed {
                    return;
                }
            }
            let Ok(command) = self.commands.pop() else {
                return;
            };
            match command {
                Command::Swap(update) => {
                    if let Some(replaced) = self.engine.queue_swap(update) {
                        self.send_garbage(replaced);
                    }
                }
                Command::Seek(bar) => self.engine.seek(bar),
                Command::Loop(bars) => self.engine.set_loop(bars),
                Command::Override {
                    generation,
                    track,
                    target,
                    plain,
                } => {
                    // Meant for a song that has not landed (or never will).
                    if generation == self.engine.generation() {
                        self.engine.set_override(track, target, plain);
                    }
                }
            }
        }
    }

    /// Sends an applied swap back if the garbage queue has room; otherwise
    /// the engine keeps it (and holds off further swaps) until it does.
    fn return_retired(&mut self) {
        if !self.garbage.is_full()
            && let Some(update) = self.engine.take_retired_swap()
        {
            self.send_garbage(update);
        }
    }

    fn send_garbage(&mut self, update: Box<SongUpdate>) {
        // Callers make sure there is room, so this never drops the update
        // (which would free memory on the audio thread).
        let pushed = self.garbage.push(update);
        debug_assert!(pushed.is_ok(), "garbage queue full");
        if let Err(update) = pushed {
            // Never free on the audio thread: leaking beats a dealloc here.
            std::mem::forget(update);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_are_send() {
        fn send<T: Send>() {}
        send::<LiveProcessor>();
        send::<LiveController>();
        send::<Box<SongUpdate>>();
        send::<Box<dyn AudioOut>>();
    }
}
