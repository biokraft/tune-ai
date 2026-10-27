//! The sample-by-sample song player shared by offline and realtime output.
//!
//! [`Engine::process`] never allocates: everything is sized when the engine
//! is built. The order of work inside one sample is fixed (see
//! [`Engine::process`]), because offline and live playback must agree bit
//! for bit.

use std::ops::Range;
use std::sync::Arc;

use tune_dsp::acid303::AcidStep;
use tune_dsp::{ParamSpec, STRIP_PARAMS};
use tune_modules::{DRUM_PITCH, Effect, EventCtx, Instrument, NoteEvent, NoteId};
use tune_song::{TICKS_PER_STEP, lane_value};

use crate::clock::hit_seed;
use crate::compile::{
    Cell, CellSource, CompiledClip, CompiledDevice, CompiledSong, CompiledTrack, MIX_TRACK,
    NoteClipEvents, ParamTarget,
};
use crate::live::SongUpdate;
use crate::mixer::{Meter, MixRuntime, Strip};

/// How long a swap that changes a track's signal path crossfades that
/// track, in milliseconds: `(SWAP_FADE_MS * 0.001 * rate).round().max(1)`
/// frames (480 at 48 kHz).
pub const SWAP_FADE_MS: f64 = 10.0;

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
    /// What the instrument is sounding, from the engine's side: it travels
    /// with the instrument on a swap and is reset with it.
    seq: Sequencer,
    /// How this track plays the swap crossfade, set by every swap (see
    /// [`FadeRole`]).
    fade: FadeRole,
    /// A placeholder a swap left behind in the old list: it holds nothing
    /// and plays no fade-out.
    vacant: bool,
}

/// How a track of the new song plays a swap's crossfade, while
/// [`Engine::swap_fading`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FadeRole {
    /// The swap kept the signal path (or the track could not keep the old
    /// one running): it plays the new song only.
    #[default]
    None,
    /// The track is new: it fades in from silence.
    In,
    /// The swap changed the path: the old chain, in the retired update's
    /// fresh-runtime slot of this track, plays beside the new one and the
    /// output moves from it to the new one. With `own_instrument` the old
    /// chain plays the old instrument (kept in that slot); otherwise both
    /// chains share the kept instrument's sample.
    Cross { own_instrument: bool },
}

/// A swap crossfade in progress: `done` frames of `len`.
#[derive(Clone, Copy, Debug)]
struct SwapFade {
    done: usize,
    len: usize,
    /// Whether some track of the old song was soloed: what decides which
    /// fading-out tracks are audible.
    old_any_solo: bool,
}

/// The notes a track's instrument is sounding, from step clips and from
/// note clips.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Sequencer {
    /// A step note that slides into the next step: its Off follows that
    /// step's On.
    held: Option<(NoteId, u8)>,
    /// A step note that does not slide: its Off and the sample it lands on.
    /// Only step notes use this slot (see [`deliver_off`](Self::deliver_off)).
    off: Option<(u64, NoteId, u8)>,
    /// Where the last step boundary played a notes clip, `None` when it
    /// played none (or none has been played since a reset).
    last: Option<NotePos>,
    /// A note-clip On went out since the last `AllOff`: a cut (a placement
    /// end, a loop wrap, a source change) must end it.
    notes_live: bool,
    /// The note-clip events of the current step not yet delivered.
    cursor: Option<NoteCursor>,
}

/// Where a track plays a notes clip at one step boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NotePos {
    /// Index into `CompiledSong::clips`.
    clip: usize,
    /// The clip's length in steps.
    steps: u64,
    /// The clip step played, `0 .. steps`.
    clip_step: u64,
    /// The song step.
    step: u64,
}

impl NotePos {
    /// Where placement `src` of `song` plays at song `step`: `None` unless
    /// it plays a notes clip.
    fn of(song: &CompiledSong, src: Option<CellSource>, step: u64) -> Option<Self> {
        let src = src?;
        let Some(CompiledClip::Notes(clip)) = song.clips.get(src.clip) else {
            return None;
        };
        Some(Self {
            clip: src.clip,
            steps: clip.steps,
            clip_step: (step - src.start_step + src.offset_steps) % clip.steps,
            step,
        })
    }

