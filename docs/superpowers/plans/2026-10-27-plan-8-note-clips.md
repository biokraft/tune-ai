# Plan 8: Note Clips and the Note-Event API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drive every instrument by sample-accurate note events, add note clips (pitch, start, length, velocity on a fine grid) to the clip pool with their format, validation, engine playback and CLI, and crossfade a track over 10 ms when a hot reload changes its signal path, while every existing song renders bit for bit as before and the current studio keeps working.

**Architecture:** The instrument trait loses `on_step(&Cell, &StepCtx)` and gains `note(&NoteEvent, &EventCtx)`; the engine turns 303 and drum steps into events with exactly today's timing (On at the step, Off after the gate unless the step slides into a note), so the acid303 and drum modules make today's samples. Note clips are a third clip kind in `tune-song`, compiled into per-clip event lists that the engine delivers at their own samples, with an all-notes-off whenever a clip is cut. A structural swap keeps the old track runtime running for 10 ms beside the new one and blends them; a kept effect copies its state into the stand-in the old path uses, so kept devices still keep their state.

**Tech Stack:** Rust 2024 (workspace, rustc 1.97: trait upcasting to `dyn Any` is stable), `toml` =1.1.2, `toml_edit` =0.25.17, `indexmap` 2.14, `rtrb` =0.3.4, React 18 + TypeScript + Vitest in `apps/desktop` and `packages/ui`. No new third-party dependencies.

**Spec:** `docs/superpowers/specs/2026-10-26-phase-2-modular-model-design.md` (the authority). This is the second of four Phase 2 plans. It covers §7.3 (instruments and note events, clip compatibility for note clips), §7.4 (note clips), the 10 ms structural-swap crossfade of §7.1, and the parts of §9 and §10 these touch. Out of scope, for later plans: the instrument-swap action with its clean-up (§7.3, an action: Plan 9), every other action, `tune actions`/`tune do`, generated TS types, structured undo, runs and the ⌘Z toast (§8, Plan 9); scenes, slots and session launches (§7.6, Plan 10). Nothing here needs Plan 9 first: the CLI edits note clips through `tune set` paths, which Plan 9 turns into actions. The spec's "device reset" swap kind has no edit that produces it until Plan 9; when one exists it is a device rebuilt with the same identity, which Task 5's rule already treats as a structural change.

## Global Constraints

- Run `task verify` before every commit. During TDD run the one test file: `cargo test -p <crate> --test <name>`.
- Every commit message is a Conventional Commit in normal prose and ends with exactly these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NTWqQ3vUQ6Q116sMFSQZbt
  ```
- "v1 songs sound different" is a failure: every existing song and fixture renders bit for bit as before. The constants in `crates/tune-engine/tests/baseline.rs` are never changed and its four tests stay byte-identical. Run `cargo test -p tune-engine --test baseline` before and after every engine change.
- Frozen fixtures: `crates/tune-song/tests/fixtures/{mini,mixed,first-acid-v1,first-acid-mixed}.toml` and `crates/tune-song/tests/fixtures/v2/*.toml` are never edited. Never edit `songs/*/song.toml` (no task here changes a song).
- Determinism: same inputs give bitwise-identical output; seeds come from `track_seed`/`hit_seed` only.
- Hot reload: swaps land at a step boundary, keep the playhead, keep state for anything that did not change, and allocate nothing on the audio thread. `crates/tune-engine/tests/live_no_alloc.rs` keeps passing and covers every new swap kind (note-clip edits, crossfading swaps). Offline renders never swap, so the crossfade never touches them.
- Crossfade (spec §7.1, values verbatim): a swap that changes a track's signal path ("a device added, removed, reordered, swapped or reset, a distortion mode change, a track added or removed") crossfades that track "over 10 ms"; "the old runtime keeps running for the fade and then goes back to the garbage queue"; pass/fail "on a steady test signal: the largest sample-to-sample jump in the 20 ms after the swap is no bigger than 1.5 times the largest in the 100 ms before it". A swap that changes only values, steps or clips does not crossfade.
- Note grid (spec §7.4): at least 1/96 of a bar. This plan uses 24 ticks per 16th step (1/384 bar).
- Device I/O stays mono: instrument → fx → duck → gain → pan as today.
- `tune-dsp`, `tune-modules`, `tune-song` and `tune-engine` stay free of Tauri, UI and network dependencies; `tune-modules` depends only on `tune-dsp`. `crates/tune-dsp/src/acid303/` is not edited.
- The registry ships today's modules only. No new UI beyond spec §10's read-only fallback for note clips ("note clips show as blocks without a step editor").
- The song file stays TOML, hand-editable and hot-reloadable; every new error carries the line and column of the user's text. v2 stays LLM-friendly: short keys, no generated ids.
- Never loosen the Open303 parity tolerances. No GPL code. No audio, renders, screenshots, `scratch/`, absolute home paths or personal data in git. Never read or print `tune.local.toml`. Any audio command uses `--device null`. Never use bare `git stash`.

## Review Focus

1. **A note-clip note cut before its end** by a placement that ends mid-note, the transport loop wrapping, or a seek: nothing hangs; the instrument gets `AllOff` at the cut and is silent after its release. (Task 3 `a_placement_ending_mid_note_sends_all_off`, `loop_wrap_sends_all_off`, `seek_mid_note_is_silent_after`.)
2. **Off-grid positions written by hand**: `start = 0.333` reads as a third of a step, `start = 0.3` is refused with a positioned message, and at 145 BPM (a fractional samples-per-step) a note at step 0.5 lands on sample 2483. (Task 2 `note_clip_rules`, `a_note_clip_parses`; Task 3 `note_events_land_on_their_samples`.)
3. **Claude hand-edits a note clip while a long note sounds**: a swap that leaves that track's clip and placement unchanged keeps the note; one that changes them ends it with `AllOff` at the swap step, never a stuck note. (Task 4 `a_value_swap_keeps_a_held_note`, `a_clip_edit_ends_held_notes`.)
4. **A structural reload while a fade still runs, or on a device that cannot copy its state**: the second swap waits until the fade ends and its garbage is collected; a track with such a device switches at once (Plan 7 behaviour) instead of crashing. (Task 5 `a_swap_during_a_fade_waits_for_it`, `a_device_that_cannot_copy_switches_at_once`.)
5. **Chords and the duck source on a note clip**: two notes starting together on a drum voice give two hits; on acid303 the later one slides; a note-clip hit on the duck source ducks the other tracks at its own sample. (Task 3 `chords_hit_once_per_note`, `note_clip_hits_drive_the_ducker`.)

---

## Decided formats and interfaces

Every name here is binding for all tasks.

### The note-event API (`tune-modules/src/instrument.rs`)

```rust
/// One note from its On to its Off; unique among the notes sounding on a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteId(pub u32);

