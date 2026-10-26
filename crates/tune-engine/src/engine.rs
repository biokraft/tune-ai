//! The sample-by-sample song player shared by offline and realtime output.
//!
//! [`Engine::process`] never allocates: everything is sized when the engine
//! is built. The order of work inside one sample is fixed (see
//! [`Engine::process`]), because offline and live playback must agree bit
//! for bit.

use std::ops::Range;
use std::sync::Arc;

use tune_dsp::{ParamSpec, STRIP_PARAMS};
use tune_modules::{Cell, Effect, Instrument, StepCtx};
use tune_song::lane_value;

use crate::clock::hit_seed;
use crate::compile::{CompiledDevice, CompiledSong, CompiledTrack, MIX_TRACK, ParamTarget};
use crate::live::SongUpdate;
use crate::mixer::{Meter, MixRuntime, Strip};

/// The live DSP objects of one track, with its knob overrides.
pub struct TrackRuntime {
    pub instrument: Box<dyn Instrument>,
    pub fx: Vec<Box<dyn Effect>>,
    /// Sends and ducking.
    pub strip: Strip,
    /// One override per instrument param in plain units; `NaN` is none.
    overrides: Vec<f64>,
    /// One override per strip param, like `overrides`.
    strip_overrides: Vec<f64>,
    /// One override table per fx slot, like `overrides`. Each travels with
    /// its device: a swap that rebuilds a device drops it.
    fx_overrides: Vec<Vec<f64>>,
}

impl TrackRuntime {
    /// Fresh instrument and fx chain for `track`, with its static params set
    /// and room for an override on every param (none set). Each device's
    /// factory gets the device's settings, which in a compiled song are
    /// always filled in from the module's own values (what the factories
    /// require).
    pub fn build(track: &CompiledTrack, sample_rate: f64) -> Self {
        let instrument = &track.instrument;
        let mut rt = Self {
            instrument: instrument
                .module
                .make_instrument(sample_rate, &instrument.settings)
                .unwrap_or_else(|| panic!("`{}` is not an instrument", instrument.module.name)),
            fx: track
                .fx
                .iter()
                .map(|fx| {
                    fx.module
                        .make_effect(sample_rate, &fx.settings)
                        .unwrap_or_else(|| panic!("`{}` is not an effect", fx.module.name))
                })
                .collect(),
            strip: Strip::new(sample_rate),
            overrides: vec![f64::NAN; instrument.params.len()],
            strip_overrides: vec![f64::NAN; STRIP_PARAMS.len()],
            fx_overrides: track
                .fx
                .iter()
                .map(|fx| vec![f64::NAN; fx.params.len()])
                .collect(),
        };
        rt.apply_static(track);
        rt
    }

    /// Sets every instrument, strip and fx param to the track's static value.
    fn apply_static(&mut self, track: &CompiledTrack) {
        for (index, &plain) in track.instrument.params.iter().enumerate() {
            self.instrument.set_param_at(index, plain);
        }
        for (index, &plain) in track.strip.iter().enumerate() {
            self.strip.set_param_at(index, plain);
        }
        for (fx, spec) in self.fx.iter_mut().zip(&track.fx) {
            for (index, &plain) in spec.params.iter().enumerate() {
                fx.set_param_at(index, plain);
            }
        }
    }

    /// Sets one param in plain units. A mix target is not the track's and
    /// is ignored.
    fn set_target(&mut self, target: ParamTarget, plain: f64) {
        match target {
            ParamTarget::Instrument(index) => self.instrument.set_param_at(index, plain),
            ParamTarget::Fx { slot, index } => self.fx[slot].set_param_at(index, plain),
            ParamTarget::Strip(index) => self.strip.set_param_at(index, plain),
            ParamTarget::Mix { .. } => {}
        }
    }

    /// The override slot of `target` and its spec, if the runtime has both.
    fn override_slot(&mut self, target: ParamTarget) -> Option<(&mut f64, &'static ParamSpec)> {
        match target {
            ParamTarget::Instrument(index) => {
                let spec = self.instrument.params().get(index)?;
                Some((self.overrides.get_mut(index)?, spec))
            }
            ParamTarget::Fx { slot, index } => {
                let spec = self.fx.get(slot)?.params().get(index)?;
                Some((self.fx_overrides.get_mut(slot)?.get_mut(index)?, spec))
            }
            ParamTarget::Strip(index) => {
                let spec = STRIP_PARAMS.get(index)?;
                Some((self.strip_overrides.get_mut(index)?, spec))
            }
            ParamTarget::Mix { .. } => None,
        }
    }

