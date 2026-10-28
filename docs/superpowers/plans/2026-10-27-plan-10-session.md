# Plan 10: The Session View in the Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give songs scenes (named rows) with one clip slot per track, saved in the song file; let the live engine play a track from its session clip instead of the arrangement, launching clips and scenes, stopping tracks and sending them back to the arrangement at the next bar line, allocation-free and deterministic; make the scene and slot edits undoable actions that run undo covers, and the four session controls live actions that `tune do` reaches through a running studio; all while every song without scenes, and every offline render, plays bit for bit as before.

**Architecture:** `tune-song` gains `Song::scenes` (`[scenes.<id>]` with `slots = { <track> = "<clip>" }`), its validation, writer and the scene edits. `tune-engine` compiles scenes to clip indices and keeps, per track runtime, what the track plays (`TrackPlay`: the arrangement, a session clip or stopped) and what is cued for the next bar line; the frame loop reads each track's cells from that source, skips the arrangement lanes of tracks off the arrangement, lands cues at bar lines and remaps both by clip id across hot reloads. `tune-studio` adds scene paths, undo targets, run-undo rules, the nine actions, and an actor that resolves session actions to engine cues only once its newest swap plays. The current UI only learns to word scene changes; the session grid is Phase 4.

**Tech Stack:** Rust 2024 (rustc 1.97), `toml_edit` =0.25.17, `indexmap` 2.14, `rtrb` =0.3.4, `ts-rs` =12.0.1, React 18 + TypeScript + Vitest in `apps/desktop`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-26-phase-2-modular-model-design.md` (the authority). This is the fourth and last Phase 2 plan, spec §12 (d) "session view in the engine". It covers §2.1, §2.2 and §2.5 for the session, P2-G4, §7.6 whole, the slot rule of §7.3 (compatibility, and the instrument swap removing slots), §8.1's "add/remove/rename/reorder a scene, set a slot" and "launch clip, launch scene, stop clip and back to arrangement" with their targets, parity and generated types, §8.2's scene targets for run undo, §9's scene swaps and allocation-free launches, §10 for songs with scenes, and §11 exit 3. It builds on Plan 9 (`feat/actions`, PR #16); Plan 9's plan says the catalog takes these actions additively.

## Planner rulings

- Ruling: scenes are `[scenes.<id>]` tables in file order (the row order), each with an optional inline `slots = { <track id> = "<clip id>" }`; a track without a key has an empty slot; the scene's id is its name, with the track and clip id rules — §12 leaves the spelling to the builder, and this matches tracks and clips (short ids, no UUIDs, §5) — cost: a free-text scene title later is an additive `name` key.
- Ruling: scenes are part of format v2; no version 3 — v2 has not left this repo, and a file without scenes is unchanged — cost: none; a v1 file with `[scenes.*]` gets the existing "this looks like a version 2 song" hint.
- Ruling: Plan 10 adds no session UI: no grid, no launch buttons, and `SongView` does not carry scenes; the only frontend change is wording scene changes and kept scene targets in the toast — spec §2.5 ("launching session clips are reachable from the CLI only until Phase 4", accepted by the producer) and §4 ("no … session grid") — cost: Phase 4 adds a scenes view and the grid.
- Ruling: a slot must name a track of the song and a clip that track's instrument can play; the validator says so with the placement messages — §7.3 "a placement or slot that pairs an incompatible clip with a track is a validation error" — cost: none.
- Ruling: `launch_clip` names a slot (`track`, `scene`), not a bare pool clip, and launching an empty slot is refused ("scene `break` has no clip for track `acid`") — every launch Phase 4's grid makes is a slot button, and §7.6 says slots reference pool clips — cost: an agent puts a clip in a slot before launching it.
- Ruling: `launch_scene` switches every track at the bar line: a slot with a clip launches it, an empty slot stops the track — Ableton's default, so a scene is the row you hear (§7.6 follows Ableton for automation too) — cost: one rule in `Engine::cue` if the producer wants empty slots to leave tracks alone.
- Ruling: `stop_clip` on a track that follows the arrangement also takes it off the arrangement: it is silent until `back_to_arrangement` — §7.6 "stopping … silences that track until it is sent back", and Ableton's stop button — cost: none.
- Ruling: all four session actions land at the next bar line, `stop_clip` and `back_to_arrangement` included; quantisation is one bar (`LAUNCH_QUANTUM_STEPS = 16`) with no setting yet — §7.6's default, and §11 exit 3 says "at bar lines" for both launches and the return — cost: a later quantisation setting is a field on `SessionCue`.
- Ruling: a cue lands at the first step boundary whose step is a multiple of 16 that the engine fires, whether reached by playing, a loop wrap or a seek; a later cue for a track replaces an earlier one still waiting — one place decides the landing — cost: none.
- Ruling: the slide lookahead at the step before a bar line ignores cues, so a 303 note never slides into a launched clip — the samples of a launch then depend only on the bar it lands at, not on when inside the bar it was sent, which is the determinism §7.6 pins — cost: no slide across a launch boundary.
- Ruling: a session clip keeps its own phase across a loop wrap and a seek: it plays on from the clip step it would have played next; a loop wrap still cuts held note-clip notes, as it does on the arrangement — §7.6 says only "starts from its beginning at the launch bar and loops" — cost: one function (`rebase`) if the producer wants a seek to restart session clips.
- Ruling: the song still ends at the arrangement's end; session clips do not extend it, and a longer jam needs a loop — the transport, bar map and `finished` stay as they are — cost: Phase 4 may want endless session play (an engine flag).
- Ruling: session state lives in the engine for one playback: `stop` drops it and `play` starts with every track on the arrangement; a launch with the transport stopped starts playback at the cursor (a bar line, so it lands at once); `stop_clip` and `back_to_arrangement` with the transport stopped succeed and change nothing — §7.6 makes it live state, unsaved and outside undo — cost: none on disk.
- Ruling: live state follows ids across a hot reload: a launched or cued clip follows its clip id (an edited clip plays its new content, §2.2); a clip that is gone, or that the track's new instrument cannot play, stops the track; a renamed or new track follows the arrangement — the engine compares ids, which allocates nothing — cost: none.
- Ruling: what plays is the launched clip, not the slot: `set_slot`, `rename_scene` or `remove_scene` while it plays change nothing until the next launch — Ableton keeps a playing clip too — cost: none.
- Ruling: the studio sends a session cue only once its newest swap plays (`playing_generation == sent_generation`, no swap waiting), resolving ids to indices against that song; the processor drops a cue stamped with another generation, as it drops overrides — a cue's indices must name the song the engine plays, and a cue that waited behind a replaced swap could otherwise never land — cost: a launch sent right after an edit can land one bar later.
- Ruling: `StudioState`, the events and the edit log carry no session state in Plan 10; the actor only remembers which tracks it took off the arrangement this playback, so a knob released on such a track writes its value instead of answering `{"automated": true}` — the lanes do not drive that track (§7.6) — cost: Phase 4 adds a session view to the state.
- Ruling: live session actions are not undoable, never logged, never part of a run, and write nothing; `tune do --batch` refuses them (`EditsOnly`) and with the studio closed they fail with `NeedsStudio` — §7.6 "not part of undo", §8.1 — cost: none.
- Ruling: `targets()` of the live session actions name their controls (`scenes.<s>.slots.<t>` for `launch_clip`, `scenes.<s>` for `launch_scene`, `tracks.<t>` for `stop_clip` and a one-track `back_to_arrangement`, `[]` for back-to-all); transport actions keep `[]` — §8.1 says teach mode spotlights the control an action names — cost: none.
- Ruling: undo targets for scenes are `scenes.<s>` (a whole scene, its `index` ignored in comparisons as a track's is), one leaf per slot `scenes.<s>.slots.<t>`, and `scenes` (the scene order); the order target overlaps itself and every `scenes.<s>`, not the slot leaves; `rename_scene` goes back whole like the other renames — §8.2's "a target is one value or one structural item (… a scene …)" — cost: a run undo of `move_scene` is kept whenever anyone added or removed a scene since.
- Ruling: `remove_track`, `remove_clip` and `set_instrument` remove the slots they would leave invalid and list them (`slots` in `Removed`); `rename_track` renames slot keys, `rename_clip` rewrites slot values; `remove_scene` lists the slots that went with it — Plan 9's clean-up rule and §7.3 ("placements and slots holding clips the new instrument cannot play are removed") — cost: none.
- Ruling: `scenes.<s>.slots.<t>` is also a writable `tune set` path, beside the `set_slot` action, as `sections[<i>].play.<t>` sits beside `set_section_play` — `tune set` takes every value path — cost: none.
- Ruling: scene `index` arguments are 0-based like section indices; `move_scene` is remove-then-insert like `move_section`; `add_scene` without `index` appends — cost: none.
- Ruling: room for recording a session into the arrangement (§7.6) is the single landing point `Engine::land_cues`, which knows each cue's track, clip and landing step; nothing is logged now — cost: a recorder hooks in there.
- Ruling: the session actions return `null`; `tune do` prints nothing for them in text mode — cost: none.

## Global Constraints

- Run `task verify` before every commit. During TDD run the one test file: `cargo test -p <crate> --test <name>` (UI: `pnpm --filter @tune/desktop test`).
- Every commit message is a Conventional Commit in normal prose, one concern per commit, and ends with exactly these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NTWqQ3vUQ6Q116sMFSQZbt
  ```