/// The pitch drum steps carry (C4); drum modules ignore pitch.
pub const DRUM_PITCH: u8 = 60;
/// A note-clip note at or above this velocity is accented.
pub const ACCENT_VELOCITY: u8 = 110;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteEvent {
    /// `slide`: the engine sends the next note's On before this note's Off
    /// (a 303 slide or tie, or a note-clip note that overlaps a later one).
    On { id: NoteId, pitch: u8, velocity: u8, accent: bool, slide: bool },
    /// Ends note `id`. An instrument ignores an Off for a note it is not sounding.
    Off { id: NoteId, pitch: u8 },
    /// Ends every sounding note: a placement end, a loop wrap or a swap cut a note clip.
    AllOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCtx {
    /// Song sample the event lands on; it is delivered just before `process` of that sample.
    pub pos: u64,
    /// The song step the event falls in.
    pub step: u64,
    /// `hit_seed(track seed, step)`, xor the event's tick within the step (0 on the grid).
    pub seed: u64,
}

pub trait Instrument: Send {            // params, set_param_at, set_param, process, reset unchanged
    fn note(&mut self, event: &NoteEvent, ctx: &EventCtx);   // replaces on_step
}
pub trait Effect: Send + Any {          // params, set_param_at, set_param, process, reset unchanged
    /// Copies this effect's whole DSP state into `to`, a device of the same module, without
    /// allocating. False (the default) when `to` is another module or this one cannot.
    fn copy_state_to(&self, _to: &mut dyn Effect) -> bool { false }
}
/// `copy_state_to` for a module whose state is plain data: `to.clone_from(from)` after a
/// downcast of `to` (via `&mut dyn Any`) to `T`.
pub fn copy_state<T: Effect + Clone>(from: &T, to: &mut dyn Effect) -> bool;
```

`Cell` and `StepCtx` leave `tune-modules`. `Cell` becomes engine-internal (`tune_engine::Cell`, same variants); `StepCtx` is deleted.

Ids: a step note's id is `NoteId((step % 2^31) as u32)`; a note-clip note's id is `NoteId(NOTE_CLIP_IDS | index)` with `pub const NOTE_CLIP_IDS: u32 = 0x8000_0000` (tune-engine) and `index` its position in the clip.

Module behaviour:
- `acid303` (`AcidTrack`: the voice plus `sounding: Option<NoteId>`, no `AcidGate`): On → `slide_to_note(pitch, accent)` when a note sounds, else `trigger_note(pitch, accent)`; then `sounding = Some(id)`. Off → `release_note()` only when `sounding == Some(id)`, then `None`. AllOff → `release_note()` when a note sounds. `process` → `voice.get_sample()`. `reset` → voice reset, `sounding = None`.
- Drum modules: On → `trigger(accent, ctx.seed)`; Off and AllOff ignored.
- Every instrument plays note clips: `ModuleDescriptor::plays(ClipKind::Notes)` is true for every instrument whatever its `plays` list holds (spec: "note clips play on any instrument").

### Note clips (song format v2, additive)

```toml
[clips.lead]
kind = "notes"
bars = 1                      # the clip's length in whole bars, 1 to 64
notes = [                     # optional; no key = an empty clip
  { start = 0, pitch = "C3", len = 3 },              # start and len in 16th steps
  { start = 0.333, pitch = "G3", len = 0.5 },         # 0.333 reads as a third of a step
  { start = 2.5, pitch = "D#3", len = 2, vel = 120 }, # vel 1 to 127, default 100
]
```

Rules:
- `start` and `len` are numbers in 16th steps on a grid of `TICKS_PER_STEP = 24` ticks; a value within 0.001 step of a multiple of 1/24 is that multiple, anything else is an error. `start >= 0`, `len > 0`, and `start + len` may not pass the clip's end (`16 * bars` steps).
- Notes are in time order (`start` never decreases); overlaps and chords are allowed.
- `pitch` is a note name as in acid notation without suffixes (`C3` = MIDI 48, `D#3`, `Bb1`, `C-1` to `G9`); the writer spells sharps.
- A notes clip has no `steps`; acid and drum clips have no `bars` or `notes`.
- Canonical writer: `kind`, `bars`, then `notes` only when non-empty. Each note is `{ start = <s>, pitch = "<name>", len = <s>[, vel = <v>] }`, `vel` only when not 100, positions printed as `ticks / 24` with the shortest round-trip form (tick 8 prints `0.3333333333333333`). One note is written inline (`notes = [{ start = 0, pitch = "C3", len = 3 }]`), two or more one per line with a two-space indent and a trailing comma, as `placements_toml` does.
- Accents: a note-clip note with `vel >= ACCENT_VELOCITY` carries `accent: true`. A 303 or drum step carries velocity 127 when accented, else 100.

## File structure

- `crates/tune-modules/src/instrument.rs`: the event types and traits above. `src/builtin.rs`: the event-driven `AcidTrack`, drums, `copy_state_to` for `Distortion` and `Equalizer`. `src/descriptor.rs`: `ClipKind::Notes`. New `tests/events.rs`.
- `crates/tune-song/src/model.rs`: `Note`, `NoteClip`, `Clip::Notes`, `TICKS_PER_STEP`, pitch helpers. `src/raw/mod.rs`, `src/validate/arrange.rs`, `src/write.rs`: reading, checking and writing note clips. New `tests/notes.rs`.
- `crates/tune-engine/src/compile.rs`: `Cell`, `CompiledClip`, `NoteClipEvents`. `src/engine.rs`: the per-track sequencer (step and note-clip events), the swap rules and the crossfade. `src/clock.rs`: `tick_start`. New `tests/notes.rs`, `tests/crossfade.rs`.
- `crates/tune-studio/src/{path.rs,doc.rs,diff.rs,set.rs,view.rs}`: note-clip paths and views. `crates/tune-cli/src/cmd/{info.rs,params.rs}`: clips and what instruments play.
- `apps/desktop/src/{client/protocol.ts,lib/song.ts,lib/describeChange.ts,screens/dock/StepsTab.tsx,screens/StudioScreen.tsx}`, `packages/ui/src/components/Arrangement.tsx`: note clips shown read-only.

---

### Task 1: Instruments receive note events (RISKY: bitwise)

**Risk.** Every acid and drum sample now comes from events instead of cells. The pins must stay bitwise: the voice calls (`trigger_note`, `slide_to_note`, `release_note`, `trigger`) must happen on the same samples, in the same order relative to `set_param_at` and `get_sample`, as today. Run `cargo test -p tune-engine --test baseline` before touching anything.

**Files:**
- Modify: `crates/tune-modules/src/instrument.rs:1-69`, `crates/tune-modules/src/builtin.rs:13-118`, `crates/tune-modules/src/lib.rs:11-13`, `crates/tune-modules/tests/registry.rs:1-90` (`factories_build_their_voices`)
- Create: `crates/tune-modules/tests/events.rs`
- Modify: `crates/tune-engine/src/compile.rs` (define `Cell` here), `crates/tune-engine/src/engine.rs:20-212,536-633,638-716`, `crates/tune-engine/src/lib.rs:16-24`
- Modify: `crates/tune-engine/tests/common/mod.rs:111-242` (Probe), `crates/tune-engine/tests/{engine.rs,arrangement.rs,overrides.rs,live.rs,registry.rs,tracks.rs}`
- Modify: `CLAUDE.md` (Layout: `tune-modules` is "instrument and effect traits (instruments receive note events), today's modules and the registry")

**Interfaces:**
- Produces: the note-event API of "Decided formats and interfaces" except `copy_state_to`/`copy_state` (Task 5). `tune_engine` re-exports `NoteEvent, NoteId, EventCtx, Instrument, Effect, AcidTrack` from `tune_modules` and exports its own `Cell`.
- The engine's per-track sequencer state lives in `TrackRuntime` (a private field, reset by `TrackRuntime::reset` and by building), travels with the track on a swap only when the instrument is kept (it describes what the instrument is sounding), and is reset when the instrument is replaced. Step events, determined here:
  ```text
  at step boundary k, per track in song order, after lanes and overrides (where on_step ran):
    cell = cell_at(region, t, k); next = cell_at(next_region, t, next_step)   // as today
    ctx  = EventCtx { pos, step: k, seed: hit_seed(track.seed, k) }
    Note(s), pitch p:  into = s.slide && next is Note
                       deliver On { id(k), p, velocity: if s.accent {127} else {100}, accent: s.accent, slide: into }
                       if held = Some((h, hp)): deliver Off { h, hp }; held = None
                       if into { held = Some((id(k), p)) } else { schedule Off { id(k), p } at pos + gate_samples }
    Hit { accent }:    deliver On { id(k), DRUM_PITCH, 127 or 100, accent, slide: false }; schedule Off at pos + gate_samples
    Rest:              if held = Some((h, hp)): deliver Off { h, hp }; held = None
    duck: when t is the duck source and an On was delivered, trigger every track's ducker
  on every other sample (after lanes and overrides): deliver the scheduled Off when pos reaches it,
    ctx.step = the step being played, ctx.seed = hit_seed(track.seed, that step)
  ```
  `Engine::frame`'s doc comment (`engine.rs:481-505`) says "deliver note events" where it said "fire `on_step`".
- Probe log (tests/common): `Event::Step` becomes `Event::Note { sample: u64, event: NoteEvent, ctx: EventCtx }`; `Log::steps()` becomes `Log::notes() -> Vec<(u64, NoteEvent, EventCtx)>`, plus `Log::ons()` and `Log::offs()` (the same, filtered to `On`/`Off`). Port rule for existing tests: a step that had a `Cell::Hit` or `Cell::Note` now has one `On` at the same sample with `ctx.step == k`; rests have no event; `ctx.next_has_gate` of a slide step is now that step's `On { slide }`; `ctx.gate_samples` assertions become "the step's Off lands `gate_samples` after its On". Tests keep their names and intent.

- [ ] **Step 1: Run the pins first:** `cargo test -p tune-engine --test baseline` — Expected: PASS (the starting point).
- [ ] **Step 2: Write the failing tests.**
  - `tune-modules/tests/events.rs::acid_track_follows_the_gate_bitwise`: 100 random 16-step acid patterns (splitmix64 in the test, seed 1: rests, accents, slides, ties) at 120 and 145 BPM, 48 kHz, 4 loops each. Reference: an `Acid303::with_param_defaults` plus `AcidGate` driven in today's order (per sample `gate.tick`, then the step's `gate.on_step(step, next_has_gate, gate_samples)` at a step start, then `get_sample`), written in the test. Under test: `ACID303.make_instrument(sr, &[])` driven by the step events above (the mapping written in the test, ~20 lines). Every sample equal bitwise.
  - `events.rs::off_for_another_note_is_ignored`: acid303 On(id 1) at 0, Off(id 2) at 3000: the RMS of samples 6000..9000 is above 0.1× that of 0..3000; with Off(id 1) instead it is below 0.01×. `all_off_releases`: On(1) then AllOff at 3000 gives the released result.
  - `events.rs::drums_hit_on_note_on_and_ignore_offs`: each drum module driven by On at 0 (seed 5) equals, bitwise over 9600 samples, the same module driven by On at 0 plus Off at 100 and AllOff at 200.
  - `registry.rs::factories_build_their_voices`: the kick gets `note(&On { id: NoteId(0), pitch: DRUM_PITCH, velocity: 100, accent: false, slide: false }, &ctx)`.
  - `tune-engine/tests/engine.rs::step_notes_end_after_the_gate`: in `slide_song`, step 0's Off lands at `step_start(0) + gate_samples`; step 15 (`C2~` into a note) has no Off before step 16's On, and its `Off` comes right after step 16's `On` at the same sample.
  - Port every test listed under Files to the probe log above.