    /// Whether `self` plays on from `was`: the same clip, one song step
    /// later, at the clip step `was` leads to (wrapping at the clip's end,
    /// as the clip loops). Placements do not matter: two adjacent
    /// placements whose offsets line up play on, as the two pieces of a
    /// split placement do (spec §7.5).
    fn follows(self, was: Self) -> bool {
        self.clip == was.clip
            && self.steps == was.steps
            && self.step == was.step.wrapping_add(1)
            && self.clip_step == (was.clip_step + 1) % self.steps
    }
}

/// The rest of one step's note-clip events: `events[next..end]` of clip
/// `clip`, in clip order.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NoteCursor {
    /// Index into `CompiledSong::clips`.
    clip: usize,
    next: usize,
    end: usize,
    /// The song step the events fall in.
    step: u64,
    /// The clip's tick at the step's start (`24 * clip step`).
    clip_tick: u32,
    /// `hit_seed(track seed, step)`.
    seed: u64,
}

impl Sequencer {
    /// The note id of a step's note.
    fn step_id(step: u64) -> NoteId {
        NoteId((step % (1 << 31)) as u32)
    }

    /// Delivers the scheduled Off once `pos` reaches it, in `step` of a
    /// track with noise seed `seed`. At a step start (`boundary`, before the
    /// step's events) it goes whatever its sample, so no step note outlasts
    /// the next step's start; with a gate of half a step, one never does.
    ///
    /// The forced delivery applies only to step notes: note-clip Offs never
    /// enter this slot. They wait in the `cursor` at their own samples, so a
    /// note-clip note may run for many steps; one only ends early through
    /// `AllOff` (see [`cut`](Self::cut)).
    fn deliver_off(
        &mut self,
        instrument: &mut dyn Instrument,
        pos: u64,
        step: u64,
        seed: u64,
        boundary: bool,
    ) {
        if let Some((at, id, pitch)) = self.off
            && (boundary || pos >= at)
        {
            let ctx = EventCtx {
                pos,
                step,
                seed: hit_seed(seed, step),
            };
            instrument.note(&NoteEvent::Off { id, pitch }, &ctx);
            self.off = None;
        }
    }

    /// Delivers the step events of `cell` (with `next` the cell of the step
    /// after it): an On for a note or a hit, then the Off of the note held
    /// into this step (a drum track never holds one), and schedules this
    /// note's own Off `gate_samples` later unless it slides into a note.
    /// Returns whether an On was delivered.
    fn step(
        &mut self,
        instrument: &mut dyn Instrument,
        cell: &Cell,
        next: &Cell,
        ctx: &EventCtx,
        gate_samples: u64,
    ) -> bool {
        let id = Self::step_id(ctx.step);
        let velocity = |accent: bool| if accent { 127 } else { 100 };
        match cell {
            &Cell::Note(AcidStep {
                note: Some(note),
                accent,
                slide,
            }) => {
                let pitch = note.min(127);
                let into = slide && matches!(next, Cell::Note(n) if n.note.is_some());
                let on = NoteEvent::On {
                    id,
                    pitch,
                    velocity: velocity(accent),
                    accent,
                    slide: into,
                };
                instrument.note(&on, ctx);
                self.release_held(instrument, ctx);
                if into {
                    self.held = Some((id, pitch));
                } else {
                    self.off = Some((ctx.pos + gate_samples, id, pitch));
                }
                true
            }
            &Cell::Hit { accent } => {
                let on = NoteEvent::On {
                    id,
                    pitch: DRUM_PITCH,
                    velocity: velocity(accent),
                    accent,
                    slide: false,
                };
                instrument.note(&on, ctx);
                self.release_held(instrument, ctx);
                self.off = Some((ctx.pos + gate_samples, id, DRUM_PITCH));
                true
            }
            Cell::Note(_) | Cell::Rest => {
                self.release_held(instrument, ctx);
                false
            }
        }
    }

    /// Ends the note held into this step, if any.
    fn release_held(&mut self, instrument: &mut dyn Instrument, ctx: &EventCtx) {
        if let Some((id, pitch)) = self.held.take() {
            instrument.note(&NoteEvent::Off { id, pitch }, ctx);
        }
    }