    /// Sets every overridden param to its override.
    fn apply_overrides(&mut self) {
        for (index, &plain) in self.overrides.iter().enumerate() {
            if !plain.is_nan() {
                self.instrument.set_param_at(index, plain);
            }
        }
        for (index, &plain) in self.strip_overrides.iter().enumerate() {
            if !plain.is_nan() {
                self.strip.set_param_at(index, plain);
            }
        }
        for (fx, table) in self.fx.iter_mut().zip(&self.fx_overrides) {
            for (index, &plain) in table.iter().enumerate() {
                if !plain.is_nan() {
                    fx.set_param_at(index, plain);
                }
            }
        }
    }

    /// Clears every override, setting each param it held back to the
    /// track's static value.
    fn clear_overrides(&mut self, track: &CompiledTrack) {
        for (index, plain) in self.overrides.iter_mut().enumerate() {
            if !plain.is_nan() {
                *plain = f64::NAN;
                self.instrument
                    .set_param_at(index, track.instrument.params[index]);
            }
        }
        for (index, plain) in self.strip_overrides.iter_mut().enumerate() {
            if !plain.is_nan() {
                *plain = f64::NAN;
                self.strip.set_param_at(index, track.strip[index]);
            }
        }
        for ((fx, table), spec) in self
            .fx
            .iter_mut()
            .zip(&mut self.fx_overrides)
            .zip(&track.fx)
        {
            for (index, plain) in table.iter_mut().enumerate() {
                if !plain.is_nan() {
                    *plain = f64::NAN;
                    fx.set_param_at(index, spec.params[index]);
                }
            }
        }
    }

    /// How many overrides are set.
    fn overrides_set(&self) -> u32 {
        let set = |table: &[f64]| table.iter().filter(|v| !v.is_nan()).count() as u32;
        set(&self.overrides)
            + set(&self.strip_overrides)
            + self.fx_overrides.iter().map(|t| set(t)).sum::<u32>()
    }

    fn reset(&mut self) {
        self.instrument.reset();
        self.strip.reset();
        for fx in &mut self.fx {
            fx.reset();
        }
    }

    /// A silent stand-in left behind when a swap moves a runtime out of the
    /// old list. Neither building nor dropping it touches the heap: the
    /// instrument is zero-sized, the fx chain is empty and the strip holds
    /// no buffers.
    fn placeholder() -> Self {
        Self {
            instrument: Box::new(Vacant),
            fx: Vec::new(),
            strip: Strip::new(48_000.0),
            overrides: Vec::new(),
            strip_overrides: Vec::new(),
            fx_overrides: Vec::new(),
        }
    }
}

/// The zero-sized instrument behind [`TrackRuntime::placeholder`].
struct Vacant;

impl Instrument for Vacant {
    fn params(&self) -> &'static [ParamSpec] {
        &[]
    }
    fn set_param_at(&mut self, _: usize, _: f64) {}
    fn on_step(&mut self, _: &Cell, _: &StepCtx) {}
    fn process(&mut self) -> f64 {
        0.0
    }
    fn reset(&mut self) {}
}

/// Whether a swap may keep the running `a` for `b`: the same id, module
/// and settings (what the factory takes). Params may differ; the static
/// reset after a swap sets them.
fn same_device(a: &CompiledDevice, b: &CompiledDevice) -> bool {
    a.id == b.id && a.module == b.module && a.settings == b.settings
}