- "v1 songs sound different" is a failure. `crates/tune-engine/tests/baseline.rs` is never changed; run `cargo test -p tune-engine --test baseline` before and after every engine change. A song without scenes, and every offline render (which never cues), takes exactly today's path through the engine.
- Frozen: `crates/tune-song/tests/fixtures/{mini,mixed,first-acid-v1,first-acid-mixed}.toml`, `crates/tune-song/tests/fixtures/v2/*.toml`, `crates/tune-studio/tests/fixtures/{commented,commented-v1}.toml` and `songs/*/song.toml` are never edited. New tests use the new `crates/tune-song/tests/fixtures/session.toml` (Task 1).
- Determinism: same inputs give bitwise-identical output; the same launches landing at the same bars give the same samples whatever the block sizes and wherever inside the bar they were sent.
- The audio thread stays allocation-free: `Engine::process`, `Engine::cue`, `LiveProcessor::process` never allocate or free; `crates/tune-engine/tests/live_no_alloc.rs` keeps passing and covers cues, landings, session clips across a loop wrap, a seek and swaps that remap or drop a launched clip.
- `tune-dsp` and `tune-modules` are not edited. The DSP and engine crates stay free of Tauri, UI and network dependencies.
- Every structural edit computes the expected `Song` with its `tune-song` function and commits through `SongDoc::commit_as(doc, Some(&expected))`; every edit keeps every other byte of the file, comments included. `IndexMap` equality ignores order, so a test of a scene's position asserts `song.scenes.keys()`.
- Run undo keeps the contract of CLAUDE.md's "Undo a whole run" paragraph: never overwrite a later edit; when in doubt keep the target and list it.
- A task that changes a wire type regenerates both goldens with `UPDATE_GOLDEN=1 cargo test -p tune-studio --test protocol --test wire_types` and reviews the diff; `apps/desktop/src/client/generated.ts` is never edited by hand.
- Parity exemptions stay exactly Plan 9's five (`get_state`, `list_songs`, `open_song`, `resolve_offered`, `set_param` with `commit: false`).
- Every CLI command takes `--json` and exits 2 on failure. Every test that runs `tune` or a studio endpoint sets `TUNE_STUDIO_DIR` to a fresh temp dir; nothing creates the real `~/.tune`.
- Automated audio checks use `--device null` or the in-process capture device only. Never commit audio, renders, screenshots or `scratch/`. Never read or print `tune.local.toml`. No personal or machine-specific data in the repo. Prefer `rg`, `fd`, `jq` in shell commands. Never use bare `git stash`.

## Review Focus

1. **A launch that arrives on the last sample before a bar line, exactly on it, or right after an edit while the swap is still in flight.** It lands at that bar line (or, behind a swap, the first one after the swap plays); never at a step that is not a bar line, never lost. (Task 2 `cues_land_at_the_bar_line_they_reach`; Task 9 `a_launch_during_a_reload_waits_for_the_swap`.)
2. **The producer or Claude edits the song while a session clip plays**: changes its steps, inserts a clip before it in the pool, removes it, swaps the instrument, renames the track. The clip follows its id; gone or unplayable stops the track; a renamed track goes back to the arrangement; nothing panics. (Task 3 `a_launched_clip_follows_its_id`, `a_clip_that_goes_stops_the_track`, `an_instrument_swap_stops_a_clip_it_cannot_play`, `a_renamed_track_follows_the_arrangement`.)
3. **A loop wrap or a seek while a session clip whose length is not a whole bar plays**: the clip plays on in its own phase, with no underflow at a step before the launch step. (Task 2 `session_clips_keep_their_phase_across_a_wrap_and_a_seek`.)
4. **Run undo after the producer worked on what the run did to scenes**: filled a scene the run made, re-slotted a slot the run set, renamed a scene the run renamed, or a run that also launched clips. Later work is kept and listed; launches never appear in the run. (Task 8 `a_scene_the_run_made_and_the_producer_filled_is_kept`, `a_slot_changed_later_is_kept`, `rename_scene_goes_back_whole`; Task 9 `launches_are_not_part_of_a_run_end_to_end`.)
5. **Session actions where there is nothing to act on**: the transport stopped, a song without scenes, an empty slot, an unknown track or scene. Launch starts playback; stop and back change nothing; the rest are refused with a plain message; nothing is written. (Task 9 `launching_while_stopped_starts_playback`, `stop_and_back_while_stopped_change_nothing`, `session_action_errors`.)

---

## Decided formats and interfaces

Every name here is binding for all tasks.

### Format (`tune-song`, Task 1)

```toml
# jam rows
[scenes.groove]
slots = { kick = "k4", acid = "a1" }

[scenes.empty]
```

`[scenes.<id>]` tables come after the clips and before `[[sections]]` in the canonical writer; the only key is `slots`. Messages (positioned): `scene id \`Drop\` may only contain a-z, 0-9 and _` (`check_id(id, "scene")`), `unknown track \`bass\``, `unknown clip \`nope\``, and the placement message `clip \`k4\` is a drum clip; track \`acid\` (acid303) plays acid clips`.

### Model (`crates/tune-song/src/model.rs`)

```rust
/// One session row: which clip each track's slot holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// Track id → clip id, in file order; a track without an entry has an empty slot.
    pub slots: IndexMap<String, String>,
}
// Song gains, after `clips`:
/// The session scenes in file order (the row order). `IndexMap` equality ignores order:
/// compare `scenes.keys()` where the order matters.
pub scenes: IndexMap<String, Scene>,
pub fn scene_table_toml(id: &str, scene: &Scene) -> String;   // write.rs: `[scenes.<id>]\n` + `slots = { … }\n` when not empty
```

### Scene edits (`crates/tune-song/src/scenes.rs`, Task 5; `Edited`, `Removed`, `EditOpError` from `edit.rs`)

```rust
pub fn add_scene(song: &Song, id: &str, index: Option<usize>) -> Result<Edited, EditOpError>; // empty; before scene `index`, else last
pub fn remove_scene(song: &Song, id: &str) -> Result<Edited, EditOpError>;                      // `removed.slots` lists its slots
pub fn rename_scene(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError>;            // keeps its place
pub fn move_scene(song: &Song, id: &str, index: usize) -> Result<Edited, EditOpError>;          // remove, then insert at `index`
pub fn set_slot(song: &Song, scene: &str, track: &str, clip: Option<&str>) -> Result<Edited, EditOpError>;
// Removed gains (Task 5): pub slots: Vec<RemovedSlot>
#[derive(Clone, Debug, PartialEq, Eq)] pub struct RemovedSlot { pub scene: String, pub track: String, pub clip: String }
// EditOpError gains:
SceneExists(String)                          // "scene `groove` already exists"
NoSuchScene(String)                          // "there is no scene `drop`"
EmptySlot { scene: String, track: String }   // "scene `break` has no clip for track `acid`"
SceneIndexOutOfBounds { index: usize, len: usize } // "index 2 is out of bounds (the song has 2 scenes)"
// BadId { what: "scene", .. }: "a scene id is lower-case letters, digits and `_`, got `Jam`"
```

`set_slot` checks the scene, the track and the clip exist (`NoSuchScene`, `NoSuchTrack`, `NoSuchClip`); compatibility is the validator's at commit. `Removed` on the wire: `{"lanes": […], "placements": […], "slots": [{"scene": "groove", "track": "acid", "clip": "a1"}], "duck_source": …}`; the CLI text line is `removed slot a1 on acid in scene groove`.

### Engine (`crates/tune-engine/src/{compile.rs,engine.rs,live.rs}`, Tasks 2 and 3)

