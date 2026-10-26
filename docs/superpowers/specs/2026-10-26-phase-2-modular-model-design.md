# tune-ai Phase 2 — Modular model and engine foundations

Date: 2026-10-26
Status: approved by the producer 2026-10-26
Roadmap: `docs/roadmap.md`, Phase 2
Parent spec: `docs/superpowers/specs/2026-10-20-tune-ai-design.md`

## 1. Intent

Phase 4 rebuilds the studio as a DAW-class app: tracks you add and remove,
instruments and effects you drop onto them, a piano roll, a session view next
to the arrangement, and an in-app agent that can do everything a human can.
Today's song model cannot carry any of that. Tracks have one of six fixed
instruments, a track holds at most one distortion and one EQ, patterns are
bound to one track, the arrangement is a list of sections that each say which
pattern every track loops, and the only edits anyone can make are "change this
existing value".

Phase 2 builds the model, engine and action layer the new UI and the agent
will sit on, before any of that UI exists. It is plumbing, but it decides how
the whole app behaves later, so the interfaces must be the ones Phase 4 and
Phase 5 can live with.

Who it serves: the producer (who will, from Phase 4 on, assemble tracks and
devices by hand) and the agent (which must be able to do every one of those
things through the CLI, and have each edit show up in the UI and be undoable).

## 2. What the producer decided

These came out of the 2026-10-26 conversation and are not the builder's to
change.

1. **Session view and arrangement both exist.** The producer wants to be able
   to jam loops until something clicks and to build on the timeline directly.
   Both views play the same clips.
2. **One clip pool per song.** Every clip (303 pattern, drum pattern, note
   clip) is a named item in the song's pool. Session slots and arrangement
   placements are references to a pool clip, so editing a clip changes it
   everywhere it is used. The browser will list the pool (Phase 3/4), and
   clips get from the pool into a slot or onto the timeline by drag and drop
   (Phase 4 UI).
3. **Ableton's track model.** Each track holds exactly one instrument followed
   by a chain of effects. Effects can be added, removed and reordered; the
   instrument can be swapped. Stacking several instruments on one track is a
   later "rack" module and the model must not rule it out.
4. **Undo steps back one action by default**, whoever made it. In addition,
   every agent run is tagged, and "undo this run" reverts all of that run's
   edits at once.
5. **The current studio keeps working** on v2 songs with everything it does
   today, and gains app-wide Undo/Redo shortcuts and an "Undo this run" button
   on the Claude-edit toast card. Adding devices and launching session clips
   are reachable from the CLI only until Phase 4 (designer's call, accepted
   by the producer: UI for them now would be thrown away in Phase 4).
6. **Returns and sends stay as today**: the delay and reverb buses, fed by the
   strip sends. Return tracks with their own device chains are not Phase 2.

## 3. Goals

- **P2-G1. Devices from a registry.** Instruments and effects are modules
  registered in one place. A track's instrument and its effect chain are
  data in the song file; any number of effects of any type, in any order.
  Adding a new module touches its own files plus one registration line, not
  the validator, the engine's signal flow, the CLI or the views (spec G9).