/// Plays a [`CompiledSong`]: arrangement, automation, mixer, buses and the
/// master stage.
pub struct Engine {
    song: Arc<CompiledSong>,
    tracks: Vec<TrackRuntime>,
    /// The buses and the master stage; a swap keeps each unit or takes the
    /// update's (see [`MixRuntime::take_units`]).
    mix: MixRuntime,
    /// Song sample position of the next sample.
    pos: u64,
    /// The next step to fire.
    step: u64,
    /// Region of the last step fired.
    region: usize,
    /// Set by `new`, `seek` and swaps: the next step re-applies static params
    /// even if the region stays the same.
    needs_static: bool,
    /// Loop as half-open step range.
    looping: Option<(u64, u64)>,
    finished: bool,
    any_solo: bool,
    gate_samples: i32,
    meter: Meter,
    /// A swap waiting for the next step boundary.
    pending: Option<Box<SongUpdate>>,
    /// The last applied swap, holding the old song, until it is taken.
    retired: Option<Box<SongUpdate>>,
    swaps_applied: u64,
    /// Swaps that kept every unit of `mix`.
    mix_swaps_kept: u64,
    /// The generation of the song playing (see [`generation`](Self::generation)).
    generation: u64,
    /// Knob overrides set across all runtimes.
    overrides_active: u32,
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
    /// song order. The runtimes are used as they are (not reset); the mix
    /// runtime is built fresh, which allocates: call off the audio thread.
    pub fn from_parts(song: Arc<CompiledSong>, tracks: Vec<TrackRuntime>) -> Self {
        assert_eq!(
            tracks.len(),
            song.tracks.len(),
            "one runtime per track is required"
        );
        Self {
            any_solo: song.tracks.iter().any(|t| t.solo),
            gate_samples: song.clock.gate_samples(),
            mix: MixRuntime::build(&song),
            song,
            tracks,
            pos: 0,
            step: 0,
            region: 0,
            needs_static: true,
            looping: None,
            finished: false,
            meter: Meter::default(),
            pending: None,
            retired: None,
            swaps_applied: 0,
            mix_swaps_kept: 0,
            generation: 0,
            overrides_active: 0,
        }
    }

    /// Queues `update` to replace the song at the next step boundary and
    /// returns the swap it replaces, if one was still pending. The update
    /// must be built at the engine's sample rate.
    ///
    /// The swap applies only once the previous one has been taken with
    /// [`take_retired_swap`](Self::take_retired_swap), so the engine never
    /// drops a song itself. Applying it, at step boundary `k`:
    /// - every new track takes the old runtime with the same id (its strip
    ///   and ducker), and in it every device that is the same device as one
    ///   of the old track's (same id, module and settings, see
    ///   `same_device`), wherever it moved in the chain; any other device,
    ///   and every other track, comes fresh from the update;
    /// - the mix keeps each unit the new song can use and takes the
    ///   update's fresh one for the rest, the old one going back in the
    ///   retired update: the delay is replaced when its bus appears or goes
    ///   or the step length changes, the reverb only when its bus appears or
    ///   goes, and a master-mode switch crossfades between the clipper and
    ///   the limiter without a silent frame (see
    ///   [`MixRuntime::take_units`]);
    /// - the new clock takes over at step `k` (the step index survives a bpm
    ///   change) and static params are re-applied;
    /// - the loop is clamped to the new end, and dropped when it starts at
    ///   or past it;
    /// - if `k` lies at or past the new end, playback jumps to the loop start
    ///   when a loop remains and finishes otherwise.
    ///
    /// A finished engine applies a swap at once and stays finished at the
    /// new end; [`seek`](Self::seek) restarts it.
    ///
    /// Knob overrides travel with what a swap keeps (a device's overrides
    /// with the device, strip overrides with the track, mix overrides with
    /// the engine). An update made to clear the overrides clears them all
    /// when it lands; an update that replaces such a pending one inherits
    /// that.
    pub fn queue_swap(&mut self, mut update: Box<SongUpdate>) -> Option<Box<SongUpdate>> {
        debug_assert_eq!(
            update.song.clock.sample_rate, self.song.clock.sample_rate,
            "a swap must keep the sample rate"
        );
        if self.pending.as_ref().is_some_and(|p| p.clear_overrides) {
            update.clear_overrides = true;
        }
        self.pending.replace(update)
    }

    pub fn has_pending_swap(&self) -> bool {
        self.pending.is_some()
    }

    pub fn has_retired_swap(&self) -> bool {
        self.retired.is_some()
    }

    /// The last applied swap's update, now holding the old song and every
    /// runtime the new song did not take. Drop it off the audio thread.
    pub fn take_retired_swap(&mut self) -> Option<Box<SongUpdate>> {
        self.retired.take()
    }

    /// Swaps applied since the engine was built.
    pub fn swaps_applied(&self) -> u64 {
        self.swaps_applied
    }

    /// Swaps that kept every unit of the running mix: buses and master
    /// stage (see [`queue_swap`](Self::queue_swap)).
    pub fn mix_swaps_kept(&self) -> u64 {
        self.mix_swaps_kept
    }

