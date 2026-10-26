//! Shared helpers for the engine integration tests.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use tune_dsp::ParamSpec;
use tune_engine::{
    Cell, CompiledSong, CompiledTrack, Effect, Engine, Instrument, StepCtx, TrackRuntime,
};
use tune_song::{Song, parse_song};

/// The shared `mini` fixture.
pub const MINI: &str = include_str!("../../../tune-song/tests/fixtures/mini.toml");

/// The shared `mixed` fixture: `mini` plus the mixer.
pub const MIXED: &str = include_str!("../../../tune-song/tests/fixtures/mixed.toml");

/// Parses `src` (which must be valid) and compiles it at the song's rate
/// (48 kHz by default).
pub fn compiled(src: &str) -> Arc<CompiledSong> {
    compile_song(&song(src))
}

pub fn song(src: &str) -> Song {
    parse_song(src).unwrap_or_else(|e| panic!("song does not parse: {e:?}"))
}

pub fn compile_song(song: &Song) -> Arc<CompiledSong> {
    Arc::new(CompiledSong::new(song, song.sample_rate.unwrap_or(48_000)))
}

/// `src` changed by `edit`, checked by a write and re-parse.
pub fn variant_of(src: &str, edit: impl FnOnce(&mut Song)) -> Song {
    let mut s = song(src);
    edit(&mut s);
    parse_song(&s.to_toml()).unwrap_or_else(|e| panic!("variant does not validate: {e:?}"))
}

/// Removes the lane on `track`'s `param` from every section.
pub fn drop_lane(s: &mut Song, track: &str, param: &str) {
    for section in &mut s.sections {
        section
            .automation
            .retain(|l| !(l.track == track && l.param == param));
    }
}

/// The bit patterns of `x`, for bitwise comparisons.
pub fn bits(x: &[f32]) -> Vec<u32> {
    x.iter().map(|v| v.to_bits()).collect()
}

/// One recorded call on a probe, in call order. `sample` is the number of
/// `process` calls the probe saw before the call.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Step {
        sample: u64,
        cell: Cell,
        ctx: StepCtx,
    },
    Param {
        sample: u64,
        index: usize,
        plain: f64,
    },
    Reset {
        sample: u64,
    },
}

#[derive(Default, Debug)]
pub struct Log {
    pub events: Vec<Event>,
    /// `process` calls so far (kept by [`ProbeFx`]).
    pub samples: u64,
}

impl Log {
    /// Every `on_step` as `(sample, cell, ctx)`.
    pub fn steps(&self) -> Vec<(u64, Cell, StepCtx)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Step { sample, cell, ctx } => Some((*sample, cell.clone(), *ctx)),
                _ => None,
            })
            .collect()
    }

    /// Every `set_param_at` as `(sample, index, plain)`.
    pub fn params(&self) -> Vec<(u64, usize, f64)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Param {
                    sample,
                    index,
                    plain,
                } => Some((*sample, *index, *plain)),
                _ => None,
            })
            .collect()
    }

    /// The value of param `index` in effect while each of the first `len`
    /// samples is processed (the last `set_param_at` at or before it).
    pub fn trace(&self, index: usize, len: u64) -> Vec<Option<f64>> {
        let mut out = vec![None; len as usize];
        let mut current = None;
        let mut records = self
            .params()
            .into_iter()
            .filter(|&(_, i, _)| i == index)
            .peekable();
        for (n, slot) in out.iter_mut().enumerate() {
            while let Some(&(s, _, v)) = records.peek() {
                if s > n as u64 {
                    break;
                }
                current = Some(v);
                records.next();
            }
            *slot = current;
        }
        out
    }
}

pub type SharedLog = Arc<Mutex<Log>>;

/// An instrument that records every call and outputs silence.
pub struct Probe {
    table: &'static [ParamSpec],
    samples: u64,
    log: SharedLog,
}

impl Probe {
    pub fn new(table: &'static [ParamSpec]) -> (Self, SharedLog) {
        let log = SharedLog::default();
        (
            Self {
                table,
                samples: 0,
                log: log.clone(),
            },
            log,
        )
    }

    fn push(&self, e: Event) {
        self.log.lock().unwrap().events.push(e);
    }
}

impl Instrument for Probe {
    fn params(&self) -> &'static [ParamSpec] {
        self.table
    }
    fn set_param_at(&mut self, index: usize, plain: f64) {
        self.push(Event::Param {
            sample: self.samples,
            index,
            plain,
        });
    }
    fn on_step(&mut self, cell: &Cell, ctx: &StepCtx) {
        self.push(Event::Step {
            sample: self.samples,
            cell: cell.clone(),
            ctx: *ctx,
        });
    }
    fn process(&mut self) -> f64 {
        self.samples += 1;
        0.0
    }
    fn reset(&mut self) {
        self.push(Event::Reset {
            sample: self.samples,
        });
    }
}

