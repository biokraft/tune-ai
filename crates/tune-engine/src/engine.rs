//! The sample-by-sample song player shared by offline and realtime output.
//!
//! [`Engine::process`] never allocates: everything is sized when the engine
//! is built. The order of work inside one sample is fixed (see
//! [`Engine::process`]), because offline and live playback must agree bit
//! for bit.

use std::ops::Range;
use std::sync::Arc;

use crate::clock::hit_seed;
use crate::compile::{CompiledSong, CompiledTrack, ParamTarget};
use crate::instrument::{Cell, Effect, Instrument, StepCtx};
use crate::mixer::Meter;
use crate::tracks::{make_effect, make_instrument};

/// The live DSP objects of one track.
pub struct TrackRuntime {
    pub instrument: Box<dyn Instrument>,
    pub fx: Vec<Box<dyn Effect>>,
}

impl TrackRuntime {
    /// Fresh instrument and fx chain for `track`, with its static params set.
    pub fn build(track: &CompiledTrack, sample_rate: f64) -> Self {
        let mut rt = Self {
            instrument: make_instrument(track.kind, sample_rate),
            fx: track
                .fx
                .iter()
                .map(|fx| make_effect(&fx.spec, sample_rate))
                .collect(),
        };
        rt.apply_static(track);
        rt
    }

    /// Sets every instrument and fx param to the track's static value.
    fn apply_static(&mut self, track: &CompiledTrack) {
        for (index, &plain) in track.params.iter().enumerate() {
            self.instrument.set_param_at(index, plain);
        }
        for (fx, spec) in self.fx.iter_mut().zip(&track.fx) {
            for (index, &plain) in spec.params.iter().enumerate() {
                fx.set_param_at(index, plain);
            }
        }
    }

    fn reset(&mut self) {
        self.instrument.reset();
        for fx in &mut self.fx {
            fx.reset();
        }
    }
}

/// Plays a [`CompiledSong`]: arrangement, automation, mixer and master clipper.
pub struct Engine {
    song: Arc<CompiledSong>,
    tracks: Vec<TrackRuntime>,
    /// Song sample position of the next sample.
    pos: u64,
    /// The next step to fire.
    step: u64,
    /// Section of the last step fired.
    section: usize,
    /// Set by `new`, `seek` (and later swaps): the next step re-applies
    /// static params even if the section stays the same.
    needs_static: bool,
    /// Loop as half-open step range.
    looping: Option<(u64, u64)>,
    finished: bool,
    any_solo: bool,
    gate_samples: i32,
    meter: Meter,
}

static REST: Cell = Cell::Rest;

impl Engine {
    /// An engine at bar 0 with fresh runtimes for every track.
    pub fn new(song: Arc<CompiledSong>) -> Self {
        let sample_rate = f64::from(song.clock.sample_rate);
        let tracks = song
            .tracks
            .iter()
            .map(|t| TrackRuntime::build(t, sample_rate))
            .collect();
        Self::from_parts(song, tracks)
    }

    /// An engine at bar 0 playing `tracks`, one runtime per song track in
    /// song order. The runtimes are used as they are (not reset).
    pub fn from_parts(song: Arc<CompiledSong>, tracks: Vec<TrackRuntime>) -> Self {
        assert_eq!(
            tracks.len(),
            song.tracks.len(),
            "one runtime per track is required"
        );
        Self {
            any_solo: song.tracks.iter().any(|t| t.solo),
            gate_samples: song.clock.gate_samples(),
            song,
            tracks,
            pos: 0,
            step: 0,
            section: 0,
            needs_static: true,
            looping: None,
            finished: false,
            meter: Meter::default(),
        }
    }

    /// Jumps to the start of 0-based `bar` (clamped to the song's end) and
    /// resets every instrument and effect. The loop and meter are kept.
    pub fn seek(&mut self, bar: u32) {
        let bar = bar.min(self.song.total_bars);
        for rt in &mut self.tracks {
            rt.reset();
        }
        self.step = 16 * u64::from(bar);
        self.pos = self.song.clock.step_start(self.step);
        self.section = section_of(&self.song, self.step);
        self.needs_static = true;
        self.finished = false;
    }