- **P2-G2. Clips.** A per-song clip pool holding three clip kinds: 303
  patterns, drum patterns (both exactly as today's notation), and new generic
  note clips (pitch, start, length, velocity, at any position on a fine
  grid). Clips are not bound to a track; any compatible track can play them.
- **P2-G3. Arrangement of clip placements.** Each track has a timeline of clip
  placements (start bar, length, offset into the clip; the clip loops to fill
  the length). Sections remain as named, contiguous regions of the ruler that
  give the song its length and its names ("intro", "drop"), and that the CLI
  and the current UI keep using.
- **P2-G4. Session view in the engine.** Scenes (rows) and per-track clip
  slots. In live play a track either follows the arrangement or plays a
  launched session clip; clips and scenes launch quantised to the next bar;
  "back to arrangement" returns a track (or all tracks) to the timeline.
  Session state is live state: offline renders play the arrangement only.
- **P2-G5. Song format v2** with a `version = 2` field and an automatic,
  comment-preserving migration from today's unversioned (v1) files.
- **P2-G6. App-wide undo/redo** over every song edit (values, steps, clips,
  placements, devices, tracks, sections, automation), human and agent alike,
  plus "undo this run".
- **P2-G7. One action layer.** Every change to a song and every transport and
  session control is a named action with typed arguments. The UI, the `tune`
  CLI and (in Phase 5) the agent all call the same actions. A test fails when
  any UI-reachable action has no CLI equivalent.

## 4. Non-goals

- Any new UI beyond the undo additions in §2.5. No device rack, piano roll,
  session grid, browser or track add/remove buttons in the current studio.
- New synths or effects (roadmap Phase 7). The registry ships with today's
  modules only: acid303, kick, clap, hat_closed, hat_open, ride, distortion,
  eq.
- Racks (several instruments on one track), drum-kit instruments, return
  tracks, group tracks, sidechain from arbitrary sources. Ducking stays keyed
  to the duck source as today.
- Recording a session jam into the arrangement, and knob recording into
  automation lanes (Phase 4). The model must leave room for both.
- Audio clips and the sampler (Phase 3).
- Third-party plugins (Phase 7).
- Tempo automation, time signatures other than 4/4, swing.

## 5. Anti-goals (failures even if everything technically works)

- **v1 songs sound different.** Every existing song and fixture renders
  bit-for-bit as before after migration. The pins in
  `crates/tune-engine/tests/baseline.rs` are never updated.
- **Migration loses the producer's or Claude's comments**, reorders their
  file beyond recognition, or rewrites a song without saying so.
- **The song file stops being something Claude and the producer can read and
  edit by hand.** v2 must stay as LLM-friendly as v1: short ids, compact step
  notation, no generated UUID soup.
- **Agent edits that bypass undo or the edit log**, or a UI action the agent
  cannot perform.
- **Clicks, dropped tails or a stall on the audio thread** when devices or
  clips change during playback. The audio callback stays allocation-free.
- **An interface Phase 4 has to tear up**, e.g. an instrument API that cannot
  carry sub-step notes, or param ids that change when a device is moved.

## 6. Constraints

- Everything in the parent spec's §5 and CLAUDE.md "Rules" still holds:
  determinism (same file, same start position, identical samples, offline and
  live), seeded randomness only, no audio or personal data in git, no GPL
  code, `task verify` before every commit, never loosen the Open303 parity
  tolerances.
- The live engine keeps the existing hot-reload contract: swaps land at a step
  boundary, keep the playhead, keep instrument and effect state for anything
  that did not change, and allocate nothing on the audio thread
  (`live_no_alloc.rs` must keep passing and must cover the new swap kinds).
- The DSP and engine crates stay free of Tauri, UI and network dependencies.
- The song file stays TOML, hand-editable and hot-reloadable, with errors
  reported at the line and column of the user's own text.
- Track ids and their seeds (`track_seed(song.seed, track id)`) are kept
  through migration, because noise depends on them.

## 7. The model, as decided

Names below are illustrative; the exact TOML spelling is the builder's, within
these rules.

### 7.1 Devices and parameter ids

- A track has `instrument` (one device) and `fx` (an ordered list of
  devices). Each device has a module type (`acid303`, `eq`, …), its params,
  and its module-specific settings (today only the distortion `mode`).
- **Every device has a short id, unique within its track**, which is also the
  param prefix. Defaults: the instrument's id is its module name (`acid303`,
  `kick`), an effect's id is its module's prefix (`dist`, `eq`), and a second
  instance of the same type gets a numbered id (`eq2`). The producer or agent
  may rename a device.
- **Full param ids are `<track>.<device>.<param>`**, plus the existing strip
  ids `<track>.send.delay`, `<track>.send.reverb`, `<track>.duck.depth`,
  `<track>.duck.release` and the `mix.*` ids. With the default device ids
  every v1 id (`acid.acid303.cutoff`, `acid.dist.drive`, `mix.delay.time`)
  is still a valid v2 id meaning the same thing, so lanes, edit-log entries,
  the UI's highlight keys and Claude's habits carry over.