    /// The generation of the song playing: 0 for the song the engine was
    /// built with, then that of each applied swap's update (assigned by the
    /// [`LiveController`](crate::live::LiveController)).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Overrides param `target` of track `track` with `plain` (clamped to the
    /// param's range), or clears the override with `None` (or a NaN), which
    /// sets the param back to its static value; a lane then takes it over
    /// again from the next sample. A mix param takes track [`MIX_TRACK`] and
    /// a [`ParamTarget::Mix`] target. An override wins over static params
    /// and lanes and is applied every sample, before `on_step` (see
    /// [`process`](Self::process)). A track or target outside the song is
    /// ignored. Never allocates. Overrides are live-only: offline renders
    /// build their own engine and never see them.
    pub fn set_override(&mut self, track: usize, target: ParamTarget, plain: Option<f64>) {
        let slot = match (track, target) {
            (MIX_TRACK, ParamTarget::Mix { unit, index }) => self.mix.override_slot(unit, index),
            (MIX_TRACK, _) => None,
            _ => self
                .tracks
                .get_mut(track)
                .and_then(|rt| rt.override_slot(target)),
        };
        let Some((slot, spec)) = slot else {
            return;
        };
        let was_set = !slot.is_nan();
        match plain.filter(|v| !v.is_nan()) {
            Some(v) => {
                *slot = v.clamp(spec.min, spec.max);
                if !was_set {
                    self.overrides_active += 1;
                }
            }
            None => {
                *slot = f64::NAN;
                if was_set {
                    self.overrides_active -= 1;
                    self.restore_static(track, target);
                }
            }
        }
    }

    /// Sets param `target` of track `track` (or of the mix) back to its
    /// static value. Both must exist.
    fn restore_static(&mut self, track: usize, target: ParamTarget) {
        let tracks = &self.song.tracks;
        let plain = match target {
            ParamTarget::Instrument(index) => tracks[track].instrument.params[index],
            ParamTarget::Fx { slot, index } => tracks[track].fx[slot].params[index],
            ParamTarget::Strip(index) => tracks[track].strip[index],
            ParamTarget::Mix { unit, index } => {
                return self.mix.restore(&self.song.mix, unit, index);
            }
        };
        self.tracks[track].set_target(target, plain);
    }

    /// Clears every override (see [`set_override`](Self::set_override)).
    pub fn clear_overrides(&mut self) {
        if self.overrides_active == 0 {
            return;
        }
        for (rt, track) in self.tracks.iter_mut().zip(&self.song.tracks) {
            rt.clear_overrides(track);
        }
        self.mix.clear_overrides(&self.song.mix);
        self.overrides_active = 0;
    }

    /// Knob overrides currently set.
    pub fn overrides_active(&self) -> u32 {
        self.overrides_active
    }