- [ ] **Step 3: Run** `cargo test -p tune-modules --test events` — Expected: FAIL (no `note`, no `NoteEvent`).
- [ ] **Step 4: Implement** the API, `AcidTrack`, the drums, the `Vacant` placeholder's `note`, the engine's step events and the test ports. Keep `Engine::frame`'s float expressions and per-sample order as they are apart from event delivery.
- [ ] **Step 5: Run** `cargo test -p tune-engine --test baseline` — Expected: PASS with the unchanged constants. Then `cargo test --workspace` — Expected: PASS.
- [ ] **Step 6: Run** `task verify`, then commit `feat: drive instruments by note events`.

### Task 2: Note clips in the song file

**Files:**
- Modify: `crates/tune-modules/src/descriptor.rs:10-35,88-94` (`ClipKind::Notes`, name `"notes"`, `ALL: [Self; 3]` = `[Acid, Drum, Notes]`; `plays(Notes)` true for every instrument), `crates/tune-modules/tests/registry.rs` (`clip_kinds_and_duck_default`)
- Modify: `crates/tune-song/src/model.rs:308-334`, `crates/tune-song/src/lib.rs:31-37`, `crates/tune-song/src/raw/mod.rs:89-92,238-253,364`, `crates/tune-song/src/validate/arrange.rs:112-138,219-259`, `crates/tune-song/src/write.rs:60-64`
- Modify: `crates/tune-engine/src/compile.rs:14-29,184-206` (`CompiledClip`), `crates/tune-engine/src/engine.rs:767-779` (`cell_at`)
- Modify: `crates/tune-studio/src/view.rs:222-243` (leave note clips out of `patterns` until Task 7, with a comment saying so), `crates/tune-studio/src/core.rs:724-747` (a note clip is "not an acid pattern" / "not a drum pattern"), `crates/tune-studio/src/doc.rs:361`, `crates/tune-studio/tests/fixtures/protocol.json` (regenerated: every `plays` gains `"notes"`)
- Create: `crates/tune-song/tests/notes.rs`
- Modify: `crates/tune-song/tests/{roundtrip.rs,parse_v2.rs:299-316}`, `crates/tune-engine/src/compile.rs` (unit tests), `CLAUDE.md` (Notation: the note-clip bullet with the example and rules of "Note clips")