- **A param id never changes when its device is moved** within the chain;
  it changes only when the device is renamed, and a rename rewrites every
  reference to it (lanes, edit log readers must cope with old ids in old
  entries).
- **Hot-swap identity is (track id, device id, module type).** A device that
  keeps all three keeps its DSP state across a swap, wherever it moved in the
  chain. A new or changed device starts fresh.
- **No click at a structural swap (designer's call).** When a swap changes a
  track's signal path (a device added, removed, reordered, swapped or reset,
  a distortion mode change, a track added or removed), that track crossfades
  from its old path to its new one over 10 ms, as the master-mode switch does
  today. The old runtime keeps running for the fade and then goes back to the
  garbage queue; this stays allocation-free. Pass/fail, on a steady test
  signal: the largest sample-to-sample jump in the 20 ms after the swap is no
  bigger than 1.5 times the largest in the 100 ms before it. A swap that only
  changes values, steps or clips needs no crossfade (it already lands on a
  step boundary). Offline renders never swap, so they are unaffected.

### 7.2 Module registry

- A module descriptor carries: module name, kind (instrument or effect), its
  param table (`ParamSpec`s as today), its settings (e.g. modes), which clip
  kinds it can play (instruments), and a factory for its DSP object.
- The validator, compiler, engine, `tune params`, the studio views and the
  edit layer read the registry; none of them match on module names. Today's
  closed enums `InstrumentKind` and `FxSpec` go away, or survive only as an
  internal detail of the built-in modules.
- Where the registry lives is the builder's call (the descriptor half must be
  visible to `tune-song`'s validator, the factory half to `tune-engine`).
- To prove P2-G1, a test registers a test-only module from inside the test
  and plays it, with no edits outside the test.

### 7.3 Instruments and note events

- Instruments receive **note events**: note on (pitch, velocity, sample
  offset), note off, and the existing 303 step semantics (accent, slide)
  as event attributes. Events carry a sample-accurate position, not only a
  step index, so note clips can sit off the 16th grid and have lengths.
- The interface must allow polyphonic instruments (events carry a note id or
  pitch so offs match ons) even though no polyphonic module ships in Phase 2.
- The 303 and drum modules keep producing exactly today's samples when driven
  by 303 and drum patterns (v1 pins). How their step behaviour maps onto
  events is the builder's call.
- **Clip compatibility:** 303 patterns play on acid303; drum patterns play on
  drum voices; note clips play on any instrument (on acid303, overlapping
  notes slide and velocity above a threshold accents; on a drum voice a note
  on is a hit, velocity above a threshold an accent — designer's call). A
  placement or slot that pairs an incompatible clip with a track is a
  validation error.
- **Swapping a track's instrument (designer's call)** is one undoable action
  that also does the clean-up, so the song stays valid: the instrument
  device takes the new module's default id (`acid303` becomes `kick`), lanes
  on the old instrument's params are removed, placements and slots holding
  clips the new instrument cannot play are removed (the clips stay in the
  pool), and the action's result lists everything it removed. Undo restores
  all of it.
- Mono or stereo device I/O is the builder's call, with one requirement: if
  the interface goes stereo, the v1 signal path must still produce today's
  bits (mono instrument → mono fx → duck → gain → pan as now).

### 7.4 Clips

- The pool is a top-level table of clips keyed by id (`[clips.acid_main]`).
  v1 `[patterns.*]` become clips with the same ids; the v1 `track = ...` field
  is dropped (it was only a binding, now replaced by compatibility).