    /// Loops the half-open bar range, clamped to the song; an empty range
    /// (after clamping) removes the loop. The jump back happens when the
    /// playhead reaches the loop's end.
    pub fn set_loop(&mut self, bars: Option<Range<u32>>) {
        let total = self.song.total_bars;
        self.looping = bars.and_then(|r| {
            let (start, end) = (r.start.min(total), r.end.min(total));
            (start < end).then(|| (16 * u64::from(start), 16 * u64::from(end)))
        });
    }

    /// Fills interleaved stereo `out` (an even length).
    ///
    /// Per sample, in this order:
    /// 1. When finished, write silence.
    /// 2. At a step boundary: wrap at the loop end (nothing is reset); finish
    ///    at the song end; re-apply static params on a section change (or the
    ///    first step after `new`/`seek`); apply the section's lanes for this
    ///    sample; fire `on_step` on every track in order; advance the step.
    ///    Elsewhere: apply the section's lanes for this sample. Lanes run
    ///    exactly once per sample, always before the tracks' `on_step`.
    /// 3. Process every track (instrument, then fx in order), write its stem
    ///    and add it to the master when audible.
    /// 4. Meter, soft-clip, write, advance the position.
    pub fn process(&mut self, out: &mut [f32]) {
        for frame in out.chunks_exact_mut(2) {
            let (l, r) = self.frame(|_, _, _| {});
            frame[0] = l;
            frame[1] = r;
        }
    }

    /// Like [`process`](Self::process), also writing one interleaved stereo
    /// stem per track (each as long as `mix`). Stems ignore mute and solo and
    /// skip the clipper.
    pub fn process_stems(&mut self, mix: &mut [f32], stems: &mut [&mut [f32]]) {
        assert_eq!(stems.len(), self.tracks.len(), "one stem per track");
        for stem in stems.iter() {
            assert_eq!(stem.len(), mix.len(), "stems must match the mix length");
        }
        for (i, frame) in mix.chunks_exact_mut(2).enumerate() {
            let (l, r) = self.frame(|t, sl, sr| {
                stems[t][2 * i] = sl as f32;
                stems[t][2 * i + 1] = sr as f32;
            });
            frame[0] = l;
            frame[1] = r;
        }
    }

    /// One frame; `stem(track, l, r)` receives every track's output.
    fn frame(&mut self, mut stem: impl FnMut(usize, f64, f64)) -> (f32, f32) {
        if self.finished {
            return self.silence(&mut stem);
        }
        let song = &*self.song;
        if self.pos == song.clock.step_start(self.step) {
            if let Some((start, end)) = self.looping
                && self.step == end
            {
                self.step = start;
                self.pos = song.clock.step_start(start);
            }
            let total_steps = 16 * u64::from(song.total_bars);
            if self.step == total_steps {
                self.finished = true;
                return self.silence(&mut stem);
            }
            let section = section_of(song, self.step);
            if section != self.section || self.needs_static {
                for (rt, track) in self.tracks.iter_mut().zip(&song.tracks) {
                    rt.apply_static(track);
                }
                self.section = section;
                self.needs_static = false;
            }
            apply_lanes(song, section, self.pos, &mut self.tracks);
            let step = self.step;
            let next_section = section_of(song, step + 1);
            for (t, (rt, track)) in self.tracks.iter_mut().zip(&song.tracks).enumerate() {
                let next = cell_at(song, next_section, t, step + 1);
                let ctx = StepCtx {
                    step,
                    next_has_gate: matches!(next, Cell::Note(n) if n.note.is_some()),
                    gate_samples: self.gate_samples,
                    seed: hit_seed(track.seed, step),
                };
                rt.instrument.on_step(cell_at(song, section, t, step), &ctx);
            }
            self.step += 1;
        } else {
            apply_lanes(song, self.section, self.pos, &mut self.tracks);
        }

        let (mut ml, mut mr) = (0.0, 0.0);
        for (t, (rt, track)) in self.tracks.iter_mut().zip(&song.tracks).enumerate() {
            let mut y = rt.instrument.process();
            for fx in &mut rt.fx {
                y = fx.process(y);
            }
            let l = y * track.gain * track.pan_gains.0;
            let r = y * track.gain * track.pan_gains.1;
            stem(t, l, r);
            if !track.mute && (!self.any_solo || track.solo) {
                ml += l;
                mr += r;
            }
        }
        let (cl, cr) = self.meter.clip_frame(ml, mr);
        self.pos += 1;
        (cl as f32, cr as f32)
    }