**Interfaces:**
- Consumes: `NoteEvent`, `NoteId`, `ACCENT_VELOCITY` (Task 1).
- Produces:
  ```rust
  // tune-song
  pub const TICKS_PER_STEP: u32 = 24;
  pub const DEFAULT_VELOCITY: u8 = 100;
  pub const NOTE_CLIP_BARS: RangeInclusive<u32> = 1..=64;
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Note { pub start: u32 /* ticks from the clip start */, pub len: u32 /* ticks, >= 1 */, pub pitch: u8, pub velocity: u8 /* 1..=127 */ }
  impl Note { pub fn end(&self) -> u32; }                                // start + len
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct NoteClip { pub bars: u32, pub notes: Vec<Note> /* time order, each ending by the clip end */ }
  impl NoteClip { pub fn steps(&self) -> u32; }                          // 16 * bars
  pub enum Clip { Acid(AcidPattern), Drum(DrumPattern), Notes(NoteClip) }
  impl Clip {
      pub fn kind(&self) -> ClipKind;
      pub fn steps_text(&self) -> Option<String>;   // None for a notes clip (was String)
      pub fn steps(&self) -> u32;                   // the clip's length in steps, every kind
  }
  pub fn parse_pitch(name: &str) -> Option<u8>;    // one acid note token without suffixes
  pub fn pitch_name(pitch: u8) -> String;          // as acid notation prints it: 51 -> "D#3"
  pub fn notes_toml(notes: &[Note]) -> String;     // the `notes` value, layout as decided
  // tune-engine
  pub enum CompiledClip { Steps(Vec<Cell>), Notes(NoteClipEvents) }    // CompiledSong::clips: Vec<CompiledClip>
  #[derive(Clone, Debug, PartialEq)]
  pub struct NoteClipEvents { pub id: String, pub steps: u64, pub events: Vec<ClipEvent>, pub step_starts: Vec<usize> /* len steps + 1 */ }
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct ClipEvent { pub tick: u32 /* 0 .. 24 * steps */, pub event: NoteEvent }
  pub const NOTE_CLIP_IDS: u32 = 0x8000_0000;
  ```