/// An effect that records every `set_param_at` and its `process` count, and
/// passes audio through (or outputs a constant, see [`ProbeFx::constant`]).
pub struct ProbeFx {
    table: &'static [ParamSpec],
    samples: u64,
    output: Option<f64>,
    log: SharedLog,
}

impl ProbeFx {
    pub fn new(table: &'static [ParamSpec]) -> (Self, SharedLog) {
        let log = SharedLog::default();
        (
            Self {
                table,
                samples: 0,
                output: None,
                log: log.clone(),
            },
            log,
        )
    }

    /// Like [`new`](Self::new), but every `process` returns `value` whatever
    /// comes in, so what follows the probe does not depend on the instrument.
    pub fn constant(table: &'static [ParamSpec], value: f64) -> (Self, SharedLog) {
        let (mut fx, log) = Self::new(table);
        fx.output = Some(value);
        (fx, log)
    }
}

impl Effect for ProbeFx {
    fn params(&self) -> &'static [ParamSpec] {
        self.table
    }
    fn set_param_at(&mut self, index: usize, plain: f64) {
        self.log.lock().unwrap().events.push(Event::Param {
            sample: self.samples,
            index,
            plain,
        });
    }
    fn process(&mut self, x: f64) -> f64 {
        self.samples += 1;
        self.log.lock().unwrap().samples = self.samples;
        self.output.unwrap_or(x)
    }
    fn reset(&mut self) {}
}

/// An engine for `song` whose track `id` plays a [`Probe`] (with the track's
/// own param table); every other track gets its real runtime.
pub fn probe_engine(song: &Arc<CompiledSong>, id: &str) -> (Engine, SharedLog) {
    let (engine, log, _) = probe_engine_fx(song, id, false);
    (engine, log)
}

/// Like [`probe_engine`]; with `probe_fx`, the probed track's first fx slot
/// is a [`ProbeFx`] too, whose log is returned.
pub fn probe_engine_fx(
    song: &Arc<CompiledSong>,
    id: &str,
    probe_fx: bool,
) -> (Engine, SharedLog, Option<SharedLog>) {
    let slots: &[usize] = if probe_fx { &[0] } else { &[] };
    let (engine, log, fx_logs) = probe_engine_slots(song, id, slots);
    (engine, log, fx_logs.into_iter().next())
}

/// Like [`probe_engine`], with a [`ProbeFx`] in each of the probed track's
/// fx `slots` (each with its device's param table); returns their logs in
/// the order of `slots`.
pub fn probe_engine_slots(
    song: &Arc<CompiledSong>,
    id: &str,
    slots: &[usize],
) -> (Engine, SharedLog, Vec<SharedLog>) {
    let mut log = None;
    let mut fx_logs = Vec::new();
    let engine = engine_with(song, |t, rt| {
        if t.id != id {
            return;
        }
        let (probe, l) = Probe::new(t.instrument.module.params);
        rt.instrument = Box::new(probe);
        log = Some(l);
        for &slot in slots {
            let (fx, l) = ProbeFx::new(t.fx[slot].module.params);
            rt.fx[slot] = Box::new(fx);
            fx_logs.push(l);
        }
    });
    let log = log.unwrap_or_else(|| panic!("no track `{id}`"));
    (engine, log, fx_logs)
}

/// An engine for `song` whose runtimes are built as usual, then changed by
/// `edit` (called once per track, in song order).
pub fn engine_with(
    song: &Arc<CompiledSong>,
    mut edit: impl FnMut(&CompiledTrack, &mut TrackRuntime),
) -> Engine {
    let sr = f64::from(song.clock.sample_rate);
    let tracks = song
        .tracks
        .iter()
        .map(|t| {
            let mut rt = TrackRuntime::build(t, sr);
            edit(t, &mut rt);
            rt
        })
        .collect();
    Engine::from_parts(song.clone(), tracks)
}

/// Runs `engine` for `frames` frames in 4096-frame blocks and returns the
/// interleaved stereo mix.
pub fn run(engine: &mut Engine, frames: u64) -> Vec<f32> {
    let mut out = vec![0.0f32; 2 * frames as usize];
    for block in out.chunks_mut(2 * 4096) {
        engine.process(block);
    }
    out
}

/// Runs `engine` for `frames` frames and returns the mix and one stereo stem
/// per track.
pub fn run_stems(engine: &mut Engine, frames: u64) -> (Vec<f32>, Vec<Vec<f32>>) {
    let n = engine.song().tracks.len();
    let mut mix = vec![0.0f32; 2 * frames as usize];
    let mut stems = vec![vec![0.0f32; 2 * frames as usize]; n];
    let block = 2 * 4096;
    for (b, chunk) in mix.chunks_mut(block).enumerate() {
        let start = b * block;
        let len = chunk.len();
        let mut views: Vec<&mut [f32]> = stems
            .iter_mut()
            .map(|s| &mut s[start..start + len])
            .collect();
        engine.process_stems(chunk, &mut views);
    }
    (mix, stems)
}

pub fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64).sqrt()
}