    fn silence(&self, stem: &mut impl FnMut(usize, f64, f64)) -> (f32, f32) {
        for t in 0..self.tracks.len() {
            stem(t, 0.0, 0.0);
        }
        (0.0, 0.0)
    }

    /// Song sample position of the next sample.
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// 0-based bar of the playhead (`total_bars` at the end).
    pub fn bar(&self) -> u32 {
        let total = self.song.total_bars;
        if self.pos >= self.song.end_sample() {
            return total;
        }
        ((self.song.clock.step_at(self.pos) / 16) as u32).min(total)
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn meter(&self) -> Meter {
        self.meter
    }

    pub fn reset_meter(&mut self) {
        self.meter = Meter::default();
    }

    pub fn song(&self) -> &Arc<CompiledSong> {
        &self.song
    }
}

/// Index of the section containing `step` (the last one for steps past the
/// end), by binary search on section start steps.
fn section_of(song: &CompiledSong, step: u64) -> usize {
    song.sections
        .partition_point(|s| 16 * u64::from(s.start_bar) <= step)
        .saturating_sub(1)
}

/// The cell track `t` plays at `step` in `section`: `Rest` when the section
/// does not play the track or the step lies past the end; otherwise the
/// pattern restarted at the section start.
fn cell_at(song: &CompiledSong, section: usize, t: usize, step: u64) -> &Cell {
    if step >= 16 * u64::from(song.total_bars) {
        return &REST;
    }
    let s = &song.sections[section];
    match s.play[t] {
        None => &REST,
        Some(p) => {
            let pattern = &song.patterns[p];
            let offset = step - 16 * u64::from(s.start_bar);
            &pattern[(offset % pattern.len() as u64) as usize]
        }
    }
}

/// Applies every lane of `section` at song sample `pos`.
fn apply_lanes(song: &CompiledSong, section: usize, pos: u64, tracks: &mut [TrackRuntime]) {
    let s = &song.sections[section];
    if s.lanes.is_empty() {
        return;
    }
    let start = song.clock.bar_start(s.start_bar);
    let x = pos.saturating_sub(start) as f64 / (16.0 * song.clock.samples_per_step);
    for lane in &s.lanes {
        let plain = lane.spec.to_plain(lane_value(&lane.points, x));
        let rt = &mut tracks[lane.track];
        match lane.target {
            ParamTarget::Instrument(index) => rt.instrument.set_param_at(index, plain),
            ParamTarget::Fx { slot, index } => rt.fx[slot].set_param_at(index, plain),
        }
    }
}

/// The normalized lane value at bar offset `x`: the first point's value
/// before it, linear between points, the last point's value after it, and
/// the later point where two share a bar.
fn lane_value(points: &[(f64, f64)], x: f64) -> f64 {
    let i = points.partition_point(|&(bar, _)| bar <= x);
    if i == 0 {
        return points[0].1;
    }
    if i == points.len() {
        return points[i - 1].1;
    }
    let (b0, v0) = points[i - 1];
    let (b1, v1) = points[i];
    v0 + (v1 - v0) * (x - b0) / (b1 - b0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_value_shapes() {
        let ramp = [(0.0, 0.0), (1.0, 1.0)];
        assert_eq!(lane_value(&ramp, 0.0), 0.0);
        assert_eq!(lane_value(&ramp, 0.5), 0.5);
        assert_eq!(lane_value(&ramp, 1.0), 1.0);
        assert_eq!(lane_value(&ramp, 1.7), 1.0);
        let single = [(0.5, 0.3)];
        assert_eq!(lane_value(&single, 0.0), 0.3);
        assert_eq!(lane_value(&single, 0.9), 0.3);
        let jump = [(1.0, 0.2), (1.0, 0.9), (2.0, 0.5)];
        assert_eq!(lane_value(&jump, 0.99), 0.2);
        assert_eq!(lane_value(&jump, 1.0), 0.9);
        assert!((lane_value(&jump, 1.5) - 0.7).abs() < 1e-15);
        assert_eq!(lane_value(&jump, 3.0), 0.5);
    }

    #[test]
    fn engine_is_send() {
        fn send<T: Send>() {}
        send::<Engine>();
        send::<TrackRuntime>();
        send::<Arc<CompiledSong>>();
    }
}