    /// Jumps to the start of 0-based `bar` (clamped to the song's end) and
    /// resets every instrument, effect, ducker, bus and the limiter. The
    /// loop, meter and knob overrides are kept.
    ///
    /// Audio after a seek starts from reset instrument state (silent filters,
    /// envelopes and delay lines), so only the param values match play-through,
    /// not the samples.
    pub fn seek(&mut self, bar: u32) {
        let bar = bar.min(self.song.total_bars);
        for rt in &mut self.tracks {
            rt.reset();
        }
        self.mix.reset();
        self.step = 16 * u64::from(bar);
        self.pos = self.song.clock.step_start(self.step);
        self.region = region_of(&self.song, self.step);
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
    /// 0. At a step boundary (or when finished), apply a pending swap (see
    ///    [`queue_swap`](Self::queue_swap)); the rest of the sample then runs
    ///    on the new song.
    /// 1. When finished, write silence.
    /// 2. At a step boundary: wrap at the loop end (nothing is reset); finish
    ///    at the song end; re-apply static params (tracks, strips and mix) on
    ///    a region change (or the first step after `new`/`seek`); apply the
    ///    region's lanes for this sample; apply the knob overrides; fire
    ///    `on_step` on every track in order; if the duck source's cell at
    ///    this step is a hit (muted or not), trigger every track's ducker;
    ///    advance the step. Elsewhere: apply the region's lanes, then the
    ///    overrides, for this sample. Lanes and overrides run exactly once
    ///    per sample (overrides only when one is set), always before the
    ///    tracks' `on_step`, so a hit reads the overridden value.
    /// 3. For every track in song order: instrument, then fx in order, then
    ///    the ducker (`y`); `l, r = y·gain·pan`; write its stem; when audible
    ///    (mute and solo), add `l, r` to the master and `y·gain·send` to the
    ///    input of each built bus.
    /// 4. Add the delay bus's return, then the reverb bus's, to the master
    ///    (a bus that is not built is off).
    /// 5. Master gain; then the soft clipper, or the limiter in limit mode;
    ///    meter, write, advance the position.
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
    ///
    /// Stems need a fixed track list, so no swap may be pending.
    pub fn process_stems(&mut self, mix: &mut [f32], stems: &mut [&mut [f32]]) {
        assert!(self.pending.is_none(), "no swaps while rendering stems");
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
        if self.pending.is_some()
            && self.retired.is_none()
            && (self.finished || self.pos == self.song.clock.step_start(self.step))
        {
            self.apply_swap();
        }
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
            let region = region_of(song, self.step);
            if region != self.region || self.needs_static {
                for (rt, track) in self.tracks.iter_mut().zip(&song.tracks) {
                    rt.apply_static(track);
                }
                self.mix.apply_static(&song.mix);
                self.region = region;
                self.needs_static = false;
            }
            apply_lanes(song, region, self.pos, &mut self.tracks, &mut self.mix);
            if self.overrides_active > 0 {
                apply_overrides(&mut self.tracks, &mut self.mix);
            }
            let step = self.step;
            // Across the loop wrap the next cell is the loop start's.
            let next_step = match self.looping {
                Some((start, end)) if step + 1 == end => start,
                _ => step + 1,
            };
            let next_region = region_of(song, next_step);
            for (t, (rt, track)) in self.tracks.iter_mut().zip(&song.tracks).enumerate() {
                let next = cell_at(song, next_region, t, next_step);
                let ctx = StepCtx {
                    step,
                    next_has_gate: matches!(next, Cell::Note(n) if n.note.is_some()),
                    gate_samples: self.gate_samples,
                    seed: hit_seed(track.seed, step),
                };
                rt.instrument.on_step(cell_at(song, region, t, step), &ctx);
            }
            if let Some(source) = song.mix.duck_source
                && matches!(cell_at(song, region, source, step), Cell::Hit { .. })
            {
                for rt in &mut self.tracks {
                    rt.strip.trigger_duck();
                }
            }
            self.step += 1;
        } else {
            apply_lanes(song, self.region, self.pos, &mut self.tracks, &mut self.mix);
            if self.overrides_active > 0 {
                apply_overrides(&mut self.tracks, &mut self.mix);
            }
        }

        let (mut ml, mut mr) = (0.0, 0.0);
        let (mut delay_in, mut reverb_in) = (0.0, 0.0);
        let (has_delay, has_reverb) = (self.mix.has_delay(), self.mix.has_reverb());
        for (t, (rt, track)) in self.tracks.iter_mut().zip(&song.tracks).enumerate() {
            let mut y = rt.instrument.process();
            for fx in &mut rt.fx {
                y = fx.process(y);
            }
            // `y·gain·pan` evaluates as `(y·gain)·pan`, as it always has.
            let y = rt.strip.duck(y) * track.gain;
            let l = y * track.pan_gains.0;
            let r = y * track.pan_gains.1;
            stem(t, l, r);
            if !track.mute && (!self.any_solo || track.solo) {
                ml += l;
                mr += r;
                let (send_delay, send_reverb) = rt.strip.sends();
                if has_delay {
                    delay_in += y * send_delay;
                }
                if has_reverb {
                    reverb_in += y * send_reverb;
                }
            }
        }
        self.mix.add_returns(&mut ml, &mut mr, delay_in, reverb_in);
        let (cl, cr) = self.mix.master(song.mix.master, &mut self.meter, ml, mr);
        self.pos += 1;
        (cl as f32, cr as f32)
    }