    /// At the start of step `ctx.step`, before its events: sends `AllOff`
    /// when a note-clip note may still sound and the track does not play
    /// on from the last step (see [`NotePos::follows`]): after a seek, a
    /// loop wrap, a placement's end, a jump to another clip or clip step,
    /// or a swap that ended the notes (see [`carry_over`](Self::carry_over)).
    /// Then remembers where placement `src` plays at this step and returns
    /// it, `None` unless `src` plays a notes clip. A track with no `last`
    /// never plays on. Events of the step before still in the cursor (none:
    /// every tick of a step starts before the next step) go first when due;
    /// the rest are dropped.
    fn cut(
        &mut self,
        instrument: &mut dyn Instrument,
        song: &CompiledSong,
        src: Option<CellSource>,
        ctx: &EventCtx,
    ) -> Option<NotePos> {
        self.deliver_notes(instrument, song, ctx.pos);
        self.cursor = None;
        let now = NotePos::of(song, src, ctx.step);
        let continuous = matches!((now, self.last), (Some(now), Some(was)) if now.follows(was));
        if !continuous && self.notes_live {
            instrument.note(&NoteEvent::AllOff, ctx);
            self.notes_live = false;
        }
        self.last = now;
        now
    }

    /// On a swap from `old` to `new` at step boundary `step`, for a track
    /// whose instrument (and so this sequencer) the swap keeps, as track
    /// `t` of `new`: held note-clip notes may play on only when the last
    /// step played a notes clip and the new song plays, at `step`, a clip
    /// with the same events (id, events and length; compared by content,
    /// since clip indices of two songs say nothing about each other). Then
    /// `last` and the cursor point into the new song's clip, and the cut at
    /// `step` decides as inside one song: the notes play on when the new
    /// song plays the clip step the old one led to, whatever placement
    /// plays it (a swap that splits a placement, moves it by a clip length
    /// or makes it longer keeps them). Otherwise `last` is `None`, so that
    /// cut sends `AllOff` if a note may sound, and the cursor is cleared,
    /// so it never reads the new song through an index into the old one.
    /// Comparisons only: never allocates.
    fn carry_over(&mut self, old: &CompiledSong, new: &CompiledSong, t: usize, step: u64) {
        let src = (step < 16 * u64::from(new.total_bars))
            .then(|| new.regions[region_of(new, step)].cells[t])
            .flatten();
        fn notes(song: &CompiledSong, clip: usize) -> Option<&NoteClipEvents> {
            match song.clips.get(clip) {
                Some(CompiledClip::Notes(clip)) => Some(clip),
                _ => None,
            }
        }
        let kept = match (self.last, NotePos::of(new, src, step)) {
            (Some(was), Some(now))
                if notes(old, was.clip).is_some_and(|a| notes(new, now.clip) == Some(a)) =>
            {
                Some(NotePos {
                    clip: now.clip,
                    ..was
                })
            }
            _ => None,
        };
        self.last = kept;
        self.cursor = match (kept, self.cursor) {
            (Some(now), Some(cur)) => Some(NoteCursor {
                clip: now.clip,
                ..cur
            }),
            _ => None,
        };
    }

    /// Queues the events of the notes clip step at `at` (of a track with
    /// noise seed `seed`) and delivers those landing on `pos`, the step's
    /// first sample. Returns whether an On was delivered.
    fn notes_step(
        &mut self,
        instrument: &mut dyn Instrument,
        song: &CompiledSong,
        at: NotePos,
        seed: u64,
        pos: u64,
    ) -> bool {
        let CompiledClip::Notes(clip) = &song.clips[at.clip] else {
            return false;
        };
        let cs_index = at.clip_step as usize;
        self.cursor = Some(NoteCursor {
            clip: at.clip,
            next: clip.step_starts[cs_index],
            end: clip.step_starts[cs_index + 1],
            step: at.step,
            clip_tick: (at.clip_step as u32) * TICKS_PER_STEP,
            seed: hit_seed(seed, at.step),
        });
        self.deliver_notes(instrument, song, pos)
    }