```rust
// CompiledSong gains:
pub clip_ids: Vec<String>,          // one per `clips` entry, same order
pub clip_kinds: Vec<ClipKind>,      // one per `clips` entry, same order
pub scenes: Vec<CompiledScene>,     // `Song::scenes` order
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledScene { pub id: String, /// per track (like `tracks`): the clip its slot holds
    pub slots: Vec<Option<usize>> }
impl CompiledSong { pub fn clip_index(&self, id: &str) -> Option<usize>; pub fn scene_index(&self, id: &str) -> Option<usize>; }

pub const LAUNCH_QUANTUM_STEPS: u64 = 16;
/// What a track plays in live play.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackPlay { #[default] Arrangement, /// a session clip, as a placement from its launch step
    Clip(CellSource), Stopped }
/// What waits for a track's next bar line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cued { Clip(usize), Stop, Back }
/// A session command; indices name the song the engine plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionCue { Launch { track: usize, clip: usize }, Scene(usize), Stop(usize), Back(Option<usize>) }
impl Engine {
    /// Cues for the next bar line; never allocates. An index outside the song, or a launch of a
    /// clip the track's instrument cannot play, is ignored.
    pub fn cue(&mut self, cue: SessionCue);
    /// What track `track` plays and what is cued for it; `None` outside the song.
    pub fn session(&self, track: usize) -> Option<(TrackPlay, Option<Cued>)>;
}
impl LiveController {
    /// Sends `cue`, stamped with the generation of the last swap sent; the processor drops it
    /// unless the engine plays that generation (as overrides).
    pub fn cue(&mut self, cue: SessionCue) -> Result<(), LiveError>;
}
```

`TrackRuntime` gains the private fields `play: TrackPlay` and `cue: Option<Cued>` (default for `build` and `placeholder`), so they travel with the runtime across a swap. Re-export `TrackPlay`, `Cued`, `SessionCue`, `CompiledScene`, `LAUNCH_QUANTUM_STEPS` from `lib.rs`.

### Paths and targets (`crates/tune-studio/src/{path.rs,target.rs}`, Tasks 4 and 7)

| String | `SongPath` | `Target` | Value |
| --- | --- | --- | --- |
| `scenes` | `Scenes` (read-only) | `SceneOrder` | `["groove", "break"]`, ids in order |
| `scenes.<s>` | `Scene { scene }` (read-only) | `Scene(String)` | path: `{"slots": {…}}`; target: `{"index": <n>, "slots": {"<t>": "<c>"}}`; `null` when absent |
| `scenes.<s>.slots.<t>` | `SceneSlot { scene, track }` (writable) | `Value(SceneSlot)` | `"<clip>"`, `null` for an empty slot |

`diff_songs` and `target_changes` emit, after the clips and before the sections: `scenes.<s>` for a scene in one song only; `scenes` when the scenes in both songs are in a different relative order; for each scene in both, one slot leaf per track key in either (old's key order, then new's) whose clip differs.

### Actions (`crates/tune-studio/src/action/`, Tasks 5 and 9)

```rust
// song edits (Task 5)
AddScene { scene: String, index: Option<usize> }, RemoveScene { scene: String },
RenameScene { scene: String, to: String }, MoveScene { scene: String, index: usize },
SetSlot { scene: String, track: String, clip: Option<String> },
// live (Task 9)
LaunchClip { track: String, scene: String }, LaunchScene { scene: String },
StopClip { track: String }, BackToArrangement { track: Option<String> },
```

Results: `remove_scene` gives `{"removed": Removed}`; the others `null`. `targets()`: `add_scene`, `remove_scene` → `["scenes.<s>"]`; `rename_scene` → `["scenes.<s>", "scenes.<to>"]`; `move_scene` → `["scenes"]`; `set_slot` → `["scenes.<s>.slots.<t>"]`; the live ones as the rulings say.

## File structure