- Raw reading: a clip's keys are read by its `kind` value. `notes`: allowed `kind, bars, notes`, `bars` required (`missing field \`bars\` in [clips.lead]`), hint on `steps` "a notes clip has no `steps`: its length is `bars` and its notes are `notes`". `acid`/`drum`: allowed `kind, steps` as today, hints on `bars` and `notes` "`bars` and `notes` are for a notes clip (kind = \"notes\")". An unknown kind allows all four keys. Note entries: `tables_in_array`, keys `start, pitch, len, vel`, `start`/`len` `Kind::Num` and required, `pitch` `Kind::Str` required, `vel` `Kind::Int` optional.
- Validation messages (each at the offending value; "note N" is 1-based):
  - "unknown clip kind `chords` (expected acid, drum or notes)"
  - "a notes clip has 1 to 64 bars, got 0"
  - "note 2 of clip `lead`: `start` 0.3 is off the grid (positions are 16th steps in multiples of 1/24, e.g. 0.5, 0.25 or 0.333)" (same for `len`)
  - "note 2 of clip `lead`: `start` must be at least 0, got -1"; "note 2 of clip `lead`: `len` must be more than 0, got 0"
  - "note 2 of clip `lead` ends at step 17, past the clip's end (step 16)" (at `len`; steps printed with the writer's number form)
  - "notes of clip `lead` must be in time order: note 3 starts before note 2" (at note 3's `start`)
  - "note 2 of clip `lead`: unknown pitch `H2` (expected a note such as `C3`, `D#2` or `Bb1`, from C-1 to G9)"
  - "note 2 of clip `lead`: `vel` must be 1 to 127, got 0"
- Compile (determined here): each note `i` gives `On { NoteId(NOTE_CLIP_IDS | i), pitch, velocity, accent: velocity >= ACCENT_VELOCITY, slide: some later note starts inside (start, end) }` at `tick = start` and `Off` at `tick = end`, an Off at the clip's end moved to tick 0. Events sort by tick, then Offs before Ons, then note index. `step_starts[s]` is the first event with `tick >= 24 * s`. Until Task 3 the engine plays nothing from a notes clip: `cell_at` returns `Rest` for one (its final step-path behaviour; Task 3 adds the events).

- [ ] **Step 1: Write the failing tests** in `tests/notes.rs` (helpers `parsed`, `fails_at` as in `parse_v2.rs`, on a v2 song with an acid track playing `[clips.lead]`):
  - `a_note_clip_parses`: the example above gives `NoteClip { bars: 1, notes: vec![Note { start: 0, len: 72, pitch: 48, velocity: 100 }, Note { start: 8, len: 12, pitch: 55, velocity: 100 }, Note { start: 60, len: 48, pitch: 51, velocity: 120 }] }`.
  - `note_clip_rules`: one `fails_at` per message above, each with its line and column; plus the two hint messages.
  - `every_instrument_plays_note_clips`: a notes clip placed on a `kick`, an `acid303` and on a test-only instrument registered with `plays: &[ClipKind::Drum]` (via `parse_with`) parses; `KICK.plays(ClipKind::Notes)` and `ACID303.plays(ClipKind::Notes)`.
  - `pitch_names_round_trip`: `parse_pitch("C3") == Some(48)`, `parse_pitch("Bb1") == Some(34)`, `parse_pitch("C3!")`, `parse_pitch(".")` and `parse_pitch("H2")` are `None`; `pitch_name(51) == "D#3"`, `pitch_name(0) == "C-1"`.
  - `roundtrip.rs::note_clips_round_trip`: a song with the example clip writes exactly `kind = "notes"\nbars = 1\nnotes = [\n  { start = 0, pitch = "C3", len = 3 },\n  { start = 0.3333333333333333, pitch = "G3", len = 0.5 },\n  { start = 2.5, pitch = "D#3", len = 2, vel = 120 },\n]` under `[clips.lead]` and parses back equal; `notes_toml` of one note is one line.
  - `parse_v2.rs::clip_rules`: the kind case uses `kind = "chords"` and the new message.
  - `compile.rs::note_clips_become_events`: clip `bars = 1`, notes `{0, C3, 4}`, `{2.5, D#3, 2, vel 120}`, `{14, G3, 2}` gives events `(tick, event)` in this order: `(0, Off G3 #2)`, `(0, On C3 #0 vel 100 accent false slide true)`, `(60, On D#3 #1 accent true slide false)`, `(96, Off C3 #0)`, `(108, Off D#3 #1)`, `(336, On G3 #2)`, and `step_starts == [0, 2, 2, 3, 3, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 6, 6]`.
  - `registry.rs::clip_kinds_and_duck_default`: `ClipKind::ALL` names `["acid", "drum", "notes"]`.
- [ ] **Step 2: Run** `cargo test -p tune-song --test notes` — Expected: FAIL.
- [ ] **Step 3: Implement** the kind, the model, reading, validation, writing, the compile and every consumer arm; regenerate the golden with `UPDATE_GOLDEN=1 cargo test -p tune-studio --test protocol` and review its diff (only `"notes"` added to `plays`); add the CLAUDE.md notation bullet.
- [ ] **Step 4: Run** `cargo test --workspace && pnpm -r test` — Expected: PASS, `baseline.rs` unchanged.
- [ ] **Step 5: Run** `task verify`, then commit `feat(song): add note clips to the song format`.

### Task 3: The engine plays note clips

**Files:**
- Modify: `crates/tune-engine/src/clock.rs` (`tick_start` and its pin test), `crates/tune-engine/src/engine.rs` (sequencer: note-clip cursor, `AllOff`, duck, seeds; `frame` doc)
- Create: `crates/tune-engine/tests/notes.rs`
- Modify: `crates/tune-engine/tests/no_panic.rs` (note clips in the generator)

**Interfaces:**
- Consumes: `CompiledClip`, `NoteClipEvents`, `ClipEvent`, `TICKS_PER_STEP` (Task 2); the sequencer and step events (Task 1).
- Produces: `impl Clock { pub fn tick_start(&self, tick: u64) -> u64 }` = `((tick as f64 / f64::from(TICKS_PER_STEP)) * self.samples_per_step).round() as u64`, so `tick_start(24 * k) == step_start(k)` exactly.
- Delivery (determined here; it extends Task 1's step boundary):
  ```text
  at step boundary k, per track t, before the step events:
    src = the region's CellSource for t (None when no placement)
    continuous = seq.last == src.map(|s| (s, k - 1))   // false after a seek, a loop wrap or a source change
    if !continuous && seq.notes_live: deliver AllOff; notes_live = false
    if src plays a notes clip c:
      cs = (k - src.start_step + src.offset_steps) % c.steps
      the step's events: c.events[c.step_starts[cs] .. c.step_starts[cs + 1]], each at
        tick_start(24 * k + (ev.tick - 24 * cs)), ctx { pos, step: k, seed: hit_seed(track.seed, k) ^ (ev.tick - 24 * cs) }
      deliver now the ones whose sample is pos; keep the rest as the track's cursor; an On sets notes_live
    seq.last = src.map(|s| (s, k))
  on every other sample: deliver the step Off when due, then every cursor event whose sample is pos, in order
  the duck source: every delivered On (step or note clip) triggers every track's ducker at its sample
  a step note whose next step is in a notes clip does not slide into it (next is treated as Rest)
  ```
  Notes that started before a placement's first step (an offset into the clip) do not sound; their Offs reach the instrument and are ignored there.

- [ ] **Step 1: Write the failing tests** in `tests/notes.rs` (probe songs are v2 texts with one track `t`; probe via `probe_engine`):
  - `clock.rs::tick_start_pinned`: at 145 BPM, 48 kHz: `tick_start(12) == 2483`, `tick_start(24) == 4966`, `tick_start(36) == 7448`, `tick_start(396) == 81931`; for `k` in 0..2000, `tick_start(24 * k) == step_start(k)`.
  - `note_events_land_on_their_samples` (145 BPM; acid track; clip `bars = 1`, one note `{ start = 0.5, pitch = "C3", len = 1 }`; placement 2 bars): Ons at samples 2483 and 81931, Offs at 7448 and 84414; the first On's `ctx` is `{ pos: 2483, step: 0, seed: hit_seed(track_seed(seed, "t"), 0) ^ 12 }`.
  - `notes_loop_with_their_clip`: a 1-bar clip in a 3-bar placement: its Ons repeat at the same offsets in each bar.
  - `an_offset_skips_notes_that_started_before`: a 2-bar clip with a note from step 8 to step 24, placed with `offset = 1`: no On for that note's id in the placement.
  - `a_placement_ending_mid_note_sends_all_off`: a 2-bar clip with one note of 24 steps in a 1-bar placement followed by silence: one `AllOff` at `step_start(16)`, nothing after.
  - `loop_wrap_sends_all_off`: `set_loop(Some(0..1))` over that note: an `AllOff` at each wrap, then the note's On again.
  - `seek_mid_note_is_silent_after`: real acid303 track; seek to bar 1 during a long note: the mix RMS of 2000..10000 samples after the seek is below 1e-4 when the clip has no note starting there.
  - `drum_note_clip_on_the_grid_renders_like_its_step_clip`: a kick playing `x...x...x...X...` and one playing notes at steps 0, 4, 8 (vel 100) and 12 (vel 127) render bitwise equal stems over 2 bars.
  - `acid_note_clip_renders_like_its_step_clip` (120 BPM): steps `C2 . D#2! . C3~ C2 . G2~ G2 . . . . . . .` against notes `{0, C2, 0.5}`, `{2, D#2, 0.5, vel 127}`, `{4, C3, 1.25}`, `{5, C2, 0.5}`, `{7, G2, 1.5}`, `{8, G2, 0.5}`: bitwise equal acid stems over 2 bars.
  - `velocity_110_accents`: notes at vel 110 and 109 give Ons with `accent` true and false.
  - `chords_hit_once_per_note`: two notes with the same start on a kick give two Ons at one sample (note order); on an acid track the probe sees On #0 then On #1 at one sample; two renders of the acid chord are bitwise equal.
  - `note_clip_hits_drive_the_ducker`: kick (duck source) plays a notes clip with one hit at step 0.5; acid track with `duck.depth = 12` plays a held note: the acid stem equals the stem without the kick hit for frames 0..3000 and differs within 3000..3010 (120 BPM).
  - `no_panic.rs::compile_never_panics_on_valid_songs`: the generator also makes notes clips (1 to 4 bars, 0 to 12 notes, starts and lengths in random ticks, random pitches and velocities, overlaps) on random instruments; every song that parses compiles and renders its first and last bar.
- [ ] **Step 2: Run** `cargo test -p tune-engine --test notes` — Expected: FAIL.
- [ ] **Step 3: Implement** `tick_start` and the delivery. Leave the step-clip path and every float expression of `frame` as Task 1 left them.
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS, `baseline.rs` unchanged.
- [ ] **Step 5: Run** `task verify`, then commit `feat(engine): play note clips with sample-accurate events`.

### Task 4: Note clips across swaps

**Files:**
- Modify: `crates/tune-engine/src/engine.rs:638-716` (`apply_swap`), `crates/tune-engine/tests/notes.rs`, `crates/tune-engine/tests/live_no_alloc.rs`

**Interfaces:**
- Consumes: the sequencer's `last`, `notes_live` and cursor (Task 3).
- Rule (determined here), in `apply_swap` for each track kept with its instrument, when `notes_live`: let `src` be the new song's source for the track at the swap step `k`. Held notes continue only if the last source played a notes clip and `src` has the same `start_step` and `offset_steps` and a clip whose `NoteClipEvents` equals the old one (same `id`, events and length); then `last` and the cursor point into the new song's clip. Otherwise `last = None`, so step `k` delivers `AllOff` (Task 3's rule). Comparisons only; nothing allocates.

- [ ] **Step 1: Write the failing tests.**
  - `notes.rs::a_value_swap_keeps_a_held_note`: a 16-step note; a swap queued during step 4 (it lands at step 5) to the same song with `gain_db = -3`: no `AllOff` and no Off for the note until its own end.
  - `notes.rs::a_clip_edit_ends_held_notes`: the same, but the swap moves another note of the clip: one `AllOff` at `step_start(5)`, then the new clip's events.
  - `notes.rs::moving_the_placement_ends_held_notes`: the swap moves the placement by one bar: `AllOff` at the swap step.
  - `live_no_alloc.rs::note_clip_swaps_never_allocate`: through `live`, swaps that edit a note, change a notes clip's `bars`, add a notes-clip placement and change only a gain while a note is held; zero allocations and frees inside `process`, one garbage per swap.
- [ ] **Step 2: Run** `cargo test -p tune-engine --test notes` — Expected: FAIL (`a_value_swap_keeps_a_held_note`).
- [ ] **Step 3: Implement** the rule.
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(engine): keep or end note-clip notes across a swap`.

### Task 5: Crossfade a track whose signal path changes (RISKY: audio thread)

**Files:**
- Modify: `crates/tune-modules/src/instrument.rs` (`copy_state_to`, `copy_state`, `Effect: Send + Any`), `crates/tune-modules/src/builtin.rs:120-148` (`Distortion` and `Equalizer` implement it with `copy_state`)
- Modify: `crates/tune-engine/src/engine.rs` (fade roles, `frame` step 3, `apply_swap`, `seek`, `take_retired_swap`, docs of `queue_swap` and `frame`), `crates/tune-engine/src/lib.rs`
- Create: `crates/tune-engine/tests/crossfade.rs`
- Modify: `crates/tune-engine/tests/live.rs` (`swap_removing_track`), `crates/tune-engine/tests/live_no_alloc.rs`, `crates/tune-modules/tests/events.rs`
- Modify: `CLAUDE.md` (Mixing, the hot-reload paragraph: "A swap that adds, removes, reorders or rebuilds a device, swaps the instrument, or adds or removes a track crossfades that track over 10 ms; value, step and clip edits switch at the step.")

**Interfaces:**
- Consumes: the swap identity of Plan 7 (`same_device`), Task 1's sequencer.
- Produces:
  ```rust
  pub const SWAP_FADE_MS: f64 = 10.0;              // fade length = (SWAP_FADE_MS * 0.001 * rate).round().max(1) frames (480 at 48 kHz)
  impl Engine {
      pub fn swap_fading(&self) -> bool;            // a structural swap's crossfade is running
      // take_retired_swap() returns None while swap_fading(); a seek or the song's end ends the fade
  }
  ```
- Roles, set in `apply_swap` per new track (a private field of `TrackRuntime`): `In` (no old track with its id), `Cross { own_instrument }` (the old track exists and its instrument is not the same device, or its fx chain differs in length or in any slot's `same_device`), else `None`. Old tracks without a new one keep their runtimes in the retired update's old list (tell them from the placeholders by a private `vacant` flag) and fade out.
- For `Cross`, after Plan 7's moves the retired `tracks[j]` holds the old chain in old order, with fresh stand-ins where kept devices were. Each kept fx device copies itself into its stand-in (`kept.copy_state_to(stand_in)`); if any returns false the role is `None` (an instant switch, as before). With `own_instrument == false` the old path takes the kept instrument's sample; otherwise the old instrument (in `tracks[j]`) keeps processing, without events.
- Per frame while fading (`a = done / len` after `done += 1`, as `MasterFade`), in `frame` step 3, before the duck: `Cross`: `y = y_old + a * (y_new - y_old)` with `y_old` the old chain on the old (or shared) instrument sample; `In`: `y = a * y_new`; `None`: unchanged. Then each fading-out old track, in old song order: instrument, fx, its own strip's duck, `* (1 - a)`, its old gain and pan, into the master and the buses as it was audible in the old song. The old paths get no lanes, overrides or events. Frames without a fade run exactly today's code.

- [ ] **Step 1: Write the failing tests** in `tests/crossfade.rs`, with test-only modules registered via `Registry::builtin().with(..)`: `TONE` (instrument, plays `[Drum]`, param `tone.freq` default 440, setting `phase` = `sin`|`cos`; the first On starts `0.25 * sin|cos(2π f n / rate)`, never stops; Offs ignored), `FLIP` (effect, `y = -x`, copies with `copy_state`), `RAMP` (effect, `y = x + count * 1e-6` then `count += 1`, copies), `NOCOPY` (effect, `y = x`, default `copy_state_to`). Songs: 120 BPM, track `t` plays `xxxxxxxxxxxxxxxx` on `TONE`. `fn swap_at(a, b) -> (Vec<f32>, u64)` plays 1 bar and 1000 frames, queues `b` and plays 2 more steps, returning the mix and the swap sample `S` (the next step start). `fn max_jump(mix, frames) -> f64` over the left channel.
  - `no_click_at_a_structural_swap`: for each pair, `max_jump(S..S+960) <= 1.5 * max_jump(S-4800..S)`: fx added (`[]` → `[FLIP]`), removed (`[FLIP]` → `[]`), reordered (`[eq low_cut 200, distortion hard drive 18]` → reversed), mode changed (distortion `soft` → `hard`, drive 18), instrument swapped (`phase = "sin"` → `"cos"`), track added (a second `TONE` track, `phase = "cos"`, `gain_db = -6`), track removed (the reverse).
  - `a_hard_switch_would_click`: for the fx-added and fx-removed pairs (a stateless `FLIP`, a kept instrument: the offline render of `b` is exactly what an instant switch plays), `|offline(b)[S] - offline(a)[S-1]| > 1.5 * max_jump(S-4800..S)`, so the test signal detects a click.
  - `the_fade_blends_old_and_new`: for `[]` → `[FLIP]`, each frame `S + n`, `n < 480`, equals `a + ((n+1)/480) * (b - a)` of the two offline renders within 1e-6, and frame `S + 480` equals `b` bitwise.
  - `kept_devices_keep_state_through_the_fade`: a silent `TONE` track (no placements) with `[RAMP, eq]` (eq at defaults, a bitwise pass-through) → `[eq, RAMP]`: every frame from 0 to `S + 2000` equals the offline render of the first song bitwise (a fresh `RAMP` anywhere would restart at 0).
  - `the_old_runtime_returns_after_the_fade`: after a structural swap, `swap_fading()` is true and `take_retired_swap()` is `None` for frames `S..S+480`, then `Some`.
  - `a_value_swap_does_not_fade`: a `gain_db` swap: `swap_fading()` false at `S`, `take_retired_swap()` `Some` right after.
  - `a_swap_during_a_fade_waits_for_it`: a second structural swap queued at `S + 100` lands at the first step boundary after the first one's garbage is taken.
  - `a_device_that_cannot_copy_switches_at_once`: `[NOCOPY]` → `[NOCOPY, FLIP]`: no fade.
  - `events.rs::effects_copy_their_state`: an `EQ` and a `DISTORTION` (`hard`) fed 1000 samples of noise, copied into fresh ones, give the same next 1000 samples bitwise; `EQ.copy_state_to(&mut distortion)` is false.
  - `live.rs::swap_removing_track`: compare with `rb` from `STEP + 480` on (the hat fades out over the first 480 frames).
  - `live_no_alloc.rs::structural_swaps_fade_without_alloc`: through `live`, swaps that reorder two eqs (state copies), change a distortion mode, add a track and remove one; zero allocations and frees inside `process`, one garbage each after `collect_garbage`.
- [ ] **Step 2: Run** `cargo test -p tune-engine --test crossfade` — Expected: FAIL.
- [ ] **Step 3: Implement** the trait method, the two built-in copies, the roles, the fade and the retired-update hold; update the CLAUDE.md line.
- [ ] **Step 4: Run** `cargo test -p tune-engine --test baseline && cargo test --workspace` — Expected: PASS with the unchanged constants.
- [ ] **Step 5: Run** `task verify`, then commit `feat(engine): crossfade a track when a swap changes its signal path`.

### Task 6: Note clips in `tune set`, `tune changes`, `tune info` and `tune params`

**Files:**
- Modify: `crates/tune-studio/src/path.rs:14-52,136-200` (`ClipNotes`, `ClipBars`), `crates/tune-studio/src/doc.rs:361,608-617` (read and write), `crates/tune-studio/src/diff.rs:110-114`, `crates/tune-studio/src/set.rs:57-67` (`from_toml` reads inline tables as JSON objects)
- Modify: `crates/tune-cli/src/cmd/info.rs:72-122`, `crates/tune-cli/src/cmd/params.rs:41-145`
- Test: `crates/tune-studio/tests/{doc.rs,diff_log.rs}`, `crates/tune-cli/tests/{song_cli.rs,edit_cli.rs}`
- Modify: `CLAUDE.md` (the `tune set` table and the Notation bullet on `tune info`)

**Interfaces:**
- Consumes: `NoteClip`, `Note`, `notes_toml`, `pitch_name`, `Clip::steps` (Task 2).
- Produces:
  ```rust
  pub enum SongPath { /* … */ ClipNotes { clip: String } /* `clips.<c>.notes` */, ClipBars { clip: String } /* `clips.<c>.bars` */ }
  ```
  - `effective_value`: `ClipNotes` is `[{"start": <steps>, "pitch": "<name>", "len": <steps>, "vel": <n>}, …]` (`vel` always present, steps as `f64` ticks / 24), `ClipBars` the bars; both `null` for an acid or drum clip; `Clip` (`clips.<c>.steps`) is `null` for a notes clip. `diff_songs` compares, per clip in the union, `Clip`, `ClipBars`, `ClipNotes` in that order.
  - `SongDoc::set`: `ClipNotes` takes a JSON array of objects with keys from `start, pitch, len, vel` and writes `notes = <text>` in the clip's table, laid out as `notes_toml` lays notes out (keys in that order, numbers as given); the commit's validation gives positioned meaning errors as `EditError::Invalid`. `ClipBars` takes an integer. On the wrong kind: `EditError::BadValue` "clip `k4` is a drum clip: it has no notes" (`bars` likewise) and, for `clips.<c>.steps` on a notes clip, "clip `lead` is a notes clip: it has no steps (set clips.lead.notes)".
  - `tune info --json` gains `"clips": [{"id", "kind", "steps"}]`, plus `"bars"` and `"notes"` (the count) for a notes clip, in pool order; text mode prints `clips: k4 drum 16 steps, lead notes 2 bars 5 notes`.
  - `tune params --json` gains `"plays": {"acid303": ["acid", "notes"], "kick": ["drum", "notes"], …}` in registry order; text mode prints `  plays: acid, notes` under each instrument.

- [ ] **Step 1: Write the failing tests.**
  - `doc.rs::clip_notes_paths`: `"clips.lead.notes".parse::<SongPath>()` displays the same, as does `clips.lead.bars`; on a v2 song with `[clips.lead]` (two notes), `get` gives the JSON above; setting it to `[{"start": 0, "pitch": "C3", "len": 2}, {"start": 4, "pitch": "G3", "len": 1, "vel": 120}]` changes only the `notes` value, to `[\n  { start = 0, pitch = "C3", len = 2 },\n  { start = 4, pitch = "G3", len = 1, vel = 120 },\n]`; setting a note with `start = 0.3` gives `EditError::Invalid` with the off-grid message and leaves the text; the three wrong-kind cases give their messages.
  - `diff_log.rs::note_edits_are_clip_note_changes`: changing one note's pitch gives exactly one change, `clips.lead.notes`, old and new arrays as above; changing `bars` gives `clips.lead.bars`.
  - `edit_cli.rs::set_clip_notes_from_the_cli`: `tune set <copy> 'clips.lead.notes=[{ start = 0, pitch = "C3", len = 2 }]' --json` succeeds, the file holds `notes = [{ start = 0, pitch = "C3", len = 2 }]` and `tune changes --json` lists `clips.lead.notes` from `claude`.
  - `song_cli.rs::a_note_clip_song_validates_and_renders`: a v2 song with a kick drum clip and an acid notes clip validates, `tune info --json` lists both clips as specified, and `tune render <song> --stems --json --out <tmp>/x.wav` succeeds with a non-silent acid stem (`peak_dbfs` above -60).
  - `song_cli.rs::params_json_lists_tables`: `v["plays"]["kick"] == json!(["drum", "notes"])`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test doc` — Expected: FAIL.
- [ ] **Step 3: Implement** the paths, values, diff, writes and the two CLI additions; add the CLAUDE.md rows `clips.<c>.notes` ("the notes as a TOML array of `{ start, pitch, len, vel }`, quoted") and `clips.<c>.bars` ("a notes clip's length, 1 to 64").
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(cli): read, write and list note clips`.

### Task 7: The current studio shows note clips read-only

**Files:**
- Modify: `crates/tune-studio/src/view.rs:139-150,222-243` (`PatternView::Notes`), `crates/tune-studio/tests/doc.rs`
- Modify: `apps/desktop/src/client/protocol.ts:148-150`, `apps/desktop/src/lib/song.ts:71-74`, `apps/desktop/src/lib/describeChange.ts:69`, `apps/desktop/src/screens/dock/StepsTab.tsx:115-150`, `apps/desktop/src/screens/StudioScreen.tsx:94-102`
- Modify: `packages/ui/src/components/Arrangement.tsx:31-36,1151,1180-1215`
- Test: `apps/desktop/src/lib/song.test.ts`, `apps/desktop/src/screens/dock/StepsTab.test.tsx`, `apps/desktop/src/lib/describeChange.test.ts`, `packages/ui/src/components/Arrangement.test.tsx`

**Interfaces:**
- Consumes: `NoteClip` (Task 2), the `clips.<c>.notes|bars` paths (Task 6).
- Produces (wire, mirrored by hand in `protocol.ts`; generated types are Plan 9):
  ```rust
  pub enum PatternView { Acid { … }, Drum { … }, Notes { id: String, bars: u32, notes: Vec<NoteView> } }   // tag "notes"
  pub struct NoteView { pub start: f64 /* steps */, pub pitch: u8, pub len: f64 /* steps */, pub vel: u8 }
  ```
  ```ts
  export type NoteView = { start: number; pitch: number; len: number; vel: number };
  // PatternView gains | { kind: "notes"; id: string; bars: number; notes: NoteView[] }
  // ArrangementPattern.kind and GlyphProps.kind: "acid" | "drum" | "notes"
  ```
- UI behaviour (spec §10): `patternGlyph` of a notes clip has `16 * bars` values; the value at `Math.floor(start)` of each note is the larger of what is there and `vel >= 110 ? 1 : 0.6`. The arrangement draws a notes clip's block like an acid one (dashes). The Steps tab, for a track whose pattern in the section is a notes clip, shows "<track> plays the note clip <id> in <section> (read only)." and no editor. `describeChange`: `clips.<c>.notes` → "Changed the notes of <c>", `clips.<c>.bars` → "Changed the length of <c>". The pattern picker lists note clips for every track (`plays` holds `"notes"`).

- [ ] **Step 1: Write the failing tests.**
  - `tests/doc.rs::views_show_note_clips`: `SongView::from(song)` of a song with `[clips.lead]` has `patterns[i] == {"kind": "notes", "id": "lead", "bars": 1, "notes": [{"start": 0.0, "pitch": 48, "len": 3.0, "vel": 100}, …]}`.
  - `song.test.ts`: "a note clip's glyph marks its note starts" (`bars: 1`, notes at 0, 2.5 (vel 120), 15.9 → values 0.6 at 0, 1 at 2, 0.6 at 15, 0 elsewhere, length 16).
  - `StepsTab.test.tsx`: "a note clip shows as read only" (the sentence above, no step buttons).
  - `Arrangement.test.tsx`: "draws a note clip's block with its glyph".
  - `describeChange.test.ts`: the two descriptions.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test doc` and `pnpm -r test` — Expected: FAIL.
- [ ] **Step 3: Implement** the view (dropping Task 2's filter), regenerate the golden (`UPDATE_GOLDEN=1 cargo test -p tune-studio --test protocol`; review: no diff unless the fixture has a notes clip), and the UI.
- [ ] **Step 4: Run** `cargo test -p tune-studio && pnpm -r test && task ui:build` — Expected: PASS.
- [ ] **Step 5: Visual check (not committed):** copy `songs/first-acid/song.toml` into `scratch/notes-check/songs/first-acid/`, add a `[clips.lead]` notes clip and a placement of it on the acid track by hand, `cargo run -p tune-cli -- serve scratch/notes-check/songs/first-acid/song.toml --device null --port 4580` in the background, then with the `agent-browser` skill: the note clip's block shows its glyph, its section's Steps tab says read only, the 303 panel, steps, drum grid, section operations, mixer and knobs still work, and an edit of the clip in the file shows the change notice. Stop the server.
- [ ] **Step 6: Run** `task verify`, then commit `feat(ui): show note clips read-only in the current studio`.

---

## Self-review notes

- Spec coverage: §7.3 note on/off with pitch, velocity and a sample position, accent and slide as attributes, note ids for polyphony (Task 1); 303 and drum modules keep their samples (Task 1, pins); note clips on any instrument, acid303 slides on overlap, accents above a velocity, drum note on = hit (Tasks 2, 3); incompatible pairs stay validation errors (Plan 7 rule, unchanged); mono I/O kept. §7.4 note clips with length, notes, a grid finer than 1/96 bar and documented notation (Task 2). §7.1 crossfade with the 10 ms length and the 1.5× pass/fail (Task 5). §9 no-panic compile with note clips (Task 3), swaps of clips and devices allocation-free (Tasks 4, 5), stems and `--bars` untouched (Task 6 renders stems). §10 note clips as blocks without a step editor (Task 7), the rest of the studio unchanged (Task 7 visual check).
- Not here on purpose: the instrument-swap action and every other action, `tune do`, generated types, undo and runs (Plan 9); session view (Plan 10).