    /// Delivers, in order, every cursor event whose sample is `pos` (or
    /// earlier): event tick `t` of step `k` lands on
    /// `tick_start(24 * k + t - clip_tick)` with seed `hit_seed ^ (t -
    /// clip_tick)`. Returns whether an On was delivered; an On marks the
    /// notes live.
    fn deliver_notes(
        &mut self,
        instrument: &mut dyn Instrument,
        song: &CompiledSong,
        pos: u64,
    ) -> bool {
        let Some(mut cur) = self.cursor else {
            return false;
        };
        let Some(CompiledClip::Notes(clip)) = song.clips.get(cur.clip) else {
            self.cursor = None;
            return false;
        };
        let mut on = false;
        while cur.next < cur.end {
            let Some(ev) = clip.events.get(cur.next) else {
                break;
            };
            let in_step = ev.tick.saturating_sub(cur.clip_tick);
            let tick = u64::from(TICKS_PER_STEP) * cur.step + u64::from(in_step);
            if song.clock.tick_start(tick) > pos {
                break;
            }
            let ctx = EventCtx {
                pos,
                step: cur.step,
                seed: cur.seed ^ u64::from(in_step),
            };
            instrument.note(&ev.event, &ctx);
            on |= matches!(ev.event, NoteEvent::On { .. });
            cur.next += 1;
        }
        self.cursor = (cur.next < cur.end && cur.next < clip.events.len()).then_some(cur);
        self.notes_live |= on;
        on
    }
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
            seq: Sequencer::default(),
            fade: FadeRole::None,
            vacant: false,
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
        self.seq = Sequencer::default();
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
            seq: Sequencer::default(),
            fade: FadeRole::None,
            vacant: true,
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
    fn note(&mut self, _: &NoteEvent, _: &EventCtx) {}
    fn process(&mut self) -> f64 {
        0.0
    }
    fn reset(&mut self) {}
}