- 303 and drum clips keep today's step-string notation and 1..64 steps.
- Note clips: a length (in bars or steps) and a list of notes, each with
  pitch, start, length and velocity. Positions on a grid of at least 1/96 of
  a bar (designer's call; finer is fine). The notation must be compact enough
  that Claude writes and reads it comfortably; the builder picks it and
  documents it in the song-format docs.
- A clip may carry its own automation (clip envelopes) later; leave room, do
  not build.

### 7.5 Arrangement, sections and automation

- **Sections** stay: an ordered list of named regions with bar counts, laid
  end to end from bar 1. Together they define the song length. Section
  operations act on the placements and automation inside the section, so the
  current UI's section operations keep their meaning (designer's calls):
  - A placement that crosses a section edge is first split at the edge into
    two placements of the same clip, the second with the matching offset, so
    every section operation sees only placements inside it. Lanes split the
    same way.
  - **Move, duplicate, delete** move, copy or remove the section's
    placements and lane stretches with it; everything after it shifts.
  - **Resize** changes the section's bar count. A placement that ran to the
    old end grows or shrinks to the new end (its clip keeps looping; nothing
    is time-stretched); other placements are cut at the new end or left
    alone; lane stretches are cut at the new end, and a growing section
    holds the last value as v1 does.
  - **Set what a track plays in a section** (today's `set_section_play`)
    replaces that track's placements inside the section with one placement
    covering the whole section, or removes them when given no clip.
- **Placements** live on each track's timeline: start bar (absolute), length
  in bars, and an offset into the clip; the clip loops from its start within
  the placement, as a pattern loops inside a section today. Placements on one
  track may not overlap (designer's call; Ableton also forbids it).
- **v1 migration of `play`:** each section's `play = { track = pattern }`
  entry becomes one placement on that track covering the section, offset 0.
  The result must render identically, including patterns restarting at each
  section start.
- **Automation** lanes move to the arrangement: one lane per param id, with
  breakpoints at absolute bar positions. A lane must be able to express
  "no automation here" stretches, because in v1 a section without a lane
  falls back to the param's static value; migration must reproduce that
  exactly (one way: a lane is a list of segments; the builder chooses).
  Interpolation, value range (normalised 0..1) and evaluation per sample stay
  as today.

### 7.6 Session view

- **Scenes** are named rows. Each track has one slot per scene; a slot is
  empty or references a pool clip.
- In live play each track is in one of two states: following the arrangement,
  or playing a session clip. Launching a clip or a scene switches the affected
  tracks at the next bar line (default launch quantisation one bar;
  designer's call). Stopping a session clip silences that track until it is
  sent back to the arrangement. "Back to arrangement" (one track or all)
  rejoins the timeline at the current position.
- **Automation while a session clip plays (designer's call, as Ableton):**
  a track playing a session clip ignores the arrangement lanes on its own
  params and uses their static values (plus live knob moves); it picks the
  lanes up again when it goes back to the arrangement. `mix.*` lanes keep
  running. Ducking follows whatever the duck source track is playing,
  arrangement or session clip.
- Scenes, slots and their clips are saved in the song file. Which clips are
  currently launched is live state and is not saved; it is not part of
  undo.
- A launched session clip starts from its beginning at the launch bar and
  loops. Determinism holds per run: the same sequence of launches at the same
  bars produces the same samples, and a test pins that.
- The model must leave room for recording a session performance into the
  arrangement later (e.g. by logging launches with their bar positions).

### 7.7 Format v2 and migration

- v2 files carry `version = 2` in `[song]`. A file without `version` is v1.
  Parsers reject versions they do not know with a clear message.
- **Migration is a comment-preserving text transform** (`toml_edit`, not the
  canonical writer). It keeps the producer's comments and keeps tables in
  their original order where they still exist.
- `tune migrate <song> [--json] [--dry-run]` migrates a file in place, writes
  atomically, and logs one edit-log entry for it. `tune validate`, `info`,
  `render` and `play` accept v1 files by migrating in memory and say so in
  their output. The studio, on opening a v1 song, migrates it, writes it
  back, logs it, tells the producer in a notice, and keeps the migration
  undoable (designer's call).
- `songs/first-acid/song.toml` is migrated in the repo as part of Phase 2.
- **The v1 fixtures stay frozen as v1 files** and keep feeding the baseline
  pins through the migration path. Phase 2 adds v2 copies of them with the
  same expected hashes, so both paths are pinned.

## 8. Actions, undo and the edit log, as decided

### 8.1 The action layer

- One Rust definition of every action: a name, typed arguments, a short help
  text, and whether it edits the song (undoable, logged) or is live control
  (transport, loop, session launches). It replaces the studio's `Command`
  enum as the source of truth.
- Song-edit actions cover at least: set a value (today's paths), set a step,
  add/remove/rename a track, set or swap a track's instrument, add/remove/
  move/rename an effect, set a device setting, add/remove/rename/duplicate a
  clip, edit clip notes and steps, add/move/resize/remove a placement, add/
  remove/rename/reorder a scene, set a slot, add/remove/move/resize/
  duplicate/rename a section, add/remove a lane, set lane breakpoints, set
  mix fields.
- Live actions cover today's transport commands plus launch clip, launch
  scene, stop clip and back to arrangement.
- Every action names its targets with stable ids (track, device, clip, scene,
  section, param ids). Phase 5's teach mode will use these ids to find the
  control to spotlight, so the UI must be able to map an action target to an
  on-screen element.
- **`tune actions --json`** lists every action with its arguments and help,
  so the agent can discover them. **`tune do <song> <action> [args] --json`**
  runs one action (or a batch from stdin, applied atomically as one undo
  step per action or as one step for the batch; builder's call, documented).
  `tune set` stays as a shorthand for the set-value action.
- **When the studio has the song open, CLI actions go through the running
  studio**, not around it, so the UI sees them instantly, they join the same
  undo history, and live actions (play, launch) work at all. When no studio
  has the song open, song-edit actions write the file directly as `tune set`
  does today, and live actions fail with a clear "studio not running" error.
  How the CLI finds the running studio is the builder's call within these
  rules: localhost only, protected by a per-user secret not stored in the
  repo, and refusing other origins the way `tune serve` does. Edits made by
  hand in an editor still arrive through the file watcher as today.
- **The parity test** fails when an action reachable from the studio protocol
  is not reachable through `tune do`, using the exhaustive-match pattern
  already used in `crates/tune-studio/tests/protocol.rs`. The only exemptions
  (designer's call) are listed by name in the test, each with its reason:
  connection and view queries the CLI already answers another way
  (`get_state`, `list_songs`, `open_song`), the conflict prompt only a human
  sees (`resolve_offered`), and knob previews during a drag (`set_param`
  with `commit: false`), whose final value the agent sets directly. Showing
  agent edits as animated knob sweeps is Phase 5.
- The wire types the frontend uses (actions, views, events) are generated from
  the Rust definitions instead of hand-written (designer's call; e.g. `ts-rs`
  or `specta`), so the TS side cannot drift.

### 8.2 Undo and runs

- **Undo and Redo step one action at a time** across human and agent edits,
  as today, but over structured actions rather than whole-file snapshots
  wherever that is needed to make run undo work. A UI gesture that is one
  action today (a knob drag, a burst of step clicks coalesced by the 150 ms
  debounce) stays one undo step.
- **Runs.** An agent action may carry a run id and a run title ("make the
  build hit harder"), given by `--run`/`--run-title` or the `TUNE_RUN` and
  `TUNE_RUN_TITLE` environment variables. The edit log stores them.
- **Undo this run** reverts every action of one run, newest first, as a single
  new undoable step. If a later edit by someone else changed the same target,
  that target keeps the later value and the result lists it as kept
  (designer's call, told to the producer). The rules (designer's calls):
  - "Someone else" is any edit not in this run: the producer, another run,
    or an edit made outside the studio.
  - A "target" is one value (a param, a step, a placement's fields, a mix
    field) or one structural item (a track, device, clip, placement, scene,
    section, lane).
  - If the run created an item that a later edit then used or changed (the
    producer tweaked a knob on a track the run added, or moved a clip it
    made), the item is kept, with the later edits, and listed as kept.
    Nobody's later work is deleted by a run undo.
  - With the studio closed, a hand edit in an editor is not in the log. Run
    undo then compares each target's current value with the value the run
    left; if they differ, the target counts as changed later and is kept.
  - Edit-log entries therefore record, per action, the value or item before
    and after, and the ids of items the action created. It is available as an action
  (`tune do <song> undo_run <run id>`), works with the studio closed (from the
  edit log and the file), and appears in the current studio as an "Undo this
  run" button on the Claude-edit toast.
- The current studio gains ⌘Z / ⇧⌘Z for undo and redo, wherever focus is,
  except inside a text field.
- Undo history while the studio is open covers the session as today (depth
  500). Whether it survives an app restart is the builder's call; run undo
  must work across restarts because it reads the edit log.

### 8.3 Edit log

- `edits.jsonl` stays the per-song, gitignored log. Entries gain the action
  name, the run id and title when present, and whatever the builder needs to
  invert structural actions for run undo. Old entries stay readable.
- The UI's change notice and highlight logic keeps working from these
  entries; highlight keys follow the v2 param ids, which equal the v1 ones for
  default device ids.

## 9. Engine requirements

- Compilation from a validated v2 song to the engine's data must not panic on
  anything a user or agent can produce; invalid input is a validation error.
- Swaps that add, remove, reorder or swap devices, add or remove tracks,
  change clips and placements, or change scenes land at the next step
  boundary, keep state per §7.1, and allocate nothing on the audio thread.
- Session launches are live commands to the engine, applied at the next bar
  line, and allocation-free.
- `--stems` renders still produce one stem per track; `--bars A..B`, `tune
  info`, `tune analyze --sections` and the ears' bar map keep working off
  sections.

## 10. What the current studio must still do

Everything it does on `main` at 656e8a5, on migrated songs: transport, seek
and loop from the ruler, the 303 panel, steps and drum grid, section
operations, the mixer and FX panel, knob drag and release, the Claude-edit
ring and toast, Undo, the open-song screen. The view types change (and become
generated), so the frontend is updated to match; its hard-coded assumptions
(`instrument === "acid303"`, `fx.kind === "distortion"`, the acid/drum pattern
kinds, the strip knob list) may stay where they are still true for the
built-in modules. A track with a module the current UI does not know shows a
plain fallback rather than breaking.

v2-only content the agent can create through the CLI is shown read-only, not
faithfully (designer's call; the real views are Phase 4): placements that do
not line up with sections show as plain blocks with the clip name, note
clips show as blocks without a step editor, extra effects beyond the first EQ
and distortion are listed by name without knobs, and lanes that cross
sections draw as today where they can. None of it may crash or block the
current screens, and editing what the current UI does support keeps working
on such a song.

## 11. Exit

1. A new song, built only with `tune do` actions, adds a track, gives it an
   instrument and an effect chain with two EQs and a distortion in a chosen
   order, places clips including a note clip, renders, and hot-reloads in a
   running studio while playing, without a click at the swap.
2. `songs/first-acid` and every v1 fixture migrate, and both the v1 and the v2
   fixtures render with the pinned hashes.
3. In a running studio, `tune do` launches a scene and a clip and sends tracks
   back to the arrangement, at bar lines.
4. An agent run of several edits is undone in one step from the toast and
   from the CLI, keeping a later human edit, and the producer's ⌘Z steps
   through actions one at a time.
5. The parity test and the test-only module test pass. `task verify` is green.

## 12. Left to the builder

- The exact TOML spelling of devices, clips, placements, scenes and lanes,
  and the note-clip notation, within §7 and the readability anti-goal.
- Crate boundaries (a new `tune-model`/`tune-actions` crate or growing the
  existing ones), where the registry lives, and the event API's shape.
- How undo is stored (inverse actions, snapshots, or both) as long as §8.2
  holds.
- How the CLI reaches a running studio, within §8.1's rules.
- How Phase 2 is split into plans. Suggested: (a) registry, devices, param
  ids and format v2 with migration; (b) clips, placements, sections and
  automation on the new model, plus the note-event instrument API; (c) the
  action layer, CLI routing, undo and runs, generated wire types; (d) session
  view in the engine. Each plan keeps `task verify` green and the v1 pins
  passing on its own.