    /// Applies the pending swap at the current step boundary (see
    /// [`queue_swap`](Self::queue_swap)). Moves boxes and vectors only: it
    /// never allocates or frees.
    fn apply_swap(&mut self) {
        let Some(mut update) = self.pending.take() else {
            return;
        };
        let u = &mut *update;
        debug_assert!(u.spare.is_empty() && u.spare.capacity() >= u.song.tracks.len());
        for (j, new) in u.song.tracks.iter().enumerate() {
            let fresh = u.tracks[j].take().expect("one fresh runtime per track");
            let old = self.song.tracks.iter().position(|t| t.id == new.id);
            let rt = match old {
                None => fresh,
                Some(i) => {
                    let old = &self.song.tracks[i];
                    let mut kept =
                        std::mem::replace(&mut self.tracks[i], TrackRuntime::placeholder());
                    let mut unused = fresh;
                    if !same_device(&old.instrument, &new.instrument) {
                        std::mem::swap(&mut kept.instrument, &mut unused.instrument);
                        std::mem::swap(&mut kept.overrides, &mut unused.overrides);
                    }
                    // Deal every kept device into its new slot of the fresh
                    // chain, then take that chain; the old one goes back
                    // with the devices nobody kept.
                    for (slot, fx) in new.fx.iter().enumerate() {
                        if let Some(k) = old.fx.iter().position(|o| same_device(o, fx)) {
                            std::mem::swap(&mut unused.fx[slot], &mut kept.fx[k]);
                            std::mem::swap(
                                &mut unused.fx_overrides[slot],
                                &mut kept.fx_overrides[k],
                            );
                        }
                    }
                    std::mem::swap(&mut kept.fx, &mut unused.fx);
                    std::mem::swap(&mut kept.fx_overrides, &mut unused.fx_overrides);
                    u.tracks[j] = Some(unused);
                    kept
                }
            };
            u.spare.push(rt);
        }
        std::mem::swap(&mut self.tracks, &mut u.spare);
        if self.mix.take_units(&mut u.mix, &self.song, &u.song) {
            self.mix_swaps_kept += 1;
        }
        std::mem::swap(&mut self.song, &mut u.song);
        self.generation = u.generation;
        let clear = u.clear_overrides;
        self.retired = Some(update);
        self.swaps_applied += 1;
        self.overrides_active = self
            .tracks
            .iter()
            .map(TrackRuntime::overrides_set)
            .sum::<u32>()
            + self.mix.overrides_set();
        if clear {
            self.clear_overrides();
        }

        let song = &*self.song;
        self.any_solo = song.tracks.iter().any(|t| t.solo);
        self.gate_samples = song.clock.gate_samples();
        let total_steps = 16 * u64::from(song.total_bars);
        self.looping = self
            .looping
            .and_then(|(start, end)| (start < total_steps).then(|| (start, end.min(total_steps))));
        self.needs_static = true;
        if self.finished || self.step >= total_steps {
            match self.looping {
                Some((start, _)) if !self.finished => self.step = start,
                _ => {
                    self.finished = true;
                    self.step = total_steps;
                }
            }
        }
        self.pos = song.clock.step_start(self.step);
        self.region = region_of(song, self.step);
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

/// Index of the region containing `step` (the last one for steps past the
/// end), by binary search on region start steps.
fn region_of(song: &CompiledSong, step: u64) -> usize {
    song.regions
        .partition_point(|r| 16 * u64::from(r.start_bar) <= step)
        .saturating_sub(1)
}

/// The cell track `t` plays at `step` in `region`: `Rest` when no placement
/// of the track covers the region or the step lies past the end; otherwise
/// the clip looped from the placement's start, `offset_steps` in.
fn cell_at(song: &CompiledSong, region: usize, t: usize, step: u64) -> &Cell {
    if step >= 16 * u64::from(song.total_bars) {
        return &REST;
    }
    match song.regions[region].cells[t] {
        None => &REST,
        Some(source) => {
            let clip = &song.clips[source.clip];
            let offset = step - source.start_step + source.offset_steps;
            &clip[(offset % clip.len() as u64) as usize]
        }
    }
}

/// Applies every override of every track and of the mix.
fn apply_overrides(tracks: &mut [TrackRuntime], mix: &mut MixRuntime) {
    for rt in tracks {
        rt.apply_overrides();
    }
    mix.apply_overrides();
}

/// Applies every lane of `region` at song sample `pos`, each at its bar
/// offset from its own stretch's start.
fn apply_lanes(
    song: &CompiledSong,
    region: usize,
    pos: u64,
    tracks: &mut [TrackRuntime],
    mix: &mut MixRuntime,
) {
    let r = &song.regions[region];
    if r.lanes.is_empty() {
        return;
    }
    for lane in &r.lanes {
        let x = pos.saturating_sub(lane.start_sample) as f64 / (16.0 * song.clock.samples_per_step);
        let plain = lane.spec.to_plain(lane_value(&lane.points, x));
        match lane.target {
            ParamTarget::Mix { unit, index } => mix.set_target(unit, index, plain),
            target => tracks[lane.track].set_target(target, plain),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_is_send() {
        fn send<T: Send>() {}
        send::<Engine>();
        send::<TrackRuntime>();
        send::<Arc<CompiledSong>>();
    }
}