- `crates/tune-song/src/{model.rs,write.rs,lib.rs,edit.rs}` change; `src/scenes.rs` new (scene edits); `src/raw/mod.rs` (`RawScene`); `src/validate/scenes.rs` new, `src/validate/{mod.rs,arrange.rs}` (shared clip-kinds map and the can't-play message). Tests: `crates/tune-song/tests/scenes.rs` new; fixture `crates/tune-song/tests/fixtures/session.toml` new.
- `crates/tune-engine/src/{compile.rs,engine.rs,live.rs,lib.rs}`. Tests: `crates/tune-engine/tests/session.rs` new, `tests/common/mod.rs` (`SESSION`), `tests/live_no_alloc.rs`.
- `crates/tune-studio/src/{path.rs,doc.rs,diff.rs,target.rs,run_undo.rs,studio.rs,lib.rs}`, `src/action/{mod.rs,catalog.rs,apply.rs}`, `src/doc/items.rs`; `src/doc/scenes.rs` new (scene text writers); `src/session.rs` new (checking and resolving session actions, the off-arrangement set). Tests: `tests/{doc.rs,actions.rs,targets.rs,run_undo.rs,studio.rs,protocol.rs}`, goldens `tests/fixtures/protocol.json`, `apps/desktop/src/client/generated.ts`.
- `crates/tune-cli/src/cmd/{info.rs,do_action.rs}`; tests `crates/tune-cli/tests/{song_cli.rs,edit_cli.rs,do_cli.rs,exit_cli.rs}`.
- `apps/desktop/src/lib/{describeChange.ts,describeChange.test.ts}`.
- `CLAUDE.md`: each task documents what it adds (named in its Files).

---

### Task 1: Scenes in the song file

**Files:**
- Create: `crates/tune-song/src/validate/scenes.rs`, `crates/tune-song/tests/scenes.rs`, `crates/tune-song/tests/fixtures/session.toml`
- Modify: `crates/tune-song/src/{model.rs,write.rs,lib.rs}`, `src/raw/mod.rs` (`RawSong.scenes: Vec<(At<String>, RawScene)>`, `RawScene { slots: Vec<(At<String>, At<String>)> }` read with `str_table`; `ROOT_KEYS_V2` gains `scenes`, `ROOT_HINTS_V1` gains `("scenes", V2_SHAPE)`, `SCENE_KEYS = ["slots"]`), `src/validate/{mod.rs,arrange.rs}`
- Modify: `crates/tune-cli/src/cmd/info.rs`, `crates/tune-cli/tests/song_cli.rs`
- Modify: `CLAUDE.md` (Notation: a "Scenes" bullet with the format above, the slot rules, that scenes are live-only and renders play the arrangement; `tune info` lists them)

**Interfaces:**
- Produces: `Scene`, `Song::scenes`, `scene_table_toml` as decided; `tune_song::Scene` and `scene_table_toml` exported.
- `validate/scenes.rs`: `pub(super) fn scenes(raw: &[(At<String>, RawScene)], shapes: &IndexMap<&str, TrackShape>, kinds: &IndexMap<&str, Option<ClipKind>>, errors: &mut Errors) -> IndexMap<String, Scene>`. `kinds` is the declared-clip map `arrange::v2` already builds for placements (expose it as `pub(super) fn clip_kinds`); the can't-play message moves out of `placements` into `pub(super) fn cannot_play(clip: &str, kind: ClipKind, track: &str, module: &ModuleDescriptor) -> String`, used by both.
- The fixture `session.toml`: the text of `crates/tune-studio/tests/fixtures/commented.toml` with its first line replaced by `# Shared fixture: the commented mini song plus two scenes (tune-song, tune-engine, tune-studio and tune-cli tests).` and this block inserted after the `[clips.h]` table, before the first `[[sections]]`:
  ```toml
  # jam rows
  [scenes.groove]
  slots = { kick = "k4", acid = "a1" }

  # hats alone
  [scenes.break]
  slots = { hat = "h" }
  ```
- `tune info`: text gains a line `scenes: groove (kick k4, acid a1), break (hat h)` (an empty scene reads `empty (no clips)`), none without scenes; JSON gains `"scenes": [{"id": "groove", "slots": {"kick": "k4", "acid": "a1"}}, …]`, `[]` without scenes.

- [ ] **Step 1: Write the failing tests** in `tune-song/tests/scenes.rs` (the session fixture via `include_str!`):
  - `scenes_parse_in_file_order`: `song.scenes.keys()` is `["groove", "break"]`; `groove.slots` is `{kick: k4, acid: a1}` with keys in that order; `break.slots` is `{hat: h}`; `parse_song` of `v2/mini.toml` has no scenes.
  - `scene_rules`: on copies of the fixture, each error at the line and column of the offending text: `slots = { bass = "k4" }` → "unknown track `bass`"; `{ acid = "nope" }` → "unknown clip `nope`"; `{ acid = "k4" }` → "clip `k4` is a drum clip; track `acid` (acid303) plays acid clips"; `[scenes.Drop]` → "scene id `Drop` may only contain a-z, 0-9 and _"; `name = "x"` in a scene table → the reader's unknown-key error naming `name`; a v1 file (`mini.toml`) with `[scenes.x]` appended → "this looks like a version 2 song: add `version = 2` to [song]".
  - `scenes_round_trip`: `to_toml` writes `\n[scenes.groove]\nslots = { kick = "k4", acid = "a1" }\n` after the last clip table and before the first `[[sections]]`; parsing it back gives an equal song with the same `scenes.keys()` order; an empty scene writes `[scenes.empty]\n` alone and parses back empty.
  - `song_cli.rs::info_lists_scenes`: `tune info <session.toml> --json` has the `scenes` array above; text has the `scenes:` line; `tune info` of `v2/mini.toml` has `"scenes": []` and no `scenes:` line; `tune validate <session.toml> --json` is ok.
- [ ] **Step 2: Run** `cargo test -p tune-song --test scenes` — Expected: FAIL (no `Song::scenes`).
- [ ] **Step 3: Implement** the model, reader, validator, writer and `tune info` line; fix every `Song { … }` literal (the validator's) to fill `scenes`.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-cli --test song_cli && cargo test -p tune-engine --test baseline` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(song): add scenes and their clip slots to the song format`.

### Task 2: The engine plays session clips and scenes (RISKY: bitwise, audio thread)

**Risk.** The frame loop changes where every track reads its cells. A track on the arrangement must take exactly today's path, and nothing new may allocate.

**Files:**
- Modify: `crates/tune-engine/src/{compile.rs,engine.rs,lib.rs}`
- Create: `crates/tune-engine/tests/session.rs`; Modify: `crates/tune-engine/tests/{common/mod.rs,live_no_alloc.rs}`

**Interfaces:**
- Consumes: `Song::scenes` (Task 1).
- Produces: `CompiledSong::{clip_ids, clip_kinds, scenes, clip_index, scene_index}`, `CompiledScene`, `TrackPlay`, `Cued`, `SessionCue`, `LAUNCH_QUANTUM_STEPS`, `Engine::{cue, session}` as decided.
- `Engine::cue` turns a `SessionCue` into per-track `Cued`: `Launch` → `Cued::Clip(clip)` on that track; `Scene(s)` → each track's slot `Some(c)` → `Cued::Clip(c)`, `None` → `Cued::Stop`; `Stop(t)` → `Cued::Stop`; `Back(Some(t))` → `Cued::Back` on `t`, `Back(None)` on every track. A new cue replaces the track's waiting one.
- The frame loop at a step boundary, in this order (an algorithm the tests do not fully fix):
  1. a pending swap, as today;
  2. the loop wrap: before `self.step = start`, `rebase(end, start)`; the song-end check as today;
  3. when `step % LAUNCH_QUANTUM_STEPS == 0`, `land_cues()`: each waiting cue sets the track's `play` (`Clip(c)` → `TrackPlay::Clip(CellSource { clip: c, start_step: step, offset_steps: 0 })`, `Stop` → `Stopped`, `Back` → `Arrangement`), clears the cue and re-applies that track's static params;
  4. static params on a region change, as today;
  5. `apply_lanes` skips a lane whose `track` is not `MIX_TRACK` while that track's `play` is not `Arrangement`; mix lanes always run;
  6. overrides, as today;
  7. per track, `src = source(song, rt.play, region, t)`: the region cell for `Arrangement`, `Some(src)` for `Clip(src)`, `None` for `Stopped`; `cut`, the step's cell and `notes_step` all take `src`. `cell_at(song, src: Option<CellSource>, step) -> &Cell` replaces `cell_at(song, region, t, step)`. The lookahead cell (`next`) is the arrangement's at `next_step` (wrap included) for an arrangement track, and the track's own source at `step + 1` otherwise (the clip plays on in its own time); it never looks at cues.
- `rebase(from: u64, to: u64)`: every track playing `Clip(src)` gets `offset_steps = (from - src.start_step + src.offset_steps) % clip_len` and `start_step = to`, `clip_len` the clip's steps in the current song (`Steps(cells).len()` or `NoteClipEvents::steps`). `seek(bar)` calls `rebase(self.step, 16 * bar)` before resetting. Invariant: `start_step <= step` for every session source, so no subtraction underflows.
- A finished engine keeps its cues; a seek lands them at the seek bar.

- [ ] **Step 1: Write the failing tests** in `tests/session.rs` (`common::{MINI, MIXED, song, compile_song}`; mini at 120 BPM and 48 kHz: a step is 6000 frames, a bar 96 000; tracks kick 0, acid 1, hat 2; clips k4 0, a1 1, h 2; `render(engine, frames)` processes in 512-frame blocks; "equal" means bit for bit; `{start, bars, clip, offset}` are `Placement` values, 0-based, offset 0 unless given; songs are edited in the model and checked with `variant_of`):
  - `scenes_compile_to_clip_indices`: the session fixture compiles to `scenes == [{id: "groove", slots: [Some(0), Some(1), None]}, {id: "break", slots: [None, None, Some(2)]}]`, `clip_ids == ["k4", "a1", "h"]`, `clip_kinds == [Drum, Acid, Drum]`.
  - `scenes_do_not_change_a_render`: `tune_engine::render` of the session fixture and of the same song with `scenes` cleared give equal mixes; `cargo test --test baseline` stays green (Step 4).
  - `cues_land_at_the_bar_line_they_reach`: `cue(Launch { track: 1, clip: 1 })` after 95 999 frames: `session(1)` is `(Arrangement, Some(Cued::Clip(1)))` after 96 000 frames and `(Clip(CellSource { clip: 1, start_step: 16, offset_steps: 0 }), None)` after 96 001; the same cue sent after exactly 96 000 frames lands at step 16 too; one sent after 96 001 lands at step 32.
  - `a_launch_plays_like_a_placement_and_ignores_the_tracks_lanes`: S = mini with acid's placements cleared (its cutoff lane kept); process 100 000 frames, `cue(Launch { 1, 1 })`, process to 384 000. R = mini with the lane removed (acid plays a1 over bars 2-3 from its placement). The two 4-bar mixes are equal.
  - `back_to_arrangement_picks_the_lanes_up_again`: S = mini; `cue(Launch { 1, 1 })` before the first frame (lands at step 0), `cue(Back(Some(1)))` after 150 000 frames. R = mini with acid's `play` = `[{start 0, bars 2, a1}, {start 2, bars 2, a1}]` (lane kept). Equal.
  - `stop_silences_until_back`: S = mini with the lane removed; launch at frame 0, `Stop(1)` after 100 000, `Back(Some(1))` after 200 000. R = that song with acid's `play` = `[{start 0, bars 2, a1}, {start 3, bars 1, a1, offset 1}]`. Equal.
  - `a_scene_switches_every_track`: S = mini plus scene `drop` = `{acid: a1, hat: h}`; `cue(Scene(0))` after 10 000 frames. R = mini with the lane removed, kick `[{start 0, bars 1, k4}]`, acid `[{start 1, bars 3, a1}]`, hat `[{start 0, bars 4, h}]`. Equal.
  - `ducking_follows_the_duck_sources_session_clip`: S = MIXED with kick's placements cleared; `cue(Launch { 0, 0 })` after 10 000 frames. R = MIXED with kick `[{start 1, bars 3, k4}]`. Equal.
  - `stopping_a_notes_clip_ends_its_notes`: S = mini with acid's placements cleared plus a notes clip `pad` (4 bars, one note `C2` with `len` 64 steps); `cue(Launch { 1, pad })` before the first frame, `cue(Stop(1))` after 10 000 frames (lands at bar 1). R = that song with the lane removed and acid's `play` = `[{start 0, bars 1, pad}]` (the placement's end sends `AllOff`). Equal.
  - `the_same_launches_give_the_same_samples`: two engines on mini; one cues `Launch { 1, 1 }` after 10 frames and `Stop(2)` after 20 frames, processing in 512-frame blocks; the other cues them after 90 000 and 95 000 frames, in 333-frame blocks; both land at bar 1 and the 4-bar mixes are equal.
  - `session_clips_keep_their_phase_across_a_wrap_and_a_seek`: mini plus a 24-step drum clip `c24` (index 3) launched on hat at frame 0, loop bars `0..2`: after 192 001 frames `session(2)` is `Clip(CellSource { clip: 3, start_step: 0, offset_steps: 8 })`; a fresh engine with no loop, after 120 000 frames (`step` 20) `seek(3)`: `Clip(CellSource { clip: 3, start_step: 48, offset_steps: 20 })`; neither panics.
  - `bad_cues_are_ignored`: `Launch { 1, 0 }` (a drum clip on acid303), `Launch { 9, 0 }`, `Scene(5)`, `Stop(9)` leave every `session(t)` at `(Arrangement, None)`; `session(9)` is `None`.
  - `a_finished_engine_lands_cues_at_the_seek_bar`: process past the end, `cue(Launch { 1, 1 })`, `seek(1)`, process one frame: `session(1).0 == Clip(CellSource { clip: 1, start_step: 16, offset_steps: 0 })`.
  - `live_no_alloc.rs::engine_cues_never_allocate`: under the flag, an `Engine` on mini gets `cue` of each `SessionCue` kind, plays across their landing, a loop wrap and a seek with a session clip playing, and the counts stay 0.
- [ ] **Step 2: Run** `cargo test -p tune-engine --test session` — Expected: FAIL (no `Engine::cue`).
- [ ] **Step 3: Implement** the compiled scenes and ids, `TrackPlay`/`Cued`/`SessionCue`, the runtime fields, `cue`, `session`, `land_cues`, `source`, the new `cell_at`, `rebase` at the wrap and the seek, and the lane skip.
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS, `baseline` included and unchanged.
- [ ] **Step 5: Run** `task verify`, then commit `feat(engine): launch session clips and scenes at bar lines`.

### Task 3: Session state across hot reloads and from the live controller (RISKY: audio thread)

**Files:**
- Modify: `crates/tune-engine/src/{engine.rs,live.rs}`, `crates/tune-engine/tests/{session.rs,live_no_alloc.rs}`

**Interfaces:**
- Consumes: Task 2's types.
- Produces: `LiveController::cue` and `Command::Cue { generation: u64, cue: SessionCue }`, drained like `Command::Override` (dropped unless `generation == engine.generation()`).
- `apply_swap`, for each new track taking an old runtime (old track `i`, new track `j`), before `carry_over`: `remap(play)`: `Clip(src)` → the new index of `old.clip_ids[src.clip]` (`new.clip_index`, string comparisons only) when it exists and `new.tracks[j].instrument.module.plays(new.clip_kinds[k])`, keeping `start_step` and `offset_steps`; otherwise `Stopped`; `Cued::Clip(c)` likewise, else `Cued::Stop`. A fresh runtime (new or renamed track) is `Arrangement` with no cue. When the swap moves the step (the jump to the loop start, or the end), `rebase` runs after the remap, on the new song.
- `Sequencer::carry_over(old, new, t, step, src)` takes the track's source in the new song at `step` (from `source` over the remapped `play`) instead of reading `new.regions`, so a session notes clip that plays on keeps its held notes.

- [ ] **Step 1: Write the failing tests** in `tests/session.rs`:
  - `a_launched_clip_follows_its_id`: launch a1 on acid; swap in mini with a drum clip `z` inserted first in the pool and a1's steps changed to `"C3 . . . . . . ."`: `session(1).0` names a1's new index 2 with the same `start_step`; a cue `Cued::Clip(1)` waiting at the swap becomes `Cued::Clip(2)`.
  - `a_clip_that_goes_stops_the_track`: launch a1, swap in mini without a1 (and without acid's placement): `session(1).0 == Stopped`; a waiting `Cued::Clip` becomes `Cued::Stop`.
  - `an_instrument_swap_stops_a_clip_it_cannot_play`: launch a1 on acid, swap acid's instrument to `kick`: `Stopped`; launch a notes clip instead, same swap: still `Clip`.
  - `a_renamed_track_follows_the_arrangement`: launch a1 on acid, swap in mini with acid renamed `bass`: `session` of the new track is `(Arrangement, None)`.
  - `a_value_swap_keeps_a_session_notes_clip_sounding`: Task 2's S with `pad` and kick and hat muted; launch `pad` on acid at frame 0; at 1.5 bars swap in the same song with hat's gain changed (a value swap: no crossfade, the held note plays on). The 4-bar mix equals the same run without the swap (`process_stems` refuses a swap, so the muting stands in for an acid stem).
  - `live_cues_reach_the_engine`: `live(S)` with Task 2's S of `a_launch_plays_like_a_placement_and_ignores_the_tracks_lanes`; pull 100 000 frames, `ctl.cue(Launch { 1, 1 })`, pull to 384 000: the frames equal that test's R. A cue sent after `ctl.swap(..)` and before that swap lands is dropped (`ctl.cue` is `Ok`, `session` never shows it).
  - `live_no_alloc.rs::process_never_allocates_or_frees` gains: controller cues of each kind, a swap that remaps a launched clip, one that removes it, a seek and a loop wrap with a session clip playing; counts stay 0.
- [ ] **Step 2: Run** `cargo test -p tune-engine --test session` — Expected: FAIL.
- [ ] **Step 3: Implement** the remap, the `carry_over` source, `Command::Cue` and `LiveController::cue`.
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(engine): keep session state across hot reloads and cue it live`.

### Task 4: Scene paths, diffs and `tune set` on a slot

**Files:**
- Modify: `crates/tune-studio/src/{path.rs,doc.rs,diff.rs}`, `crates/tune-studio/tests/{doc.rs,diff_log.rs,core_edits.rs}`, `crates/tune-cli/tests/edit_cli.rs`
- Modify: `CLAUDE.md` (the `tune set` path table: `scenes.<s>.slots.<t>` = clip id, empty clears; `scenes`, `scenes.<s>` read-only)

**Interfaces:**
- Consumes: `Song::scenes` (Task 1).
- Produces: `SongPath::{Scenes, Scene { scene }, SceneSlot { scene, track }}` as decided (`is_read_only` true for the first two; the `FromStr` error text lists `scenes.<scene>.slots.<track>`). `effective_value`: `Scenes` → the id list (`[]` without scenes); `Scene` → `{"slots": {…}}`, `None` for an unknown scene; `SceneSlot` → the clip id or `null` for an existing scene, `None` for an unknown one.
- `SongDoc::set(SceneSlot)`: a string writes `<track> = "<clip>"` into the scene's `slots` (in place when the key exists, appended otherwise; a scene without `slots` gets `slots = { <track> = "<clip>" }`); `null` (or `""`) removes the key, and the `slots` key when it was the last; an unknown scene is `EditError::NoSuchTarget`. Works on `slots` written inline or as a `[scenes.<s>.slots]` table (`toml_doc::Slot`). Unknown tracks and unplayable clips are the validator's.
- `diff_songs` as decided.

- [ ] **Step 1: Write the failing tests** (the session fixture):
  - `doc.rs::scene_paths_parse_and_print`: `scenes`, `scenes.groove`, `scenes.groove.slots.acid` round-trip through `FromStr`/`Display`; the first two are read-only; `scenes.Groove.slots.acid` and `scenes.groove.acid` are errors.
  - `doc.rs::scene_values`: `effective_value` gives `["groove", "break"]`, `{"slots": {"kick": "k4", "acid": "a1"}}`, `"a1"` for `scenes.groove.slots.acid`, `null` for `scenes.groove.slots.hat`, `None` for `scenes.nope.slots.acid`.
  - `doc.rs::set_a_slot_in_place`: `set(scenes.groove.slots.hat, "h")` makes the line `slots = { kick = "k4", acid = "a1", hat = "h" }` and leaves every other byte; `set(scenes.groove.slots.acid, null)` gives `slots = { kick = "k4" }`; `set(scenes.break.slots.hat, null)` removes `break`'s `slots` line and keeps `# hats alone` and `[scenes.break]`.
  - `doc.rs::slot_errors`: `scenes.nope.slots.acid` → `NoSuchTarget`; `scenes.groove.slots.acid = "k4"` → the validator's can't-play message; `scenes.groove.slots.bass = "k4"` → "unknown track `bass`"; the text is unchanged after each.
  - `diff_log.rs::diff_finds_scene_changes`: a scene `jam` added in the model → `scenes.jam` `null` → `{"slots": {}}`; `break` moved first → one `scenes` change `["groove", "break"]` → `["break", "groove"]`; groove's acid slot cleared → `scenes.groove.slots.acid` `"a1"` → `null`.
  - `core_edits.rs::a_song_with_scenes_opens_and_edits`: `StudioCore::open` on the fixture; `view()` builds; a `set_value tracks.acid.acid303.cutoff 820` writes and leaves the scene tables byte-identical (spec §10).
  - `edit_cli.rs::set_writes_a_slot`: `tune set <song> scenes.groove.slots.hat=h --json` gives `changes[0] == {path: "scenes.groove.slots.hat", old: null, new: "h"}`; `scenes.groove.slots.hat=` clears it.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test doc` — Expected: FAIL.
- [ ] **Step 3: Implement** the paths, values, the slot writer and the diff.
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli --test edit_cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): address scenes and slots by path`.

### Task 5: Scene actions

**Files:**
- Create: `crates/tune-song/src/scenes.rs`, `crates/tune-studio/src/doc/scenes.rs`
- Modify: `crates/tune-song/src/{edit.rs,lib.rs}` (error variants, `Removed.slots`, `RemovedSlot`), `crates/tune-song/tests/scenes.rs`, `crates/tune-studio/src/{doc.rs,action/mod.rs,action/catalog.rs,action/apply.rs}` (`removed_value` writes `slots`), `crates/tune-studio/tests/{actions.rs,protocol.rs}`, goldens, `crates/tune-cli/src/cmd/do_action.rs` (one `removed slot <clip> on <track> in scene <scene>` line per removed slot)
- Modify: `CLAUDE.md` (an actions table for the five scene actions, one example each)

**Interfaces:**
- Consumes: `SongPath::SceneSlot` and `SongDoc::set` (Task 4), `insert_table`/`Place` and `rename_keys` (`doc/items.rs`, made `pub(super)`).
- Produces: the scene edit functions, error variants, `Removed.slots`/`RemovedSlot` and the five edit actions as decided; `SongDoc::{add_scene, remove_scene, rename_scene, move_scene, set_slot}` in `doc/scenes.rs`, returning `Removed` for `remove_scene`. Every existing `Removed` result gains `"slots": []` on the wire (Plan 9's result tests update to it).
- Text: `add_scene` inserts `scene_table_toml` with `insert_table(doc, "scenes", id, …)`: `Place::Before(<scene now at index>)` (before its comments) or `Place::Last`, anchors `["song", "mix", "tracks", "clips"]`; `remove_scene` drops the table and the comments above it; `rename_scene` renames the key in place; `move_scene` gives the table the position right before the table of the scene now at `index` (after removal), or after the last scene table, renumbering positions as `insert_table` does, its comments travelling in its decor; a root `scenes = { … }` inline table refuses `move_scene` with "`move_scene` needs each scene as its own [scenes.<id>] table; edit this form by hand"; `set_slot` is `SongDoc::set(SceneSlot)`.
- Catalog (kind `edit`; help says `index` is 0-based): `add_scene` (`scene` id required, `index` int optional "0-based position among the scenes; the end by default"), `remove_scene` (`scene`), `rename_scene` (`scene`, `to`), `move_scene` (`scene`, `index` required), `set_slot` (`scene`, `track`, `clip` optional "empty clears the slot").

- [ ] **Step 1: Write the failing tests** (the session fixture; Plan 9's `comments_kept` helper):
  - `tune-song/tests/scenes.rs::scene_edits`: `add_scene(song, "jam", None)` appends an empty `jam`; `Some(0)` puts it first; `move_scene("break", 0)` gives keys `["break", "groove"]`; `set_slot("groove", "hat", Some("h"))` adds the slot, `None` on `acid` clears it; each message: "scene `groove` already exists", "a scene id is lower-case letters, digits and `_`, got `Jam`", "there is no scene `drop`", "there is no track `bass`", "there is no clip `nope`", "index 2 is out of bounds (the song has 2 scenes)".
  - `actions.rs::add_scene_writes_a_table`: `add_scene { scene: "jam", index: None }` writes `\n[scenes.jam]\n` right after `break`'s table and before the first `[[sections]]`; `index: Some(0)` writes it right before `# jam rows`; on `commented.toml` (no scenes) it goes right after `[clips.h]`'s table; every comment kept.
  - `actions.rs::remove_scene_takes_its_comments`: `remove_scene groove`: `# jam rows` and the table are gone, the rest byte-identical; result `{"removed": {"lanes": [], "placements": [], "slots": [{"scene": "groove", "track": "kick", "clip": "k4"}, {"scene": "groove", "track": "acid", "clip": "a1"}], "duck_source": null}}`.
  - `actions.rs::rename_and_move_scenes`: `rename_scene groove → drop` leaves `[scenes.drop]` right under `# jam rows` with the same `slots` line; `move_scene break 0` puts `# hats alone` and `[scenes.break]` before `# jam rows`, and the parsed `scenes.keys()` are `["break", "groove"]`.
  - `actions.rs::set_slot_from_the_cli`: `action_from_cli("set_slot", ["scene=groove", "track=acid", "clip="])` is `SetSlot { scene: "groove", track: "acid", clip: None }`; applied, the line reads `slots = { kick = "k4" }`.
  - `actions.rs::a_scene_batch_builds_on_itself`: `apply_to_file` with `[add_scene jam, set_slot jam acid a1]` writes once (one `file_hash`, actions `add_scene`, `set_slot`).
  - `protocol.rs`: one sample per new action in `sample_commands()` and `wire_name`; `every_protocol_action_is_reachable_from_tune_do` and `the_catalog_matches_the_enum` pass with them.
- [ ] **Step 2: Run** `cargo test -p tune-song --test scenes` — Expected: FAIL.
- [ ] **Step 3: Implement** the functions, writers, actions and catalog entries; regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-studio && pnpm -r test` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): add, remove, rename and move scenes and set slots as actions`.

### Task 6: Slots follow track, clip and instrument edits

**Files:**
- Modify: `crates/tune-song/src/edit.rs`, `crates/tune-song/tests/edit.rs`, `crates/tune-studio/src/{doc/items.rs,doc/scenes.rs}`, `crates/tune-studio/tests/actions.rs`, `crates/tune-cli/tests/do_cli.rs`
- Modify: `CLAUDE.md` (the track, clip and `set_instrument` rows say what happens to slots)

**Interfaces:**
- Consumes: Task 5's writers.
- Produces: `remove_track` removes the track's slot in every scene; `remove_clip` every slot holding the clip; `set_instrument` the track's slots whose clip the new module cannot play (a notes clip stays); each lists them in `removed.slots` (scene order, then the slot's order). `rename_track` renames the slot keys in place; `rename_clip` rewrites slot values in place. The text writers edit each scene's `slots` through `toml_doc::Slot`, other bytes unchanged.

- [ ] **Step 1: Write the failing tests** (the session fixture):
  - `actions.rs::remove_track_removes_its_slots`: `remove_track acid`: groove's line is `slots = { kick = "k4" }`; `results.removed.slots == [{"scene": "groove", "track": "acid", "clip": "a1"}]`.
  - `actions.rs::rename_track_renames_its_slots`: `rename_track acid → bass`: `slots = { kick = "k4", bass = "a1" }`.
  - `actions.rs::remove_clip_removes_its_slots`: `remove_clip h`: break's `slots` line is gone (`# hats alone` stays), `removed.slots` lists `{break, hat, h}`, and the two hat placements are listed as before.
  - `actions.rs::rename_clip_rewrites_slots`: `rename_clip a1 → bass1`: `acid = "bass1"` in groove.
  - `edit.rs::swap_removes_slots_it_cannot_play`: `set_instrument(acid, kick)` removes groove's acid slot and lists it; with groove's acid slot holding a notes clip instead, the slot stays.
  - `do_cli.rs::removed_slots_are_printed`: `tune do <song> remove_track track=acid` prints `removed slot a1 on acid in scene groove`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test actions` — Expected: FAIL.
- [ ] **Step 3: Implement** the clean-ups and the result field.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-studio && cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): keep slots valid when tracks, clips and instruments change`.

### Task 7: Scene undo targets

**Files:**
- Modify: `crates/tune-studio/src/target.rs`, `crates/tune-studio/src/doc/scenes.rs` (`put_scene`, `put_scene_order`), `crates/tune-studio/tests/targets.rs`

**Interfaces:**
- Consumes: Tasks 4 to 6.
- Produces: `Target::{Scene(String), SceneOrder}` with the strings and values decided (`FromStr` error text lists `scenes` and `scenes.<scene>`), `target_value` and `target_changes` for them, and `put`: `Scene` `null` removes the scene's table, a value inserts it at `index` (before the table of the scene now there, else after the last) in the `scene_table_toml` layout, replacing one that is there; `SceneOrder` reorders the scene tables to the value's order, scenes not in the value following in their current order (comments travel); a slot leaf goes through `set`.

- [ ] **Step 1: Write the failing tests** in `tests/targets.rs` (the session fixture):
  - `a_new_scene_is_one_target`: after `add_scene jam`: exactly `[scenes.jam: null → {"index": 2, "slots": {}}]`.
  - `slots_are_leaf_targets`: after `set_slot groove hat h`: exactly `[scenes.groove.slots.hat: null → "h"]`.
  - `a_scene_move_is_the_order_target`: after `move_scene break 0`: exactly `[scenes: ["groove", "break"] → ["break", "groove"]]`.
  - `remove_track_touches_its_slots`: after `remove_track acid`: `tracks.acid` and `scenes.groove.slots.acid` (`"a1"` → `null`), in that order.
  - `targets_round_trip_as_strings` gains `scenes`, `scenes.groove`, `scenes.groove.slots.acid`.
  - `put_restores_each_kind` gains one case for each of `add_scene`, `remove_scene`, `rename_scene`, `move_scene`, `set_slot`, and `remove_track` / `remove_clip` with slots: putting every `old` back, newest first, retrying a refused put once after the others, leaves `target_changes(fixture, restored)` empty and `scenes.keys()` in the fixture's order.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test targets` — Expected: FAIL.
- [ ] **Step 3: Implement** the targets and puts.
- [ ] **Step 4: Run** `cargo test -p tune-studio` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): record scenes and slots as undo targets`.

### Task 8: Run undo over scenes (RISKY: deletes nobody's work)

**Risk.** A run undo must never delete or overwrite a later edit by someone else. When in doubt it keeps the target and lists it.

**Files:**
- Modify: `crates/tune-studio/src/run_undo.rs`, `crates/tune-studio/tests/run_undo.rs`
- Modify: `CLAUDE.md` ("Undo a whole run": scenes and slots revert one by one, `rename_scene` goes back whole, the scene order is kept when anyone added, removed or moved a scene since; live actions are never part of a run)

**Interfaces:**
- Consumes: `Target::{Scene, SceneOrder}` (Task 7).
- Produces, in `run_undo.rs`: `owner` maps a slot leaf `scenes.<s>.slots.<t>` to `Scene(s)` when the run created it; `fold(Scene, slot leaf)` sets `after["slots"][t]` to the new clip, or removes the key for `null`; `holds(Track(t), slot leaf of track t)` is true; `overlap(SceneOrder, slot leaf)` is false (the prefix rule would say true); `approx` compares `Scene` values without `index`; `RENAMES` gains `rename_scene`.

- [ ] **Step 1: Write the failing tests** in `tests/run_undo.rs` (`StudioCore` on the session fixture; agent edits by `apply_now` with `Origin::claude(Some(r1))`, human edits by `apply_edit` + `flush`):
  - `scene_edits_revert`: r1 `add_scene jam`, `set_slot jam acid a1`, `move_scene jam 0`, `set_slot groove hat h`: the undo leaves `target_changes(fixture, now)` empty, `scenes.keys()` `["groove", "break"]`, `kept` empty, one undo step.
  - `a_slot_changed_later_is_kept`: r1 `set_slot groove kick` (no clip: clears it) and `set_value song.bpm 130`; then the producer sets `scenes.groove.slots.kick` to `"h"`: the undo keeps the slot (`changed_later`, by `human`, still `"h"`) and puts bpm back to 120. Second case: the producer sets it back to `"k4"` instead: the slot is listed in `already`, bpm reverts.
  - `a_scene_the_run_made_and_the_producer_filled_is_kept`: r1 `add_scene jam`; the producer `set_slot jam hat h`: `scenes.jam` kept, `changed_later` by `human`; `jam` stays with its slot.
  - `a_clip_the_run_made_and_the_producer_slotted_is_kept`: r1 `add_clip lead notes`; the producer `set_slot groove acid lead`: `clips.lead` kept, `still_used`, the validator's message.
  - `rename_scene_goes_back_whole`: r1 `rename_scene groove → drop`; undone alone, `groove` is back with its slots and comment; when the producer then `set_slot drop hat h`, the whole rename is kept and no `groove` appears beside `drop`.
  - `remove_track_reverts_with_its_slots`: r1 `remove_track acid`: the undo restores the track and groove's acid slot; `target_changes(fixture, now)` empty.
  - `rename_track_takes_its_slots_back`: r1 `rename_track acid → bass`: the undo restores `acid = "a1"` in groove's slots.
  - `the_order_is_kept_when_scenes_changed_since`: r1 `move_scene break 0`; the producer `add_scene jam`: `scenes` kept `changed_later` by `human`; with a slot edit by the producer instead, the move reverts.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test run_undo` — Expected: FAIL.
- [ ] **Step 3: Implement** the four rule changes.
- [ ] **Step 4: Run** `cargo test -p tune-studio` — Expected: PASS; every Plan 9 run-undo test unchanged.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): undo a run's scene edits, keeping later ones`.

### Task 9: Launch, stop and return from `tune do` (RISKY: concurrency)

**Risk.** Session cues cross from the actor to the audio thread while swaps may be in flight; a cue resolved against the wrong song launches the wrong clip.

**Files:**
- Create: `crates/tune-studio/src/session.rs`
- Modify: `crates/tune-studio/src/{studio.rs,action/mod.rs,action/catalog.rs,action/apply.rs,lib.rs}`, `crates/tune-studio/tests/{studio.rs,protocol.rs,actions.rs,run_undo.rs}`, goldens, `crates/tune-cli/tests/do_cli.rs`
- Modify: `CLAUDE.md` ("Working with the studio open": the four session actions, that they need the studio, land at the next bar line, are not undoable or logged, follow the rulings on empty slots, stopped transport and hot reloads)

**Interfaces:**
- Consumes: `LiveController::cue`, `SessionCue`, `CompiledSong::{clip_index, scene_index}` (Tasks 2, 3); the scene model (Task 1).
- Produces: the four live `Action` variants (catalog kind `live`; `launch_clip` args `track`, `scene`; `launch_scene` `scene`; `stop_clip` `track`; `back_to_arrangement` `track` optional "every track when left out"), their `targets()`, and in `session.rs`:
  ```rust
  /// Checks a session action against the open song: its track and scene exist, and a launched slot holds a clip.
  pub(crate) fn check(song: &Song, action: &Action) -> Result<(), EditOpError>;
  /// The engine cue for a checked session action, resolved against the song the engine plays.
  pub(crate) fn resolve(song: &CompiledSong, action: &Action) -> Option<SessionCue>;
  /// The tracks the studio has taken off the arrangement this playback.
  #[derive(Default)] pub(crate) struct OffArrangement(BTreeSet<String>);
  impl OffArrangement {
      pub(crate) fn apply(&mut self, action: &Action, song: &Song); // launch_clip/stop_clip add the track; launch_scene every track; back removes one or all
      pub(crate) fn contains(&self, track: &str) -> bool;
      pub(crate) fn retain(&mut self, song: &Song);                // drops ids the song no longer has
      pub(crate) fn clear(&mut self);
  }
  ```
- Actor: `handle_action` gets one arm for the four session actions (`self.session(&action)`), and the catch-all `edit => self.edit(&edit)` becomes an or-pattern naming every edit variant, so a new variant without an arm fails to compile (m2 from Plan 9's review). `session`: `check` (the refusal, through `EditError`, as edit refusals read); with no player, a launch calls `play(None)` first, and stop or back return `null`; push the action on `Player.cues: VecDeque<Action>`, update `OffArrangement`, `send_cues()`, return `null`. `send_cues` (also on every tick, after `send_swap`): while `player.retry.is_none()` and `controller.playing_generation() == player.sent_generation()`, pop the front, `resolve` against `player.latest()` (an action that no longer resolves is dropped with `Event::Error { message: "dropped `launch_clip`: there is no scene `groove` any more" }`), `controller.cue`; `QueueFull` puts it back and stops. `play` and `stop` clear `OffArrangement`; `song_changed` calls `retain`. `release` writes (no `{"automated": true}`) when the track is in `OffArrangement`.

- [ ] **Step 1: Write the failing tests** (`tests/studio.rs` helpers and the session fixture; `render_bars` as there):
  - `launches_land_at_bar_lines_in_the_studio`: play from bar 0; pull 48 000 frames; `call(LaunchClip { track: "acid", scene: "groove" })`; pull to 384 000. The frames equal `render_bars` of the fixture with acid's `play` = `[{ at = 2, clip = "a1", bars = 3 }]` and its `[[automation]]` table removed, bars 0..4.
  - `a_live_action_never_writes_the_file`: playing; each of `launch_clip`, `launch_scene groove`, `stop_clip hat`, `back_to_arrangement` through `call` and through `run_actions` with `Origin::claude(Some(r1))`: the file bytes and the log line count are unchanged, `can_undo` stays false, no `Applied` event arrives within 300 ms, and `EditLog::read_records` is empty.
  - `launching_while_stopped_starts_playback`: stopped, `seek_bar 1`; `launch_scene groove` → state `playing`, `cursor_bar == 1`; the frames pulled to the end equal `render_bars(R, 1, 4)` with R = the fixture with kick `[{ at = 2, clip = "k4", bars = 3 }]`, acid `[{ at = 2, clip = "a1", bars = 3 }]`, hat `[{ at = 1, clip = "h", bars = 1 }]` and no lane.
  - `stop_and_back_while_stopped_change_nothing`: stopped; `stop_clip acid` and `back_to_arrangement` return `null`, state not playing, file unchanged.
  - `session_action_errors`: "there is no scene `drop`", "there is no track `bass`", "scene `break` has no clip for track `acid`"; nothing is cued (the frames pulled afterwards equal the plain render).
  - `a_launch_during_a_reload_waits_for_the_swap`: playing; pull 24 000 frames; `call(SetTrackMix { track: "hat", gain_db: Some(-12.0), .. })` then at once `call(LaunchClip { acid, groove })` (the cue waits: the swap has not played); pull 6 000 frames (the swap lands at step 4); sleep 200 ms (the actor's ticks send the cue); pull to 384 000. Frames 24 000 to 384 000 equal those of `render_bars` of the edited song with acid's `play` = `[{ at = 2, clip = "a1", bars = 3 }]` and no lane (before step 4 the hat still played at the old gain).
  - `a_knob_release_on_a_launched_track_writes`: playing, acid launched, pulled into bar 2 (where acid's cutoff lane runs): a release of `acid303.cutoff` at 900 returns `null` and writes 900 (without the launch, the same release returns `{"automated": true}`).
  - `actions.rs::session_targets`: `LaunchClip { acid, groove }.targets() == ["scenes.groove.slots.acid"]`, `LaunchScene` → `["scenes.groove"]`, `StopClip { hat }` → `["tracks.hat"]`, `BackToArrangement { track: None }` → `[]`.
  - `protocol.rs`: samples for the four live actions; parity passes with no new exemption.
  - `run_undo.rs::launches_are_not_part_of_a_run_end_to_end`: through `run_actions`, r1 runs `launch_clip` and one `set_value`; `read_records` holds one record for r1 (`actions: ["set_value"]`); `undo_run r1` reverts it; r2 with only a `launch_scene` gives "no edits of run `r2` in the edit log".
  - `do_cli.rs::session_actions_need_the_studio`: `tune do <song> launch_scene groove --json` exits 2 with "`launch_scene` needs the studio: open the song with `tune serve <song>` or the app first"; `--batch` with `[{"cmd": "launch_scene", "args": {"scene": "groove"}}]` gives "a batch holds song edits only; run `launch_scene` on its own"; `tune actions --json` lists `launch_clip` with kind `live` and args `track` and `scene`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test studio` — Expected: FAIL.
- [ ] **Step 3: Implement** `session.rs`, the actions, the explicit arms, the cue queue and `send_cues`, the release rule; regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli && cargo test -p tune-desktop && pnpm -r test` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): launch clips and scenes and return to the arrangement from tune do`.

### Task 10: The toast words scene changes

**Files:**
- Modify: `apps/desktop/src/lib/describeChange.ts`, `apps/desktop/src/lib/describeChange.test.ts`

**Interfaces:**
- Consumes: the scene paths (Task 4) and targets (Task 7) as strings.
- Produces (copy verbatim): `describeChange`: `scenes` → `Scene order`; `scenes.<s>` → `Scene <s> added` (old `null`) or `Scene <s> removed` (new `null`); `scenes.<s>.slots.<t>` → `<t> in scene <s> ${span}` (`none → a1`). `describeTarget`: `scenes` → `the scene order`; `scenes.<s>` → `scene <s>`; `scenes.<s>.slots.<t>` → `<t>'s slot in scene <s>`; the `(by you)` / `(another run)` suffixes as for every target.

- [ ] **Step 1: Write the failing tests** in `describeChange.test.ts`: one case per form above, including `{path: "scenes.groove.slots.hat", old: null, new: "h"}` → `hat in scene groove none → h` and a kept `scenes.jam` by human → `scene jam (by you)`.
- [ ] **Step 2: Run** `pnpm --filter @tune/desktop test` — Expected: FAIL.
- [ ] **Step 3: Implement** the cases in `describeWith` and `describeTarget`.
- [ ] **Step 4: Run** `pnpm -r typecheck && pnpm -r test && pnpm lint` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(ui): describe scene and slot changes in the change toast`.

### Task 11: Exit 3, end to end

**Files:**
- Modify: `crates/tune-cli/tests/exit_cli.rs`

**Interfaces:**
- Consumes: everything above; `exit_cli.rs`'s `env_with`, `start_studio` (in-process studio with the capture device and `ControlServer::start` on the test's `TUNE_STUDIO_DIR`) and pulling helpers.
- S = the session fixture with its `[[automation]]` table removed. R = S with `scenes` removed and, 1-based: kick `[{ at = 1, clip = "k4", bars = 4 }]`, acid `[{ at = 2, clip = "a1", bars = 3 }]`, hat `[{ at = 1, clip = "h", bars = 1 }, { at = 3, clip = "h", bars = 2 }]`.

- [ ] **Step 1: Write the test** `a_running_studio_launches_and_returns_at_bar_lines` (spec §11 exit 3): `tune do <song> play --json`; pull 48 000 frames; `tune do <song> launch_scene groove --json` (`via: "studio"`; kick and acid launch, hat stops at bar 2); pull to 144 000; `tune do <song> launch_clip track=hat scene=break --json`; pull to 240 000; `tune do <song> back_to_arrangement --json`; pull to 384 000. The pulled frames equal `tune_engine::render` of R over bars 0..4, bit for bit; the song file and `edits.jsonl` are unchanged.
- [ ] **Step 2: Run** `cargo test -p tune-cli --test exit_cli` — Expected: PASS. A failure here is a bug in an earlier task: find it there (the landing step in Task 2, the cue sending in Task 9), never loosen the equality.
- [ ] **Step 3: Run** `task verify`, then commit `test(cli): launch a scene and a clip in a running studio and return at bar lines`.

---

## Self-review notes

- Spec coverage: §7.6 scenes as named rows with one slot per track referencing a pool clip (Task 1); two live states and launches at the next bar line, one-bar default (Task 2); stop silences until back, back to arrangement for one track or all at the current position (Task 2); session tracks ignore their own lanes, `mix.*` lanes run, ducking follows what the duck source plays (Task 2 `a_launch_plays_like_a_placement_and_ignores_the_tracks_lanes`, `back_to_arrangement_picks_the_lanes_up_again`, `ducking_follows_the_duck_sources_session_clip`); scenes saved, launches live and outside undo (Tasks 1, 9); a launched clip starts from its beginning at the launch bar and loops, determinism pinned (Task 2 `the_same_launches_give_the_same_samples`); room for recording (ruling). §7.3 slot compatibility (Task 1) and the instrument swap removing slots (Task 6). §8.1 scene edit actions (Task 5), live session actions (Task 9), stable ids and targets (Tasks 5, 9), `tune actions`/`tune do`, routing through the studio and the closed-studio error (Task 9), parity and generated types (Tasks 5, 9). §8.2 scene targets in run undo (Tasks 7, 8). §9 scene swaps at a step boundary and allocation-free launches (Tasks 2, 3). §10 a song with scenes in the current studio (Task 4 `a_song_with_scenes_opens_and_edits`; the UI ignores scenes). §11 exit 3 (Task 11); exit 5's parity half (Tasks 5, 9). §2.5 no session UI (ruling, Task 10 wording only). P2-G4's "offline renders play the arrangement only" (Task 2 `scenes_do_not_change_a_render`, `baseline`).
- Plan 9 review items: m2's explicit arms with a never-writes test (Task 9); run undo and targets for scenes in the adversarial style (Tasks 7, 8); `live_no_alloc` (Tasks 2, 3); v1 pins (Global Constraints, Task 2 Step 4); generated types and parity (Tasks 5, 9).
- Not here on purpose: a session grid, launch buttons or a scenes view in the UI (Phase 4), a launch-quantisation setting, recording a session into the arrangement, session state in `StudioState`.