impl FadeRole {
    /// The new chain's output `y` (on instrument sample `x`) at crossfade
    /// weight `a`, for the track whose old path, if it has one, is `path`
    /// (see [`FadeRole`]). The old path gets no events, lanes or overrides.
    #[inline]
    fn blend(self, a: f64, x: f64, y: f64, path: Option<&mut Option<TrackRuntime>>) -> f64 {
        match (self, path.and_then(Option::as_mut)) {
            (Self::In, _) => a * y,
            (Self::Cross { own_instrument }, Some(path)) => {
                let mut y_old = if own_instrument {
                    path.instrument.process()
                } else {
                    x
                };
                for fx in &mut path.fx {
                    y_old = fx.process(y_old);
                }
                y_old + a * (y - y_old)
            }
            _ => y,
        }
    }
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
    /// How long a step note sounds unless it slides into the next one.
    gate_samples: u64,
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
    /// The crossfade of the last swap, while it runs (see
    /// [`swap_fading`](Self::swap_fading)).
    fade: Option<SwapFade>,
    /// Crossfade length in frames ([`SWAP_FADE_MS`]).
    fade_len: usize,
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
        let fade_len = fade_len(&song);
        Self {
            any_solo: song.tracks.iter().any(|t| t.solo),
            gate_samples: gate_samples(&song),
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
            fade: None,
            fade_len,
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
    /// - a track that keeps its instrument keeps its held note-clip notes
    ///   only when it plays at `k` a clip with the same events, at the clip
    ///   step the old song led to (whichever placement plays it, so a split
    ///   placement plays on); otherwise they get `AllOff` at `k` (see
    ///   `Sequencer::carry_over`);
    /// - a swap that changes a track's signal path (a device added, removed,
    ///   reordered or rebuilt, the instrument swapped, the track added or
    ///   removed) crossfades that track over [`SWAP_FADE_MS`]: the old path
    ///   keeps running beside the new one, from the same state (each kept
    ///   device copies itself into a stand-in, see
    ///   [`Effect::copy_state_to`]), with no events, lanes or overrides; an
    ///   added track fades in and a removed one fades out. A track with a
    ///   kept device that cannot copy its state switches at once. Value,
    ///   step and clip edits switch at the step, as before. The fade covers
    ///   the instrument and fx chain only: the blend feeds the track's
    ///   strip, so a structural swap that also changes the track's gain,
    ///   pan or a send (or its duck, mute or solo) switches those at once,
    ///   as a value swap does, and the old path plays at the new values;
    /// - the retired update, which holds the old paths, comes back from
    ///   [`take_retired_swap`](Self::take_retired_swap) only once the fade
    ///   is over, so the next swap waits for it;
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
    /// `None` while [`swap_fading`](Self::swap_fading): the old runtimes in
    /// it are still playing.
    pub fn take_retired_swap(&mut self) -> Option<Box<SongUpdate>> {
        if self.fade.is_some() {
            return None;
        }
        self.retired.take()
    }

    /// Whether a structural swap's crossfade is running (see
    /// [`queue_swap`](Self::queue_swap)). A seek or the song's end ends it.
    pub fn swap_fading(&self) -> bool {
        self.fade.is_some()
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
    /// and lanes and is applied every sample, before note events (see
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
        // Every runtime restarts from silence, so there is nothing to fade.
        self.fade = None;
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
    ///    region's lanes for this sample; apply the knob overrides; deliver
    ///    note events on every track in order (a step note's scheduled Off
    ///    still pending; `AllOff` when a note-clip note may sound and the
    ///    track does not play on from the previous step (the same notes
    ///    clip at the next clip step, whichever placement plays it), as
    ///    after a seek, a loop wrap or a placement's end;
    ///    then the step's: an On for a note or a hit, the Off of a note that
    ///    slid into this step, or the note-clip events of the step that
    ///    land on this sample, the rest waiting in the track's cursor); if
    ///    the duck source got an On (muted or not), trigger every track's
    ///    ducker; advance the step.
    ///    Elsewhere: apply the region's lanes, then the overrides, for this
    ///    sample, then deliver every track's scheduled step Off that lands
    ///    on it (a step note that does not slide ends `gate_samples` after
    ///    its On), then the cursor's note-clip events that land on it, in
    ///    order (event tick `t` of step `k` lands on
    ///    `clock.tick_start(24 * k + t)`); if the duck source got an On,
    ///    trigger every track's ducker. Lanes and overrides run exactly once
    ///    per sample (overrides only when one is set), always before the
    ///    note events, so a hit reads the overridden value.
    /// 3. For every track in song order: instrument, then fx in order, then
    ///    the ducker (`y`); `l, r = y·gain·pan`; write its stem; when audible
    ///    (mute and solo), add `l, r` to the master and `y·gain·send` to the
    ///    input of each built bus. While a swap crossfades (`a = done / len`
    ///    after `done += 1`), before the ducker: a track whose path changed
    ///    plays `y_old + a·(y − y_old)`, `y_old` being its old chain on the
    ///    old (or the shared) instrument sample, and an added track `a·y`.
    ///    Then each track the swap removed, in old song order: instrument,
    ///    fx, its own ducker, `·(1 − a)`, its old gain and pan, into the
    ///    master and the buses as it was audible in the old song (no stem).
    ///    The frame that reaches `done == len` ends the fade.
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
            self.fade = None;
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
                self.fade = None;
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
            let mut duck = false;
            for (t, (rt, track)) in self.tracks.iter_mut().zip(&song.tracks).enumerate() {
                let ctx = EventCtx {
                    pos: self.pos,
                    step,
                    seed: hit_seed(track.seed, step),
                };
                let instrument = rt.instrument.as_mut();
                // Forces only a step note's Off (see `deliver_off`).
                rt.seq
                    .deliver_off(instrument, self.pos, step, track.seed, true);
                let src = song.regions[region].cells[t];
                let at = rt.seq.cut(instrument, song, src, &ctx);
                // A notes clip's cell is `Rest`, so a step note before it
                // does not slide into it.
                let cell = cell_at(song, region, t, step);
                let next = cell_at(song, next_region, t, next_step);
                let mut on = rt.seq.step(instrument, cell, next, &ctx, self.gate_samples);
                if let Some(at) = at {
                    on |= rt
                        .seq
                        .notes_step(instrument, song, at, track.seed, self.pos);
                }
                duck |= on && song.mix.duck_source == Some(t);
            }
            if duck {
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
            // The step being played; the first sample after `new`, `seek`
            // or a swap is always a step start.
            let step = self.step.saturating_sub(1);
            let mut duck = false;
            for (t, (rt, track)) in self.tracks.iter_mut().zip(&song.tracks).enumerate() {
                let instrument = rt.instrument.as_mut();
                rt.seq
                    .deliver_off(instrument, self.pos, step, track.seed, false);
                let on = rt.seq.deliver_notes(instrument, song, self.pos);
                duck |= on && song.mix.duck_source == Some(t);
            }
            if duck {
                for rt in &mut self.tracks {
                    rt.strip.trigger_duck();
                }
            }
        }

        let (mut ml, mut mr) = (0.0, 0.0);
        let (mut delay_in, mut reverb_in) = (0.0, 0.0);
        let (has_delay, has_reverb) = (self.mix.has_delay(), self.mix.has_reverb());
        // A swap crossfade: this frame's weight of the new song, and the
        // retired update holding the old paths.
        let fade = self.fade.as_mut().map(|f| {
            f.done += 1;
            (f.done as f64 / f.len as f64, f.old_any_solo)
        });
        let mut old = match fade {
            Some(_) => self.retired.as_deref_mut(),
            None => None,
        };
        for (t, (rt, track)) in self.tracks.iter_mut().zip(&song.tracks).enumerate() {
            let x = rt.instrument.process();
            let mut y = x;
            for fx in &mut rt.fx {
                y = fx.process(y);
            }
            if let (Some((a, _)), Some(old)) = (fade, old.as_deref_mut()) {
                y = rt.fade.blend(a, x, y, old.tracks.get_mut(t));
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
        if let (Some((a, old_any_solo)), Some(old)) = (fade, old) {
            // Each old track the new song dropped fades out as it played.
            for (rt, track) in old.spare.iter_mut().zip(&old.song.tracks) {
                if rt.vacant {
                    continue;
                }
                let mut y = rt.instrument.process();
                for fx in &mut rt.fx {
                    y = fx.process(y);
                }
                let y = rt.strip.duck(y) * (1.0 - a) * track.gain;
                if !track.mute && (!old_any_solo || track.solo) {
                    ml += y * track.pan_gains.0;
                    mr += y * track.pan_gains.1;
                    let (send_delay, send_reverb) = rt.strip.sends();
                    if has_delay {
                        delay_in += y * send_delay;
                    }
                    if has_reverb {
                        reverb_in += y * send_reverb;
                    }
                }
            }
        }
        if self.fade.is_some_and(|f| f.done >= f.len) {
            self.fade = None;
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
                None => TrackRuntime {
                    fade: FadeRole::In,
                    ..fresh
                },
                Some(i) => {
                    let old = &self.song.tracks[i];
                    let mut kept =
                        std::mem::replace(&mut self.tracks[i], TrackRuntime::placeholder());
                    let mut unused = fresh;
                    let own_instrument = !same_device(&old.instrument, &new.instrument);
                    if own_instrument {
                        std::mem::swap(&mut kept.instrument, &mut unused.instrument);
                        std::mem::swap(&mut kept.overrides, &mut unused.overrides);
                        std::mem::swap(&mut kept.seq, &mut unused.seq);
                    } else {
                        // Held note-clip notes play on or end here.
                        kept.seq.carry_over(&self.song, &u.song, j, self.step);
                    }
                    let path_changed = own_instrument
                        || old.fx.len() != new.fx.len()
                        || old.fx.iter().zip(&new.fx).any(|(o, n)| !same_device(o, n));
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
                    // `unused` now holds the old chain in old order, with a
                    // fresh stand-in where a kept device was: each kept
                    // device copies itself into its stand-in, so the old
                    // path plays on from the same state. One that cannot
                    // copy makes the track switch at once.
                    let copied = path_changed
                        && new.fx.iter().enumerate().all(|(slot, fx)| {
                            match old.fx.iter().position(|o| same_device(o, fx)) {
                                Some(k) => kept.fx[slot].copy_state_to(unused.fx[k].as_mut()),
                                None => true,
                            }
                        });
                    kept.fade = if copied {
                        FadeRole::Cross { own_instrument }
                    } else {
                        FadeRole::None
                    };
                    u.tracks[j] = Some(unused);
                    kept
                }
            };
            u.spare.push(rt);
        }
        std::mem::swap(&mut self.tracks, &mut u.spare);
        // `u.spare` is the old list: placeholders, and the runtimes of the
        // tracks the new song dropped, which fade out.
        let fades = u.spare.iter().any(|rt| !rt.vacant)
            || self.tracks.iter().any(|rt| rt.fade != FadeRole::None);
        let old_any_solo = self.any_solo;
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
        self.gate_samples = gate_samples(song);
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
        // A finished engine plays silence: nothing to fade.
        self.fade = (fades && !self.finished).then_some(SwapFade {
            done: 0,
            len: self.fade_len,
            old_any_solo,
        });
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

/// The swap crossfade's length in frames at the song's rate: 480 at 48 kHz.
fn fade_len(song: &CompiledSong) -> usize {
    (SWAP_FADE_MS * 0.001 * f64::from(song.clock.sample_rate))
        .round()
        .max(1.0) as usize
}

/// The song's gate length in samples (half a step).
fn gate_samples(song: &CompiledSong) -> u64 {
    u64::try_from(song.clock.gate_samples()).unwrap_or(0)
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
            match &song.clips[source.clip] {
                CompiledClip::Steps(clip) => {
                    let offset = step - source.start_step + source.offset_steps;
                    &clip[(offset % clip.len() as u64) as usize]
                }
                // A notes clip plays events, not cells (see
                // `Sequencer::notes_step`).
                CompiledClip::Notes(_) => &REST,
            }
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
