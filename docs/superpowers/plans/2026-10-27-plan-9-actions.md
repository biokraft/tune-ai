# Plan 9: The Action Layer, Undo and Runs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every song edit, transport control and undo a named, typed action that the studio UI, `tune do` and `tune set` all call; route CLI actions through a running studio (so they show up at once and join its undo history); generate the frontend's wire types from Rust; record per-write targets in the edit log so one agent run can be undone in one step while later work by anyone else is kept; and give the current studio an "Undo this run" button and ⌘Z everywhere outside text fields.

**Architecture:** `tune-studio` gains `action/` (the `Action` enum, its catalog, CLI parsing and the apply function), `control` (a JSON-lines TCP endpoint on 127.0.0.1 with a per-user secret that `tune serve` and the app start, and the client the CLI uses), `target` (the song flattened into undo targets, the diff between two songs as target changes, and `SongDoc::put` that writes a target back) and `run_undo`. Structural edits are pure functions in a new `tune-song/src/edit.rs` (like the section operations of `arrange.rs`); the studio writes their result into the text and commits it only when the text describes exactly that song. ⌘Z keeps today's byte-exact snapshot history; run undo reads one action record per write from `edits.jsonl`. The frontend's `protocol.ts` becomes a thin layer over a file `ts-rs` generates.

**Tech Stack:** Rust 2024 (rustc 1.97), `toml_edit` =0.25.17, `serde_json` 1, `ts-rs` =12.0.1 (new, `tune-studio` only, features `serde-json-impl`, `indexmap-impl`), `getrandom` =0.3.4 (already in `Cargo.lock`, now a direct dependency of `tune-studio`), React 18 + TypeScript + Vitest in `apps/desktop` and `packages/ui`.

**Spec:** `docs/superpowers/specs/2026-10-26-phase-2-modular-model-design.md` (the authority). This is the third of four Phase 2 plans (spec §12 (c)). It covers §8 (actions, `tune actions`/`tune do`, CLI routing to a running studio, the parity test, generated wire types, undo and runs, the edit log), the instrument-swap action of §7.3 (Plan 8 built its crossfade), §2.4 and §2.5 (run undo, ⌘Z, the toast button), §11 exits 1 and 4, and the parts of §10 these touch. Out of scope, for Plan 10: scenes, slots and their actions (add/remove/rename/reorder a scene, set a slot), and the live session actions (launch clip, launch scene, stop clip, back to arrangement). The catalog takes them additively.

## Planner rulings

- Ruling: the action layer lives in `tune-studio` (`src/action/`), and structural model edits in `tune-song/src/edit.rs` — actions need `SongDoc`'s comment-keeping writes, and the section operations already live in `tune-song` — cost if wrong: moving a module into a new crate later; no format change.
- Ruling: the wire keeps `{"cmd": …, "args": …}`; `Command` is either a `StudioCommand` (`list_songs`, `get_state`, `open_song`, `resolve_offered`) or an `Action` — the current transports keep working and the parity test's exemptions are a separate type — cost: none on disk; a later rename is a regenerated TS file.
- Ruling: the UI commands keep their names except `set_fx_mode`, which becomes `set_device_setting { track, device, setting, value }`, and the argument `pattern`, which becomes `clip` — v2 has clips, and devices have more settings than a mode — cost: four frontend call sites in Task 1.
- Ruling: `set_value { path, value }` is the one action for every `tune set` path, so the spec's "set mix fields" and "edit clip notes and steps" are `set_value` on `mix.*` and `clips.<c>.steps|notes|bars` — no duplicate actions over the same paths — cost: typed sugar actions can be added later without breaking anything.
- Ruling: `tune do` arguments are `name=value` with `tune set`'s TOML value rules; arguments typed id, param id, path or text take the text as given (a TOML-quoted string is unquoted); one argument without a name fills the action's first argument — this gives the spec's `tune do <song> undo_run <run id>` — cost: none.
- Ruling: a batch (`tune do --batch`, a JSON array on stdin; `tune set` with several assignments) is all or nothing and one undo step, and holds song edits only — `tune set a=1 b=2` stays one step as today, and the toast's Undo reverts what one CLI call did — cost: the producer cannot ⌘Z half a batch (run undo still works per target).
- Ruling: placement and lane positions (`at`, `to`) are 1-based bars as in the song file; transport bars (`bar`, `from_bar`, `bars`) and section indices stay 0-based as the protocol is today; every argument's help says which, and `at = 0` is refused with "bars are 1-based: `at` must be at least 1" — cost: an agent can still mix them up; the help and CLAUDE.md say it.
- Ruling: sections stay addressed by 0-based index; no section ids — v2 sections have none and adding them is a format change — cost: Phase 5 teach mode may need ids (additive).
- Ruling: the CLI reaches a studio through a JSON-lines TCP endpoint on 127.0.0.1 (ephemeral port) found via `~/.tune/studios/studio-<pid>-<port>.json` (dir 0700, file 0600; `TUNE_STUDIO_DIR` overrides the dir) holding the port and a fresh 256-bit hex secret; the CLI asks each endpoint which song it has open — spec §8.1's rules: localhost only, a per-user secret outside the repo, and a browser (which can only send HTTP, never a valid first line) is refused — cost: no 0600 guard on non-Unix hosts (not a target).
- Ruling: when several studios have the song open, the CLI uses the one with the newest endpoint file — cost: the other studio sees the edit through its watcher as today.
- Ruling: a request that reached a studio and got no answer within 5 s is an error ("the studio did not answer; check it before retrying"), never a fallback to writing the file — the studio may have applied it — cost: the agent retries by hand.
- Ruling: when `tune serve` or the app cannot start the endpoint, it prints a `warning:` and keeps running; CLI edits then write the file and arrive through the watcher as today — cost: those edits carry no run tag into the toast.
- Ruling: agent actions through the studio first write the producer's pending edits (their own undo step), then apply and write at once, one undo step per action or batch; the producer's edits keep the 150 ms burst; `undo_run` from the UI also writes at once — cost: none.
- Ruling: `set_param` from a non-human origin writes like `set_value` (no automation check, no live override); only the UI's knob release keeps the `{"automated": true}` refusal — cost: none.
- Ruling: ⌘Z history stays whole-file snapshots (byte-exact, comments kept, depth 500) and does not survive a restart; run undo works from one action record per write in `edits.jsonl`, so it survives restarts — §12 leaves the storage to the builder and §8.2 needs structure only for run undo — cost: none.
- Ruling: undo targets are leaf values (every `tune set` value path), whole items (`tracks.<t>`, `tracks.<t>.instrument`, `tracks.<t>.fx`, `clips.<c>`), element lists merged one element at a time (`tracks.<t>.play`, `automation.<t>.<param>`) and the section list (`sections`) — §8.2's "one value or one structural item"; placements and lane stretches have no ids, so each counts by its value — cost: a producer knob tweak on a device the run added keeps that device.
- Ruling: a revert the validator refuses (a run-made clip the producer placed, a device a later lane uses) is kept and listed as `still_used` — the generic way to detect "used" — cost: a run undo can keep more than ideal, never less.
- Ruling: leaf targets compare effective values (defaults filled in), so reverting to a default writes it explicitly — as `tune set` does — cost: a stray explicit `x = <default>` line.
- Ruling: removing an item drops its table and the comments above it; restoring a removed track or clip writes it in the minimal canonical layout without them — §5 forbids losing comments in migration, not in deletions — cost: a run undo of `remove_track` does not bring its comments back.
- Ruling: `remove_track` removes the lanes on its params and an explicit `mix.duck_source` naming it, `remove_effect` its lanes, `remove_clip` its placements, `set_instrument` the clean-up of §7.3 plus an explicit duck source naming the track when the new module plays no drum clips; each result lists what went; any other invalid result (e.g. `duck.depth` with no kick left) refuses the action with the validator's message — the §7.3 rule applied to every removal — cost: the agent sometimes needs a second action.
- Ruling: `rename_track` changes the track's noise seed (`track_seed(song.seed, id)`) and its help says so — §6 keeps ids through migration only — cost: hats and claps on a renamed track render different noise.
- Ruling: a new song starts from a hand-written minimal file (`[song]` with `version = 2`, `name`, `bpm`, and one `[[sections]]`); there is no `new_song` action — cost: none; `tune new` can come later.
- Ruling: `Action::targets()` returns the edit paths the UI already uses as highlight keys; that is the target-to-control mapping teach mode will use; no DOM attributes now — cost: Phase 5 adds `data-target` attributes.
- Ruling: run ids are 1 to 64 of `[A-Za-z0-9_.:-]`, titles one line of 1 to 200 characters; `--run`/`--run-title` win over `TUNE_RUN`/`TUNE_RUN_TITLE`; a title without an id is an error — cost: none.
- Ruling: a run undo is a new `undo_run` action by whoever asked (human from the toast, claude from the CLI) and carries the asker's run tag, if any — cost: none.
- Ruling: `duplicate_clip` copies the clip's values but not the comments above its table — cost: none.

## Global Constraints

- Run `task verify` before every commit. During TDD run the one test file: `cargo test -p <crate> --test <name>`.
- Every commit message is a Conventional Commit in normal prose and ends with exactly these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NTWqQ3vUQ6Q116sMFSQZbt
  ```
- "v1 songs sound different" is a failure. `crates/tune-engine/tests/baseline.rs` is never changed. This plan does not edit `tune-dsp`, `tune-modules` or `tune-engine` sources. Frozen fixtures (`crates/tune-song/tests/fixtures/{mini,mixed,first-acid-v1,first-acid-mixed}.toml`, `crates/tune-song/tests/fixtures/v2/*.toml`) and `songs/*/song.toml` are never edited.
- Determinism: same inputs give bitwise-identical output. The endpoint secret is the only randomness and never touches audio.
- Every edit changes only what it names and keeps every other byte of the file, comments included; every structural action computes the expected `Song` with its `tune-song` function, writes it into the text and commits with `SongDoc::commit_as(doc, Some(&expected))`, so a writer bug is `EditError::Internal`, never a wrong file. Errors from a commit are the validator's positioned messages.
- The undo depth stays `UNDO_DEPTH = 500`, the producer's burst `WRITE_DEBOUNCE = 150 ms`; one UI gesture (a knob drag, a burst of step clicks) stays one undo step.
- Runs (spec §8.2, verbatim names): `--run`/`--run-title`, or the `TUNE_RUN` and `TUNE_RUN_TITLE` environment variables. `tune do <song> undo_run <run id>` works with the studio open or closed.
- Parity exemptions (spec §8.1, by name, each with its reason in the test): `get_state`, `list_songs`, `open_song`, `resolve_offered`, and `set_param` with `commit: false`. Nothing else.
- `tune-studio` stays headless: no async runtime, no UI, no audio device code. Its network code is `std::net` on 127.0.0.1 only. `tokio`/`axum` stay in `tune-cli` and the Tauri host.
- New dependencies: `ts-rs = { version = "=12.0.1", features = ["serde-json-impl", "indexmap-impl"] }` and `getrandom = "=0.3.4"`, both in `tune-studio`. No others.
- A task that changes a wire type regenerates both goldens with `UPDATE_GOLDEN=1 cargo test -p tune-studio --test protocol --test wire_types` and reviews the diff; `apps/desktop/src/client/generated.ts` is never edited by hand.
- Every CLI command takes `--json` and exits 2 on failure. Every test that runs `tune` sets `TUNE_STUDIO_DIR` to a fresh temp dir, so no test reads or writes the real `~/.tune`.
- No new UI beyond spec §2.5: the toast's "Undo this run" (with its result line) and ⌘Z/⇧⌘Z. No add/remove buttons, device rack or session grid.
- Never commit audio, renders, screenshots, `scratch/`, absolute home paths or personal data; never read or print `tune.local.toml`; never print an endpoint secret. Anything that plays uses a capture device in tests or `--device null`. Never use bare `git stash`.

## Review Focus

1. **The CLI meets a stale endpoint, a studio on another song, or a studio that hangs.** A stale endpoint file is removed and the edit goes to the file; a studio on another song is ignored; a studio that took the request but never answered is an error and the file is not written behind its back. (Task 4 `stale_endpoint_files_are_removed`, `find_studio_matches_the_open_song`, `a_studio_that_does_not_answer_is_an_error_not_a_fallback`.)
2. **The producer works on what the run made before undoing it**: tweaks a knob on a run-made track, places a run-made clip, removes one of the run's placements, or changes a value the run changed twice. Later work stays and is listed as kept; the rest reverts. (Task 11 `an_item_the_run_created_and_the_human_changed_is_kept`, `a_clip_still_used_is_kept`, `placements_revert_one_by_one`, `the_run_after_a_human_edit_reverts_to_the_human_value`.)
3. **"Undo this run" pressed twice, or after ⌘Z already undid part of the run**: nothing reverts twice, no empty undo step is pushed, and the result says what was already undone. (Task 11 `undo_run_twice_changes_nothing`, `after_cmd_z_the_rest_reverts`.)
4. **A batch whose later actions build on earlier ones** (add a track, add an effect to it, set that effect's param), and one that fails in its last action: the first applies in order as one undo step; the second leaves the file, the log and the open studio untouched. (Task 5 `a_batch_builds_on_itself`; Task 3 `do_batch_is_all_or_nothing`; Task 4 `a_failing_batch_changes_nothing`.)
5. **An agent action lands while the producer drags a knob or has a step burst pending**: the pending burst is written first as its own step, the drag stays audible across the swap, and the release still writes. (Task 4 `pending_human_edits_are_written_before_an_agent_action`, `an_agent_action_during_a_drag_keeps_the_drag`.)

---

## Decided formats and interfaces

Every name here is binding for all tasks.

### Actions (`crates/tune-studio/src/action/`)

```rust
/// One action. Serde form: `{"cmd": "<snake_case name>", "args": {…}}`, `args` left out for
/// actions without arguments. Optional arguments are `#[serde(default)]` and serialize as null.
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
pub enum Action {
    // live (Task 1)
    Play { from_bar: Option<u32> }, Stop, SeekBar { bar: u32 }, SetLoop { bars: Option<[u32; 2]> },
    // history (Task 1)
    Undo, Redo,
    // song edits (Task 1)
    SetValue { path: SongPath, value: Value },
    SetParam { track: String, param: String, value: f64, #[serde(default = "yes")] commit: bool },
    SetDeviceSetting { track: String, device: String, setting: String, value: String },
    SetAcidStep { clip: String, index: usize, note: Option<u8>, accent: bool, slide: bool },
    SetDrumStep { clip: String, index: usize, hit: DrumHitDto },
    MoveSection { from: usize, to: usize }, ResizeSection { index: usize, bars: u32 },
    DuplicateSection { index: usize }, DeleteSection { index: usize },
    SetSectionPlay { section: usize, track: String, clip: Option<String> },
    SetTrackMix { track: String, gain_db: Option<f64>, pan: Option<f64>, mute: Option<bool>, solo: Option<bool> },
    // Task 5
    AddTrack { track: String, instrument: String }, RemoveTrack { track: String },
    RenameTrack { track: String, to: String },
    AddEffect { track: String, module: String, id: Option<String>, index: Option<usize> },
    RemoveEffect { track: String, device: String }, MoveEffect { track: String, device: String, index: usize },
    RenameDevice { track: String, device: String, to: String },
    // Task 6
    SetInstrument { track: String, module: String },
    // Task 7
    AddClip { clip: String, kind: String, steps: Option<String>, bars: Option<u32>, notes: Option<Value> },
    RemoveClip { clip: String }, RenameClip { clip: String, to: String }, DuplicateClip { clip: String, to: String },
    // Task 8 (`at`, `to` 1-based bars)
    AddPlacement { track: String, clip: String, at: u32, bars: u32, offset: Option<u32> },
    MovePlacement { track: String, at: u32, to: u32 }, ResizePlacement { track: String, at: u32, bars: u32 },
    RemovePlacement { track: String, at: u32 },
    AddSection { name: String, bars: u32, index: Option<usize> }, RenameSection { index: usize, name: String },
    // Task 9 (`param` as in the file: `acid.acid303.cutoff`, `mix.reverb.size`; `at` 1-based)
    AddLane { param: String, at: u32, bars: u32, points: Vec<[f64; 2]> },
    RemoveLane { param: String, at: u32 }, SetLanePoints { param: String, at: u32, points: Vec<[f64; 2]> },
    // Task 11
    UndoRun { run: String },
}
pub enum ActionKind { Edit, Live, History }                   // "edit" | "live" | "history"
pub enum ArgType { Id, ParamId, Path, Text, Int, Number, Bool, Json }   // "id" | "param_id" | …
pub struct ArgSpec { pub name: &'static str, pub ty: ArgType, pub required: bool, pub help: &'static str }
pub struct ActionSpec { pub name: &'static str, pub kind: ActionKind, pub help: &'static str, pub args: &'static [ArgSpec] }
pub const ACTIONS: &[ActionSpec];                             // catalog order = enum order
pub fn find_action(name: &str) -> Option<&'static ActionSpec>;
impl Action {
    pub fn name(&self) -> &'static str;                       // exhaustive match
    pub fn spec(&self) -> &'static ActionSpec;
    pub fn kind(&self) -> ActionKind;
    /// Edit paths this action changes, in the UI's highlight-key form.
    pub fn targets(&self) -> Vec<String>;
    /// `name=value` arguments `action_from_cli` reads back as this action.
    pub fn to_cli_args(&self) -> Vec<String>;
}
pub fn action_from_cli(name: &str, args: &[String]) -> Result<Action, String>;
pub fn parse_value(text: &str) -> Value;                      // moved from set.rs, unchanged
pub(crate) struct ApplyCtx<'a> { pub log: Option<&'a EditLog> }        // used from Task 11 on
pub(crate) fn apply_action(doc: &mut SongDoc, action: &Action, ctx: &ApplyCtx<'_>) -> Result<Value, StudioError>;
```

`apply_action` returns the action's result: `null` for most; `{"id": "<device id>"}` for `add_effect`; `{"removed": Removed}` for the removals; a `RunUndo` for `undo_run`. Live and history actions give `StudioError::NotAnEdit`.

`Removed` on the wire: `{"lanes": [{"param": "acid.acid303.cutoff", "at": 3, "bars": 2, "points": [[0, 0.1], [2, 0.8]]}], "placements": [{"track": "acid", "at": 3, "clip": "a1", "bars": 2, "offset": 0}], "duck_source": "kick" | null}` (`at` 1-based).

### Commands, origins and outcomes (`crates/tune-studio/src/{command.rs,action/mod.rs}`)

```rust
#[derive(Serialize)] #[serde(untagged)]
pub enum Command { Studio(StudioCommand), Action(Action) }    // Deserialize by hand: `cmd` in StudioCommand's names → Studio, else Action
impl From<Action> for Command;
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
pub enum StudioCommand { ListSongs, GetState, OpenSong { path: String }, ResolveOffered { reapply: bool } }

pub struct RunTag { pub id: String, pub title: Option<String> }        // serde, TS
impl RunTag { pub fn new(id: &str, title: Option<&str>) -> Result<Self, String>; }
pub struct Origin { pub source: Source, pub run: Option<RunTag> }
impl Origin { pub fn human() -> Self; pub fn claude(run: Option<RunTag>) -> Self; }
#[serde(rename_all = "lowercase")] pub enum Via { Studio, File }
pub struct ActionsOutcome { pub via: Via, pub changes: Vec<Change>, pub results: Vec<Value>, pub file_hash: Option<String>, pub migrated_from: Option<u32> }
// StudioError gains:
NeedsStudio { action: String }  // "`play` needs the studio: open the song with `tune serve <song>` or the app first"
EditsOnly { action: String }    // "a batch holds song edits only; run `play` on its own"
NoSuchRun(String)               // "no edits of run `r9` in the edit log"
NoAnswer                        // "the studio did not answer; check it before retrying"
```

`RunTag::new` errors: "run id must be 1 to 64 letters, digits or `_.:-`, got `has space`"; "a run title is one line of 1 to 200 characters".

### The edit log (`crates/tune-studio/src/log.rs`)

`EditEntry` (one line per changed path, as today) gains `action`, `run`, `run_title` (`Option<String>`, left out when `None`). From Task 10 every write also appends one action record, in the same `O_APPEND` write, after its entries:

```json
{"ts":"2026-10-27T10:00:00.000Z","source":"claude","record":"action","actions":["set_value"],"run":"r1","run_title":"open the filter","targets":[{"target":"tracks.acid.acid303.cutoff","old":500,"new":820}],"file_hash":"…"}
```

A record has no `path`, so `read_since` (and `tune changes`) skip it, and older logs without records stay readable.

### Undo targets (`crates/tune-studio/src/target.rs`, Task 10)

```rust
pub enum Target {
    Value(SongPath),                       // a leaf: song.bpm, tracks.<t>.<field>, tracks.<t>.<device>.<param|setting>,
                                           // tracks.<t>.send.delay …, mix.master, mix.duck_source, mix.<param>, clips.<c>.steps|notes|bars
    Track(String),                         // "tracks.<t>"
    Instrument(String),                    // "tracks.<t>.instrument"
    Fx(String),                            // "tracks.<t>.fx"
    Clip(String),                          // "clips.<c>"
    Play(String),                          // "tracks.<t>.play"
    Lanes { track: String, param: String },// "automation.<t>.<param id>", e.g. automation.acid.acid303.cutoff, automation.mix.reverb.size
    Sections,                              // "sections"
}   // Display/FromStr as the strings above; serde as that string
pub struct TargetChange { pub target: Target, pub old: Value, pub new: Value }   // null = absent
pub fn target_changes(old: &Song, new: &Song) -> Vec<TargetChange>;
pub fn target_value(song: &Song, target: &Target) -> Value;
impl SongDoc { pub fn put(&mut self, target: &Target, value: &Value) -> Result<(), EditError>; }
```

Values (JSON, every param and setting effective, defaults filled in):
- Device: `{"type": "<module>", "id": "<id>", "settings": {"<name>": "<value>"}, "params": {"<local>": <n>}}`.
- `tracks.<t>`: `{"index": <position>, "instrument": Device, "fx": [Device], "strip": {"send.delay": <n>, …}, "gain_db", "pan", "mute", "solo", "play": [Placement]}`; comparisons ignore `index`.
- `tracks.<t>.instrument`: Device. `tracks.<t>.fx`: `[Device]`.
- `clips.<c>`: `{"kind": "acid"|"drum", "steps": "<notation>"}` or `{"kind": "notes", "bars": <n>, "notes": [<note as effective_value gives it>]}`.
- Placement: `placement_json` (`{"at": 1-based, "clip", "bars", "offset"}`). `tracks.<t>.play`: `[Placement]`.
- `automation.<t>.<p>`: `[{"at": 1-based, "bars", "points": [[x, v], …]}]`. `sections`: `[{"name", "bars"}]`.
- A leaf: `effective_value`.

`target_changes` emits, in song-file order (bpm, mix, tracks in union order, clips, sections, lanes): leaf changes as `diff_songs` finds them; `tracks.<t>` for a track on one side only (nothing under it); for a track on both sides its field and strip leaves, `tracks.<t>.instrument` when the instrument's (type, id) differ (else its param and setting leaves), `tracks.<t>.fx` when the chain's [(id, type)] differ, then param and setting leaves of every fx device whose (id, type) is on both sides, and `tracks.<t>.play` when the placements differ; `clips.<c>` for a clip on one side only or whose kind changed (else its leaves); `sections` when the list differs; one `automation.<t>.<p>` per lane param whose stretches differ.

`put` writes: a leaf through `SongDoc::set`; `tracks.<t>` null removes the track table, a value inserts it at `index` (before the table of the track now at that index, else after the last track table) in the minimal layout; `tracks.<t>.instrument` replaces the instrument device; `tracks.<t>.fx` sets the chain to the value's (id, type) order, devices in both keeping their current entry (params, settings, comments), devices only in the value written from it, others removed; `clips.<c>` removes or inserts the clip table; `tracks.<t>.play` and `automation.<t>.<p>` write the list; `sections` rebuilds the `[[sections]]` list in the value's order, reusing for each entry the first unused current table with that name (comments kept, `bars` set) and writing new tables for the rest.

### Run undo (`crates/tune-studio/src/run_undo.rs`, Task 11)

```rust
pub struct RunUndo { pub run: String, pub title: Option<String>, pub reverted: Vec<TargetItem>, pub kept: Vec<KeptTarget>, pub already: Vec<Target> }
pub struct TargetItem { pub target: Target, pub item: Option<Value> }      // item: the placement or stretch, for list targets
pub struct KeptTarget { pub target: Target, pub item: Option<Value>, pub reason: KeptReason, pub by: Option<Source>, pub message: Option<String> }
#[serde(rename_all = "snake_case")] pub enum KeptReason { ChangedLater, StillUsed }
pub fn undo_run(doc: &mut SongDoc, records: &[ActionRecord], run: &str) -> Result<RunUndo, StudioError>;
```

### The control endpoint (`crates/tune-studio/src/control.rs`, Task 4)

```rust
pub const STUDIO_DIR_ENV: &str = "TUNE_STUDIO_DIR";
pub fn studio_dir() -> PathBuf;                       // $TUNE_STUDIO_DIR, else $HOME/.tune/studios
pub struct ControlServer { /* listener thread; Drop stops it and removes the endpoint file */ }
impl ControlServer { pub fn start(handle: StudioHandle, dir: &Path) -> io::Result<Self>; pub fn port(&self) -> u16; }
pub struct StudioLink { /* port, secret */ }
pub fn find_studio(dir: &Path, song: &Path) -> Option<StudioLink>;
impl StudioLink { pub fn run(&self, actions: &[Action], origin: &Origin) -> Result<ActionsOutcome, StudioError>; }
```

Endpoint file `<dir>/studio-<pid>-<port>.json`: `{"port": <u16>, "secret": "<64 lowercase hex from getrandom>", "pid": <u32>}`. One request per connection, one JSON object per line each way, lines at most 4 MiB, 5 s read timeout:
- `{"secret": "…", "op": "song"}` → `{"ok": true, "song": "<canonical path>" | null}`.
- `{"secret": "…", "op": "do", "actions": [Action…], "source": "claude", "run": RunTag | null}` → `{"ok": true, "outcome": ActionsOutcome}` or `{"ok": false, "error": "<message>"}`.
- A first line that is not JSON, lacks `secret`, or carries another one (compared in constant time) gets `{"ok": false, "error": "refused"}` and the connection closes; nothing reaches the studio.

`find_studio` reads every `studio-*.json` in `dir`, newest first; one it cannot connect to within 200 ms is deleted; it returns the first whose `song` equals `fs::canonicalize(song)`.

## File structure

- `crates/tune-studio/src/action/{mod.rs,catalog.rs,cli.rs,apply.rs}`: the enum, the catalog, CLI parsing, `apply_action` (today's `core::apply_command` moves here). `src/command.rs`: `Command`, `StudioCommand`. `src/control.rs`, `src/target.rs`, `src/run_undo.rs`, `src/wire.rs` (the TS rendering). `src/doc/items.rs`: the text writers for tracks, devices, clips, lanes and sections. `src/{core.rs,studio.rs,log.rs,set.rs,view.rs,error.rs,lib.rs}` change.
- `crates/tune-song/src/edit.rs`: the structural model edits. `src/arrange.rs` (`insert_section`, `LaneOrigin::New`), `src/write.rs` (`track_table_toml`, `clip_table_toml`).
- `crates/tune-cli/src/cmd/{actions.rs,do_action.rs}` new; `src/cmd/{set.rs,changes.rs,serve.rs}`, `src/main.rs` change.
- `apps/desktop/src-tauri/src/lib.rs`: start the endpoint. `apps/desktop/src/client/generated.ts` (generated), `client/protocol.ts`, `store/studioStore.ts`, `screens/{ChangeNotice.tsx,StudioScreen.tsx}`, `lib/{useStudioKeys.ts,describeChange.ts}`, dock call sites.
- Tests: `crates/tune-studio/tests/{actions.rs,wire_types.rs,control.rs,targets.rs,run_undo.rs}` new, `tests/protocol.rs` and the existing studio tests ported; `crates/tune-song/tests/edit.rs` new; `crates/tune-cli/tests/{do_cli.rs,exit_cli.rs}` new.

---

### Task 1: One action enum, its catalog, the parity test and `tune actions`

**Files:**
- Create: `crates/tune-studio/src/action/{mod.rs,catalog.rs,cli.rs,apply.rs}`, `crates/tune-studio/tests/actions.rs`
- Modify: `crates/tune-studio/src/command.rs` (whole file), `src/core.rs:39-75,233-257,403-441,477-511,690-865` (`PendingEdit.action: Action`, `apply_edit(&Action, …)`, `apply_command` moves to `action/apply.rs` as `apply_action`), `src/studio.rs:32,170-213,346-432,641-674` (dispatch `Command::Studio`/`Command::Action`), `src/set.rs:11-71` (`parse_value` moves to `action/cli.rs`), `src/lib.rs`
- Modify: `crates/tune-studio/tests/{protocol.rs,core_edits.rs,core_external.rs,diff_log.rs,studio.rs}`, `tests/fixtures/protocol.json` (regenerated), `apps/desktop/src-tauri/src/lib.rs` (test)
- Create: `crates/tune-cli/src/cmd/actions.rs`; Modify: `crates/tune-cli/src/{main.rs,cmd/mod.rs}`, `crates/tune-cli/tests/cli.rs`
- Modify: `apps/desktop/src/client/protocol.ts:7-78` (`set_value`, `set_device_setting`, `clip` args), `apps/desktop/src/screens/dock/{StepsTab.tsx,Dock.tsx,SoundTab.tsx}` and their tests
- Modify: `CLAUDE.md` (Layout: `tune-cli` lists `actions`, `do`; `tune-studio` "… the action layer, the edit log, undo …")

**Interfaces:**
- Produces: `Action` with the Task 1 variants, `ActionKind`, `ArgType`, `ArgSpec`, `ActionSpec`, `ACTIONS`, `find_action`, `Action::{name,spec,kind,targets,to_cli_args}`, `action_from_cli`, `parse_value`, `ApplyCtx`, `apply_action`, `Command`, `StudioCommand`, as decided above. Every later task adds its variants to all of these and a sample to `tests/protocol.rs`.
- `action_from_cli` rules: unknown name → "unknown action `x` (see `tune actions`)"; each argument splits at the first `=`; a key not in the spec → "`add_track` has no argument `foo` (it takes track, instrument)"; a repeated key → "`track` is given twice"; an argument without `=` is the spec's first argument, a second one → "only the first argument (`track`) may be given without a name"; a missing required one → "`add_track` needs `instrument`"; values of `Id`/`ParamId`/`Path`/`Text` arguments are the text (a TOML string literal is unquoted), others `parse_value`; then `serde_json::from_value(json!({"cmd": name, "args": {…}}))` (no `args` for an action without any), its error prefixed "bad arguments for `x`: "; `set_param` with `commit=false` → "`set_param` with commit=false is a knob preview for the studio UI; give the value without it".
- `targets()`: `set_value` → `[path]`; `set_param` → `[SongPath::param(track, param)]`; `set_device_setting` → `["tracks.<t>.<device>.<setting>"]`; steps → `["clips.<c>.steps[<i>]"]`; `resize_section` → `["sections[<i>].bars"]`; `set_section_play` → `["sections[<s>].play.<t>"]`; `set_track_mix` → one `tracks.<t>.<field>` per given field; `move_section`, `duplicate_section`, `delete_section` → `["sections"]`; live and history → `[]`.
- `tune actions [--json]`: JSON `{"actions": [{"name", "kind", "help", "args": [{"name", "type", "required", "help"}]}]}` in catalog order; text one line `name  kind  help` per action, then `    arg: type[, optional]  help` per argument.

- [ ] **Step 1: Write the failing tests.**
  - `tests/protocol.rs`: `wire_name` becomes an exhaustive match over `Command::Studio(..)` and every `Action` variant; `sample_commands()` holds one sample of each, now including `set_value` (`path = "tracks.acid.acid303.cutoff"`, `value = 820`) and `set_device_setting` (`acid`, `dist`, `mode`, `hard`), with `clip` arguments; the four existing tests keep their names.
  - `protocol.rs::every_protocol_action_is_reachable_from_tune_do`: for each sample, `exemption(&cmd)` (an exhaustive match naming `get_state` "the CLI reads the file and `tune info`", `list_songs` and `open_song` "the CLI names the song file", `resolve_offered` "the conflict prompt only a human sees", `set_param` with `commit: false` "a knob preview during a drag; the agent sets the final value") is `Some` or `action_from_cli(a.name(), &a.to_cli_args()) == Ok(a)`; plus a `set_param` sample with `commit: false` that is exempt and that `action_from_cli` refuses.
  - `protocol.rs::the_catalog_matches_the_enum`: `ACTIONS` names equal the sample names in order; for each sample the serde `args` keys are a subset of its spec's arg names and include every required one; `find_action("nope")` is `None`.
  - `tests/actions.rs::cli_arguments`: `action_from_cli("set_section_play", ["section=1", "track=kick", "clip="])` is `SetSectionPlay { section: 1, track: "kick", clip: None }`; `("set_value", ["path=song.bpm", "value=130"])` has `value == json!(130)`; `("rename_section", ["index=0", "name=2024"])` has `name == "2024"`; `("seek_bar", ["3"])` is `SeekBar { bar: 3 }`; each error message above, verbatim.
  - `tests/actions.rs::targets_are_highlight_keys`: the `targets()` of `set_param`, `set_acid_step { clip: "a1", index: 3, … }` (`["clips.a1.steps[3]"]`) and `set_track_mix { gain_db: Some(-3.0), mute: Some(true), .. }` (`["tracks.<t>.gain_db", "tracks.<t>.mute"]`).
  - `crates/tune-cli/tests/cli.rs::actions_lists_the_catalog`: `tune actions --json` lists `set_value` with kind `edit` and args `path` (`path`, required) and `value` (`json`, required), and `play` with kind `live`.
  - Port every existing test to `Command::Action(..)`/`Command::Studio(..)`, `clip`, `set_device_setting`; keep names and intent.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test protocol` — Expected: FAIL (no `Action`).
- [ ] **Step 3: Implement** the modules, the hand-written `Deserialize` for `Command`, the dispatch in the actor (an `Action` of kind `Edit` goes through `apply_edit` as today; live and history as today's commands), `set_value` through `SongDoc::set`, `set_device_setting` as `SongDoc::set(TrackParam { param: "<device>.<setting>" })`, the help texts (every bar argument says 0-based or 1-based), `tune actions`; update `protocol.ts` and the three dock call sites; regenerate the golden.
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli && pnpm -r test` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): define every studio command as a named action`.

### Task 2: Generate the frontend's wire types

**Files:**
- Modify: `crates/tune-studio/Cargo.toml` (`ts-rs`), `src/{command.rs,action/mod.rs,view.rs,studio.rs,core.rs,diff.rs,error.rs,log.rs}` (`#[derive(TS)]`), `src/lib.rs`
- Create: `crates/tune-studio/src/wire.rs`, `crates/tune-studio/tests/wire_types.rs`, `apps/desktop/src/client/generated.ts` (generated)
- Modify: `apps/desktop/src/client/protocol.ts` (whole file), `.prettierignore`, `CLAUDE.md` (Layout, `apps/desktop`: "`src/client/generated.ts` is generated from `tune-studio`; regenerate with `UPDATE_GOLDEN=1 cargo test -p tune-studio --test wire_types`")

**Interfaces:**
- Consumes: Task 1's types.
- Produces: `pub fn wire_types_ts() -> String` (in `wire.rs`): the line `// Generated by \`UPDATE_GOLDEN=1 cargo test -p tune-studio --test wire_types\` from crates/tune-studio. Do not edit.`, a blank line, then `export ` + each type's ts-rs declaration (`TS::decl`, ts-rs 12; check its API with context7) in this order: the declarations ts-rs needs for `serde_json::Value` (`JsonValue`, from `serde-json-impl`), `Source`, `DrumHitDto`, `ErrorView`, `Change`, `ExternalChange`, `ParamView`, `FxView`, `PlacementView`, `TrackView`, `MixView`, `AcidStepView`, `NoteView`, `PatternView`, `LaneView`, `SectionView`, `SongView`, `StudioState`, `Event`, `Action`, `StudioCommand`, `Command`, then `export const ACTION_NAMES = [<catalog names>] as const;` and `export const STUDIO_COMMAND_NAMES = ["list_songs", "get_state", "open_song", "resolve_offered"] as const;`. Later tasks append their new wire types (`RunTag`, `RunUndo`, `TargetItem`, `KeptTarget`, `KeptReason`) before `Event`.
- Type mapping: `u64` fields (`Event::Playhead.step`) get `#[ts(type = "number")]`; fields of type `SongPath` (and later `Target`) get `#[ts(type = "string")]`; `serde_json::Value` maps through `serde-json-impl`. If `indexmap-impl` does not accept `indexmap` 2.14, annotate the `IndexMap` fields with `#[ts(type = …)]` instead of adding a crate.
- `protocol.ts` keeps its public names (`Command`, `StudioEvent`, `StudioState`, `SongView`, …, `Json`, `COMMAND_NAMES = [...STUDIO_COMMAND_NAMES, ...ACTION_NAMES]`, `EVENT_NAMES` with its compile-time completeness check), now re-exported or derived from `generated.ts`; `StateEvent`, `AppliedEvent` and the others are `Extract<StudioEvent, { event: "…" }>`. Where `@tune/ui` declares a structurally different type, fix the difference in `protocol.ts`, never in `generated.ts`.

- [ ] **Step 1: Write the failing test** `tests/wire_types.rs::generated_types_are_current`: `wire_types_ts()` equals the text of `apps/desktop/src/client/generated.ts`; with `UPDATE_GOLDEN` set it writes the file first (as `protocol.rs` does).
- [ ] **Step 2: Run** `cargo test -p tune-studio --test wire_types` — Expected: FAIL (no `wire_types_ts`).
- [ ] **Step 3: Implement** the derives, `wire.rs`, the generated file, the new `protocol.ts`, and add `apps/desktop/src/client/generated.ts` to `.prettierignore`.
- [ ] **Step 4: Run** `UPDATE_GOLDEN=1 cargo test -p tune-studio --test wire_types`, then `cargo test -p tune-studio && pnpm -r typecheck && pnpm -r test && pnpm lint` — Expected: PASS; `protocol.test.ts` still finds every golden command in `COMMAND_NAMES`.
- [ ] **Step 5: Run** `task verify`, then commit `feat(ui): generate the studio wire types from Rust`.

### Task 3: `tune do`, runs and the edit log, with the studio closed

**Files:**
- Modify: `crates/tune-studio/src/{action/mod.rs,set.rs,log.rs,error.rs,lib.rs}` (`RunTag`, `Origin`, `Via`, `ActionsOutcome`, `apply_to_file`, `set_paths` on top of it, `EditEntry` fields, `EditLog::append_changes` takes the action name and run)
- Create: `crates/tune-cli/src/cmd/do_action.rs`, `crates/tune-cli/tests/do_cli.rs`
- Modify: `crates/tune-cli/src/{main.rs,cmd/set.rs,cmd/changes.rs,cmd/mod.rs}`, `crates/tune-cli/tests/edit_cli.rs`
- Modify: `CLAUDE.md` ("Working with the studio open": `tune do`, `--batch`, runs; `tune set` is a shorthand for `set_value`)

**Interfaces:**
- Consumes: `action_from_cli`, `apply_action` (Task 1).
- Produces: `RunTag`, `Origin`, `Via`, `ActionsOutcome`, `StudioError::{NeedsStudio,EditsOnly}`, `EditEntry.{action,run,run_title}` as decided, and
  ```rust
  /// All or nothing: applies every action to one document, writes the file once and logs one
  /// entry per change, each with its action's name and the origin's run. A file in an older
  /// format is written in the newest with a `song.version` change first (no action name).
  /// No change: nothing written or logged. Live or history action: `NeedsStudio`.
  pub fn apply_to_file(song_path: &Path, actions: &[Action], origin: &Origin) -> Result<ActionsOutcome, StudioError>;
  ```
  `set_paths(path, assignments, source)` becomes `apply_to_file` with one `SetValue` per assignment and `Origin { source, run: None }`, returning the same `SetOutcome`.
- CLI: `tune do <SONG> [ACTION] [ARG]... [--batch] [--run ID] [--run-title TEXT] [--json]`. `--batch` reads a JSON array of action objects from stdin and conflicts with `ACTION`; a batch with a live or history action is `EditsOnly`. `tune set` gains `--run`/`--run-title`. Without the flags, `TUNE_RUN`/`TUNE_RUN_TITLE` are read with `std::env`; edits from the CLI are `Source::Claude`. JSON output: `{"ok": true, "via": "file", "changes": […], "results": […], "file_hash": "…"}` plus `migrated_from` for a migrated file; text: one `set <path> <old> -> <new>` line per change as `tune set` prints them, "no changes" when none, then one line per non-null result (`removed lane acid.acid303.cutoff at bar 3`, …). `tune changes` text lines end with ` (run <id>)` for an entry with a run; its JSON entries carry the new fields.

- [ ] **Step 1: Write the failing tests** in `tests/do_cli.rs` (a temp copy of `crates/tune-studio/tests/fixtures/commented.toml`; `TUNE_STUDIO_DIR` set to a temp dir):
  - `do_sets_a_value_and_logs_the_run`: `tune do <song> set_value path=tracks.acid.acid303.cutoff value=820 --run r1 --run-title "open the filter" --json` gives `via == "file"`, `changes[0] == {path: "tracks.acid.acid303.cutoff", old: 500, new: 820}`; the last log line has `source: "claude"`, `action: "set_value"`, `run: "r1"`, `run_title: "open the filter"`; the comments `# the body`, `# the squelch`, `# acid enters` are still in the file.
  - `do_reads_the_run_from_the_environment`: with `TUNE_RUN=r2` the entry has `run: "r2"`; with `TUNE_RUN=r2` and `--run r3`, `r3`.
  - `do_batch_is_all_or_nothing`: stdin `[{"cmd":"set_value","args":{"path":"song.bpm","value":130}},{"cmd":"set_value","args":{"path":"tracks.nope.gain_db","value":1}}]` with `--batch` exits 2 naming `tracks.nope.gain_db`, the file bytes and the log are unchanged; the same with a valid second action writes once: two entries (actions `set_value`) with the same `file_hash`.
  - `a_batch_with_a_live_action_is_refused`: `[{"cmd":"stop"}, …]` → exit 2, "a batch holds song edits only; run `stop` on its own".
  - `live_action_without_a_studio_fails`: `tune do <song> play --json` and `tune do <song> undo --json` exit 2 with "`play` needs the studio: open the song with `tune serve <song>` or the app first" (`undo` likewise); `tune do <song> seek_bar 3` reaches the same error (the positional parsed).
  - `bad_runs_are_refused`: `--run "has space"` and `--run-title x` without a run exit 2 with their messages; nothing is written.
  - `set_takes_a_run`: `tune set <song> song.bpm=130 --run r4` logs `run: "r4"`, `action: "set_value"`.
  - `changes_show_the_run`: after the first test's edit, `tune changes <song>` prints a line ending ` (run r1)`.
  - `edit_cli.rs`: every existing test passes unchanged (only `TUNE_STUDIO_DIR` added to its `tune()` helper).
- [ ] **Step 2: Run** `cargo test -p tune-cli --test do_cli` — Expected: FAIL (no `do` subcommand).
- [ ] **Step 3: Implement** `apply_to_file`, the log fields, `tune do`, the run flags and the `tune changes` suffix.
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(cli): run actions with tune do and tag them with a run`.

### Task 4: CLI actions go through a running studio (RISKY: security, concurrency)

**Risk.** The endpoint accepts edits from outside the process. It must listen on 127.0.0.1 only, refuse anything without the secret before parsing a request, never print the secret, and never let a CLI request and the producer's pending edits interleave inside one undo step.

**Files:**
- Create: `crates/tune-studio/src/control.rs`, `crates/tune-studio/tests/control.rs`
- Modify: `crates/tune-studio/Cargo.toml` (`getrandom`), `src/studio.rs` (`Msg::Actions`, `StudioHandle::run_actions`, `Event::Applied.run`, `announce_applied(source, run, changes)`), `src/core.rs` (`apply_now`; `undo`/`redo` log with a given source), `src/lib.rs`
- Modify: `crates/tune-cli/src/cmd/{do_action.rs,set.rs,serve.rs}`, `crates/tune-cli/tests/do_cli.rs`, `apps/desktop/src-tauri/src/lib.rs`, `tests/fixtures/protocol.json`, `apps/desktop/src/client/generated.ts` (regenerated)
- Modify: `CLAUDE.md` ("Working with the studio open": with the studio open, `tune do` and `tune set` go through it, show up at once and are one undo step each; live actions need it; the endpoint files under `~/.tune/studios`)

**Interfaces:**
- Consumes: `ActionsOutcome`, `Origin`, `apply_action` (Tasks 1, 3).
- Produces: the control endpoint as decided, and
  ```rust
  impl StudioHandle {
      /// Runs one action or a batch as `origin`. Edits: as `StudioCore::apply_now`. One live or
      /// history action: as the UI's command, logged with `origin.source`. Waits at most 5 s.
      pub fn run_actions(&self, actions: Vec<Action>, origin: Origin) -> Result<ActionsOutcome, StudioError>;
  }
  impl StudioCore {
      /// Writes the pending edits first (their own `human` step), then applies every action to a
      /// copy (all or nothing), writes at once, pushes one undo step with `origin.source`, clears
      /// redo and logs the entries with each action's name and the run. `FileInvalid` while the
      /// file does not parse.
      pub fn apply_now(&mut self, actions: &[Action], origin: &Origin) -> Result<ActionsOutcome, StudioError>;
  }
  // Event::Applied { source: Source, changes: Vec<Change>, run: Option<RunTag> }
  ```
  `set_param` with a non-human origin runs as `set_value` (no automation check, no override change). The actor sends `Applied { source: origin.source, run }` and swaps the engine as for any edit.
- Hosts: `tune serve` and the Tauri app call `ControlServer::start(handle.clone(), &studio_dir())` after spawning the studio and keep it alive until shutdown (Tauri: managed state); on failure `serve` prints `warning: CLI edits will write the file directly: <error>` to stderr (the app logs it) and goes on.
- CLI routing (`do_action.rs`, used by `tune set` too): `find_studio(&studio_dir(), song)`; `Some(link)` → `link.run(actions, origin)` (`via: "studio"`); `None` → `apply_to_file` for edits, `NeedsStudio` for live and history actions. An error after the request was sent (`NoAnswer`, or the studio's `{"ok": false}`) is reported, never retried against the file.

- [ ] **Step 1: Write the failing tests** in `tests/control.rs` (studios on temp copies of the commented fixture with the capture device of `tests/studio.rs`; endpoint dirs are temp dirs):
  - `endpoint_file_names_a_secret_and_goes_with_the_server`: after `start`, the dir holds one `studio-<pid>-<port>.json` with that `port`, a 64-hex `secret` and this `pid`; mode `0o600`, dir mode `0o700`; after drop it is gone.
  - `find_studio_matches_the_open_song`: `Some` for the open song, also given as `<dir>/../<dir name>/song.toml`; `None` for another file.
  - `a_wrong_secret_or_http_is_refused`: over a raw `TcpStream`, `GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://evil.example\r\n\r\n` and `{"secret":"00","op":"do","actions":[…]}` each get `{"ok":false,"error":"refused"}` and a closed connection; the song file is unchanged.
  - `stale_endpoint_files_are_removed`: a `studio-1-9.json` naming a port nothing listens on: `find_studio` returns `None` and the file is gone.
  - `a_studio_that_does_not_answer_is_an_error_not_a_fallback`: a test listener that reads the request and never replies: `StudioLink::run` returns `StudioError::NoAnswer` after 5 s.
  - `actions_through_the_link_are_one_undo_step_each`: `run([set_value cutoff 820], claude r1)` writes the file before returning (no debounce), the subscriber gets `Applied { source: claude, run: Some(RunTag { id: "r1", .. }) }`, the log entry has `run: "r1"`; then `run([set_value song.bpm 130, set_value tracks.acid.gain_db -6], claude r1)`; `call(Undo)` puts bpm and gain back with cutoff still 820; a second `Undo` puts cutoff back to 500 (spec §11 exit 4: ⌘Z steps one action at a time).
  - `pending_human_edits_are_written_before_an_agent_action`: a committed knob (pending) then an agent action: the log holds the `human` entry before the `claude` one; `Undo` reverts the agent's step, the next `Undo` the knob.
  - `a_failing_batch_changes_nothing`: `[set_value song.bpm 130, set_value tracks.nope.gain_db 1]` errs; file bytes, log and `get_state` unchanged; no `Applied` event.
  - `live_actions_through_the_link`: `[play { from_bar: Some(1) }]` → state `playing`, `cursor_bar == 1`; `[stop]` → not playing; `[undo]` from claude logs `source: "claude"`.
  - `agent_set_param_ignores_automation`: with the cursor in bar 3 (automated `acid303.cutoff`), a human release returns `{"automated": true}` and writes nothing; the same `set_param` from claude writes 820.
  - `an_agent_action_during_a_drag_keeps_the_drag`: playing; a drag on `acid303.cutoff` to 2000; an agent `set_value tracks.hat.gain_db -12`; after the swap lands the drag is still heard, checked as `studio.rs::drag_during_reload_is_resent` checks it (the pulled frames differ from an offline render of the edited song), and the release writes 2000.
  - `do_cli.rs::do_goes_through_a_running_studio`: an in-process studio (`NullOut`) with `ControlServer::start` on the test's `TUNE_STUDIO_DIR`: `tune do <song> set_value path=song.bpm value=130 --run r5 --json` gives `via: "studio"`, the subscriber sees `Applied` with run `r5`; `tune do <song> play --json` succeeds and the state is playing; `tune set <song> song.bpm=128 --json` gives `via: "studio"`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test control` — Expected: FAIL (no `control`).
- [ ] **Step 3: Implement** the endpoint (a listener thread, one short-lived thread per connection), `run_actions`, `apply_now`, the `Applied.run` field, the routing, and the two hosts; regenerate both goldens.
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli && cargo test -p tune-desktop && pnpm -r test` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): route CLI actions through a running studio`.

### Task 5: Track and device actions

**Files:**
- Create: `crates/tune-song/src/edit.rs`, `crates/tune-song/tests/edit.rs`, `crates/tune-studio/src/doc/items.rs`
- Modify: `crates/tune-song/src/{lib.rs,write.rs,arrange.rs}` (`track_table_toml`, `LaneOrigin::New` with `write_automation` support), `crates/tune-studio/src/{doc.rs,doc/arrange.rs,action/*}`, `crates/tune-studio/tests/actions.rs`, goldens
- Modify: `CLAUDE.md` (a short "Actions" list under "Working with the studio open": the track and device actions with one example each)

**Interfaces:**
- Produces (tune-song):
  ```rust
  pub struct Edited { pub arranged: Arranged, pub removed: Removed }
  #[derive(Default)] pub struct Removed { pub lanes: Vec<Lane>, pub placements: Vec<(String, Placement)>, pub duck_source: Option<String> }
  #[derive(thiserror::Error)] pub enum EditOpError { /* one variant per message below */ }
  pub fn add_track(song: &Song, registry: &Registry, id: &str, module: &str) -> Result<Edited, EditOpError>;
  pub fn remove_track(song: &Song, id: &str) -> Result<Edited, EditOpError>;
  pub fn rename_track(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError>;
  pub fn add_effect(song: &Song, registry: &Registry, track: &str, module: &str, id: Option<&str>, index: Option<usize>) -> Result<Edited, EditOpError>;
  pub fn remove_effect(song: &Song, track: &str, device: &str) -> Result<Edited, EditOpError>;
  pub fn move_effect(song: &Song, track: &str, device: &str, index: usize) -> Result<Edited, EditOpError>;
  pub fn rename_device(song: &Song, track: &str, device: &str, to: &str) -> Result<Edited, EditOpError>;
  pub fn track_table_toml(id: &str, track: &Track) -> String;   // `[tracks.<id>]`: instrument, then only non-default fields, params, fx, play
  pub enum LaneOrigin { Kept(usize), Split(usize), Copy(usize), New }   // New: a stretch with no old table
  ```
  Like the section operations, these do not validate; the studio commits their result through the validator. A new track: instrument device id = the module's `prefix`, gain 0, pan 0, not muted or soloed, no fx or placements, appended last. `add_effect`: id default `Track::next_device_id`, index default the end. `rename_track` keeps the track's position and rewrites its lanes and an explicit `mix.duck_source`. `rename_device` (instrument or effect) rewrites the device's param keys and its lanes.
- Messages (`EditOpError` Display): "track `acid` already exists"; "there is no track `bass`"; "`mix` is reserved for the mix params"; "a track id is lower-case letters, digits and `_`, got `Bass`" (same form for device and clip ids); "unknown instrument `acid3030` (expected acid303, kick, clap, hat_closed, hat_open or ride)" (the list from the registry); "unknown effect `reverb` (expected distortion or eq)"; "`eq` is an effect, not an instrument" and the reverse; "track `acid` has no device `eq3`"; "track `acid` already has a device `eq`"; "`send` and `duck` are reserved for the strip params"; "index 4 is out of bounds (the chain has 2)".
- Text (`doc/items.rs`, used by the actions): a new track table is rendered right after the last `[tracks.*]` table with one blank line before it; `remove_track` removes the table and the comment lines above it, and the lane tables it removed; `rename_track` renames the key in place (table, comments and position kept); a new fx entry is an inline table inserted into the `fx` array at its index, the others' text unchanged (a single-line array stays single-line); `move_effect` moves the entry's text; `rename_device` writes `id = "<to>"` (for an instrument written as a name, `instrument = { type = "acid303", id = "bass303" }`) and renames its param keys in place.
- `targets()`: `tracks.<t>` for the track actions; `tracks.<t>.fx` for the effect actions (`rename_device` on the instrument: `tracks.<t>.instrument`).

- [ ] **Step 1: Write the failing tests** (`tune-song/tests/edit.rs` for the model, `tune-studio/tests/actions.rs` for the text; the commented fixture; a helper `comments_kept(before, after, except: &[&str])` asserts every comment line of `before` not in `except` is in `after`, in order):
  - `add_track_appends_a_default_track`: `add_track { track: "bass", instrument: "acid303" }`: the text has `\n[tracks.bass]\ninstrument = "acid303"\n` after hat's `play` list and before `\n[clips.k4]`; track order `[kick, acid, hat, bass]`; every comment kept; `CompiledSong::new(song, 48_000)` builds.
  - `track_id_rules`: each track message above from its action; the file is unchanged after each.
  - `remove_track_cleans_up`: `remove_track { track: "acid" }`: the `[tracks.acid]` table, `# the squelch`, the automation table and `# the filter opens over the section` are gone; the kick and hat tables are byte-identical; the result is `{"removed": {"lanes": [{"param": "acid.acid303.cutoff", "at": 3, "bars": 2, "points": [[0, 0.1], [2, 0.8]]}], "placements": [], "duck_source": null}}`.
  - `removing_the_duck_source_clears_it`: with `duck_source = "kick"` in `[mix]`, `remove_track kick` removes the key and reports `"duck_source": "kick"`; with `"duck.depth" = 6` on acid as well, it is refused with the validator's "`duck.depth` needs a kick to follow…" message.
  - `rename_track_rewrites_references`: `rename_track acid → bass`: `[tracks.bass]` stands where `[tracks.acid]` stood under `# the squelch`; the lane reads `param = "bass.acid303.cutoff"`; order `[kick, bass, hat]`.
  - `effects_insert_at_their_index`: `add_effect { track: "acid", module: "eq", index: Some(0) }` gives `fx = [{ type = "eq" }, { type = "distortion", mode = "soft", params = { "dist.drive" = 12 } }]` and result `{"id": "eq"}`; a second `add_effect eq` (no index) gets id `eq2` at the end; `move_effect { device: "dist", index: 2 }` gives the order `eq, eq2, dist` with `dist.drive` still 12.
  - `remove_effect_removes_its_lanes`: on a copy with a lane on `acid.dist.drive`, `remove_effect dist` removes the entry and that lane and lists it.
  - `rename_device_rewrites_params_and_lanes`: `rename_device acid acid303 → bass303`: `instrument = { type = "acid303", id = "bass303" }`, param keys `bass303.cutoff`…, the lane `acid.bass303.cutoff`; then `set_value tracks.acid.bass303.cutoff=600` succeeds. `rename_device acid dist → drive2`: the entry gets `id = "drive2"` and `"drive2.drive" = 12`.
  - `device_id_rules`: the device messages above.
  - `a_batch_builds_on_itself`: `apply_to_file` with `[add_track bass acid303, add_effect bass eq, set_value tracks.bass.eq.low_cut 120]` writes once (one `file_hash` on all entries, actions `add_track`, `add_effect`, `set_value`) and the song has `bass` with `eq.low_cut == 120`; the same batch with a fourth `set_value tracks.bass.eq.nope 1` writes nothing.
- [ ] **Step 2: Run** `cargo test -p tune-song --test edit` — Expected: FAIL.
- [ ] **Step 3: Implement** the model functions, the writers, the seven actions (catalog entries with help; `rename_track`'s help says the track's noise changes), regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-studio && pnpm -r test` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): add, remove and rename tracks and devices as actions`.

### Task 6: Swap a track's instrument

**Files:**
- Modify: `crates/tune-song/src/edit.rs`, `crates/tune-song/tests/edit.rs`, `crates/tune-studio/src/{doc/items.rs,action/*}`, `crates/tune-studio/tests/actions.rs`, `crates/tune-cli/tests/do_cli.rs`, goldens, `CLAUDE.md` (the action and its clean-up)

**Interfaces:**
- Consumes: `Edited`, `Removed`, the writers (Task 5).
- Produces: `pub fn set_instrument(song: &Song, registry: &Registry, track: &str, module: &str) -> Result<Edited, EditOpError>` and `Action::SetInstrument { track, module }` (result `{"removed": Removed}`). Rules (spec §7.3): the same module changes nothing; the new device takes the new module's prefix as id ("track `acid` already has a device `kick`; rename it first" when an fx device has it) with default settings and no params; the old instrument's params leave the track's `params` table (strip params stay; an empty table is removed); lanes on the old instrument's params are removed; placements whose clip the new module cannot play are removed (the clips stay in the pool); an explicit `mix.duck_source` naming the track goes when the new module plays no drum clips. One action, one undo step.

- [ ] **Step 1: Write the failing tests.**
  - `actions.rs::swap_cleans_up_and_lists_it`: `set_instrument { track: "acid", module: "kick" }` on the commented fixture: `instrument = "kick"`, no `params` line and no `play` line in `[tracks.acid]`, the `fx` line unchanged, the automation table gone, `[clips.a1]` still there; the result is `{"removed": {"lanes": [{"param": "acid.acid303.cutoff", "at": 3, "bars": 2, "points": [[0, 0.1], [2, 0.8]]}], "placements": [{"track": "acid", "at": 3, "clip": "a1", "bars": 2, "offset": 0}], "duck_source": null}}`.
  - `edit.rs::swap_keeps_strip_params_and_note_clips`: acid with `"send.delay" = -20` and a notes-clip placement: after the swap to `kick`, `send.delay` is still -20 and the notes placement is kept.
  - `edit.rs::swap_to_the_same_module_changes_nothing`: the song is unchanged.
  - `edit.rs::swap_clears_a_duck_source`: with `duck_source = "kick"`, swapping kick to `acid303` reports `duck_source: "kick"` and the mix has none.
  - `actions.rs::swap_is_one_undo_step`: through `StudioCore::apply_now`, then `undo()`: the file bytes equal the fixture's.
  - `do_cli.rs::swap_from_the_cli`: `tune do <song> set_instrument track=acid module=kick --json` gives `results[0].removed.placements[0].clip == "a1"`; text mode prints `removed placement a1 on acid at bar 3` and `removed lane acid.acid303.cutoff at bar 3`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test actions` — Expected: FAIL.
- [ ] **Step 3: Implement** the function, the instrument writer and the action; regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-studio && cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): swap a track's instrument with its clean-up as one action`.

### Task 7: Clip actions

**Files:**
- Modify: `crates/tune-song/src/{edit.rs,write.rs,lib.rs}`, `crates/tune-song/tests/edit.rs`, `crates/tune-studio/src/{doc.rs,doc/items.rs,action/*}`, `crates/tune-studio/tests/actions.rs`, goldens, `CLAUDE.md`

**Interfaces:**
- Produces:
  ```rust
  pub fn add_clip(song: &Song, id: &str, clip: Clip) -> Result<Edited, EditOpError>;
  pub fn remove_clip(song: &Song, id: &str) -> Result<Edited, EditOpError>;      // removes its placements, listed
  pub fn rename_clip(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError>;  // rewrites placements
  pub fn duplicate_clip(song: &Song, id: &str, to: &str) -> Result<Edited, EditOpError>;
  pub fn clip_table_toml(id: &str, clip: &Clip) -> String;   // `[clips.<id>]` as `to_toml` writes it
  ```
- `add_clip` action: `kind` is `acid`, `drum` or `notes` ("unknown clip kind `chords` (expected acid, drum or notes)"); `steps` only for acid and drum (default 16 rests: `". . . . . . . . . . . . . . . ."` and `"................"`), `bars` and `notes` only for notes (default `bars = 1`, no notes), the wrong one refused with the validator's hints; `notes` takes the JSON array `clips.<c>.notes` takes and is written the same way; a bad note gives the validator's positioned message. The table goes right after the last `[clips.*]` table (else after the last track table). Messages: "clip `k4` already exists", "there is no clip `k9`". `duplicate_clip`'s copy goes right after the original, without its comments. `targets()`: `clips.<c>` (and `clips.<to>`).

- [ ] **Step 1: Write the failing tests.**
  - `actions.rs::add_clip_kinds`: `add_clip { clip: "k8", kind: "drum", steps: Some("x.x.x.x.x.x.x.x.") }` writes `\n[clips.k8]\nkind = "drum"\nsteps = "x.x.x.x.x.x.x.x."\n` right after `[clips.h]`'s table; `add_clip lead notes bars=2 notes='[{ start = 0, pitch = "C3", len = 2 }]'` (through `action_from_cli`) writes `kind = "notes"\nbars = 2\nnotes = [{ start = 0, pitch = "C3", len = 2 }]`; `add_clip a2 acid` writes 16 rests.
  - `actions.rs::add_clip_rules`: the messages above; `steps` on a notes clip and `bars` on a drum clip give the validator's hints; `steps = "x.q"` gives the validator's positioned notation error; the file is unchanged after each.
  - `actions.rs::remove_clip_removes_its_placements`: `remove_clip k4`: no `play` line left in `[tracks.kick]`, the result lists `{track: "kick", at: 1, clip: "k4", bars: 2, offset: 0}` and the one at 3.
  - `actions.rs::rename_clip_rewrites_placements`: `rename_clip a1 → bass1`: `[clips.bass1]` stands where `[clips.a1]` stood; acid's placement reads `clip = "bass1"`.
  - `actions.rs::duplicate_clip_copies_after_the_original`: `duplicate_clip h → h2`: `[clips.h2]` right after `[clips.h]` with the same steps.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test actions` — Expected: FAIL.
- [ ] **Step 3: Implement** the functions, writers and actions; regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-studio` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): add, remove, rename and duplicate clips as actions`.

### Task 8: Placement and section actions

**Files:**
- Modify: `crates/tune-song/src/{edit.rs,arrange.rs,lib.rs}`, `crates/tune-song/tests/{edit.rs,arrange.rs}`, `crates/tune-studio/src/{doc.rs,doc/arrange.rs,action/*}` (`Tables::Insert(index, name, bars)`), `crates/tune-studio/tests/actions.rs`, goldens, `CLAUDE.md`

**Interfaces:**
- Produces:
  ```rust
  pub fn add_placement(song: &Song, track: &str, placement: Placement) -> Result<Edited, EditOpError>;   // kept in time order
  pub fn move_placement(song: &Song, track: &str, start_bar: u32, to: u32) -> Result<Edited, EditOpError>; // 0-based
  pub fn resize_placement(song: &Song, track: &str, start_bar: u32, bars: u32) -> Result<Edited, EditOpError>;
  pub fn remove_placement(song: &Song, track: &str, start_bar: u32) -> Result<Edited, EditOpError>;
  /// Inserts an empty section before section `index` (`index == len` appends). A placement or
  /// stretch crossing the insert point is split there first (clip phase and values kept);
  /// everything from it on moves `bars` later.
  pub fn insert_section(song: &Song, index: usize, name: &str, bars: u32) -> Result<Arranged, ArrangeError>;
  ```
  The actions convert 1-based `at`/`to` to 0-based (`at = 0`: "bars are 1-based: `at` must be at least 1"); "track `acid` has no placement at bar 2" names a missing one 1-based. Overlaps, incompatible clips and placements past the song's end are the validator's messages. `add_section` (index default: append) goes through `SongDoc::arrange` with `Tables::Insert`; `rename_section` is `SongDoc::set(SectionName(index))`. `targets()`: `tracks.<t>.play` for placements; `sections` for `add_section`; `sections[<i>].name` for `rename_section`.

- [ ] **Step 1: Write the failing tests.**
  - `actions.rs::placements_by_start_bar`: `add_placement { track: "acid", clip: "a1", at: 1, bars: 2 }` gives acid's `play` as `placements_toml` lays out two placements (`at = 1` then `at = 3`); `move_placement acid at=3 to=1` on the fixture gives `[{ at = 1, clip = "a1", bars = 2 }]`; `resize_placement acid at=3 bars=1`; `remove_placement kick at=3` leaves `play = [{ at = 1, clip = "k4", bars = 2 }]`.
  - `actions.rs::placement_rules`: `add_placement acid a1 at=4 bars=1` gives the validator's "placement at bar 4 on track `acid` overlaps the one at bar 3"; `add_placement acid k4 at=1 bars=1` the incompatible-clip message; `at=0` and "track `acid` has no placement at bar 2"; the file is unchanged after each.
  - `arrange.rs::insert_section_splits_a_crossing_placement`: sections `[2, 2]`, a placement over bars 1-4 (0-based 0..4), `insert_section(1, "break", 2)`: placements `{start 0, bars 2, offset 0}` and `{start 4, bars 2, offset 2}`; total bars 6.
  - `actions.rs::add_section_shifts_what_follows`: `add_section { name: "break", bars: 2, index: Some(1) }` on the fixture: sections `[a, break, b]`, acid's placement at 5, the lane at `at = 5`, kick `[1..2]` and `[5..6]`; `# acid enters` still sits right above `name = "b"`; `add_section outro 4` without an index appends and moves nothing.
  - `actions.rs::rename_section`: `rename_section 1 drop` changes only `name = "b"` to `name = "drop"`.
- [ ] **Step 2: Run** `cargo test -p tune-song --test arrange` — Expected: FAIL (no `insert_section`).
- [ ] **Step 3: Implement** the functions, `Tables::Insert` and the actions; regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-studio` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): edit placements and add or rename sections as actions`.

### Task 9: Lane actions

**Files:**
- Modify: `crates/tune-song/src/edit.rs`, `crates/tune-song/tests/edit.rs`, `crates/tune-studio/src/{doc/arrange.rs,action/*}` (`write_automation` creates the root `[[automation]]` list when absent), `crates/tune-studio/tests/actions.rs`, goldens, `CLAUDE.md`

**Interfaces:**
- Produces:
  ```rust
  pub fn add_lane(song: &Song, lane: Lane) -> Result<Edited, EditOpError>;
  pub fn remove_lane(song: &Song, track: &str, param: &str, start_bar: u32) -> Result<Edited, EditOpError>;
  pub fn set_lane_points(song: &Song, track: &str, param: &str, start_bar: u32, points: Vec<(f64, f64)>) -> Result<Edited, EditOpError>;
  ```
  The action's `param` is the file's spelling, split at its first `.` into track (or `mix`) and param id. A new stretch is `LaneOrigin::New` (a new table laid out by `rerank` after the section it starts in); `remove_lane` drops the stretch's table with its comments; `set_lane_points` replaces only the `points` value in place. "there is no lane `acid.acid303.cutoff` at bar 1"; overlaps, unknown params and points out of range are the validator's messages. `targets()`: `automation.<param>`.

- [ ] **Step 1: Write the failing tests.**
  - `actions.rs::add_lane_writes_a_table`: `add_lane { param: "mix.reverb.size", at: 1, bars: 4, points: [[0.0, 0.2], [4.0, 0.6]] }`: the song's automation gains that stretch, the existing lane table and its comment are byte-identical; on a song without lanes (the v2 `mini` copy with its lane removed) `add_lane acid.acid303.cutoff` creates the first `[[automation]]` table.
  - `actions.rs::lane_rules`: `add_lane acid.acid303.cutoff at=3 bars=2 …` gives the validator's overlap message; `acid.acid303.nope` the unknown-param message; `remove_lane acid.acid303.cutoff at=1` the missing-lane message.
  - `actions.rs::remove_and_set_points`: `remove_lane acid.acid303.cutoff at=3` removes the table and `# the filter opens over the section`; `set_lane_points acid.acid303.cutoff at=3 points='[[0, 0.3], [2, 0.9]]'` changes only the `points` line.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test actions` — Expected: FAIL.
- [ ] **Step 3: Implement** the functions, the writer change and the actions; regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-song && cargo test -p tune-studio` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): add, remove and reshape automation lanes as actions`.

### Task 10: Undo targets and one action record per write

**Files:**
- Create: `crates/tune-studio/src/target.rs`, `crates/tune-studio/tests/targets.rs`
- Modify: `crates/tune-studio/src/{log.rs,core.rs,set.rs,doc.rs,doc/items.rs,lib.rs}`, `crates/tune-studio/tests/{diff_log.rs,core_edits.rs}`

**Interfaces:**
- Consumes: the writers of Tasks 5 to 9 (`put` reuses them without their clean-up).
- Produces: `Target`, `TargetChange`, `target_changes`, `target_value`, `SongDoc::put` as decided, and
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct ActionRecord { pub ts: Timestamp, pub source: Source, pub actions: Vec<String>, pub run: Option<String>,
      pub run_title: Option<String>, pub targets: Vec<TargetChange>, pub file_hash: Option<String> }   // serialized with "record": "action"
  impl EditLog {
      /// The entries, then the record, in one `O_APPEND` write.
      pub fn append_write(&self, entries: &[EditEntry], record: Option<&ActionRecord>) -> io::Result<()>;
      /// Every action record, in file order; lines that are not records are skipped.
      pub fn read_records(&self) -> io::Result<Vec<ActionRecord>>;
  }
  ```
  Every logged write appends its record with `targets = target_changes(song before, song after)`: a producer burst (`actions` = the burst's action names, first occurrence order), an agent `apply_now`, `apply_to_file` (and so `tune set`), an external change the studio logs, the open-time migration and `migrate_file` (`actions: ["migrate"]`), and undo and redo (`["undo"]`, `["redo"]`, with the asker's source).

- [ ] **Step 1: Write the failing tests** in `tests/targets.rs` (the commented fixture):
  - `value_changes_are_value_targets`: cutoff 500 → 820 gives exactly `[TargetChange { target: "tracks.acid.acid303.cutoff", old: 500, new: 820 }]`.
  - `a_new_track_is_one_target`: after `add_track bass acid303`: one change, `tracks.bass`, old `null`, new with `index == 3`, `instrument.type == "acid303"`, `instrument.params.cutoff` equal to `ACID303`'s default, `gain_db == 0`, `play == []`.
  - `an_added_effect_is_an_fx_target`: after `add_effect acid eq`: one change, `tracks.acid.fx`, old `[dist]`, new `[dist, eq]` (full devices), and no leaf change for the eq's params.
  - `a_section_op_touches_sections_plays_and_lanes`: after `duplicate_section 1`: changes for `sections`, `tracks.kick.play`, `tracks.acid.play`, `tracks.hat.play` and `automation.acid.acid303.cutoff`, in that order.
  - `targets_round_trip_as_strings`: `Display`/`FromStr` for one target of each kind, including `automation.mix.reverb.size` (track `mix`, param `reverb.size`).
  - `put_restores_each_kind`: for each structural action of Tasks 5 to 9 applied to the fixture (one case each), putting every change's `old` back, newest first, retrying a refused put once after the others, gives a song with `target_changes(fixture, restored)` empty.
  - `every_write_appends_a_record`: through `StudioCore`: a knob burst, an external edit, an undo, an `apply_now` with run `r1`, and `apply_to_file` each add exactly one record (source and `actions` as listed above; `run: "r1"` and `run_title` on the agent's); `read_since` returns the same entries as before records existed, and a log written by Task 3's code reads unchanged.
  - `records_have_a_pinned_shape`: one record serializes exactly as the example in "The edit log".
- [ ] **Step 2: Run** `cargo test -p tune-studio --test targets` — Expected: FAIL.
- [ ] **Step 3: Implement** the target model, `put`, the record and its writes at every call site of `append_changes` (which `append_write` replaces).
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): record each write's undo targets in the edit log`.

### Task 11: Undo this run (RISKY: deletes nobody's work)

**Risk.** A run undo must never delete or overwrite a later edit by someone else. When in doubt it keeps the target and lists it.

**Files:**
- Create: `crates/tune-studio/src/run_undo.rs`, `crates/tune-studio/tests/run_undo.rs`
- Modify: `crates/tune-studio/src/{action/*,core.rs,set.rs,studio.rs,error.rs,lib.rs}` (`UndoRun` uses `ApplyCtx.log`; from the UI it writes at once like `apply_now`), `crates/tune-cli/src/cmd/do_action.rs` (text), `crates/tune-cli/tests/do_cli.rs`, goldens
- Modify: `CLAUDE.md` ("Working with the studio open": `tune do <song> undo_run <run id>`, what it keeps and the JSON result)

**Interfaces:**
- Consumes: `ActionRecord`, `EditLog::read_records`, `target_value`, `SongDoc::put` (Task 10).
- Produces: `RunUndo`, `TargetItem`, `KeptTarget`, `KeptReason`, `undo_run`, `Action::UndoRun { run }` (result: the `RunUndo`), `StudioError::NoSuchRun`. An undo that reverts nothing writes nothing and pushes no undo step. Text output: `undid run r1: 4 reverted, 1 kept` then one `kept <target>: changed later by human` (`… by another run`, `… outside the studio`, `still used: <message>`) line each.
- The algorithm (determined here):
  ```text
  mine = records with run == R (file order); none → NoSuchRun(R)
  plan: for each record i in mine, each change c (record order):
    if c.target is part of an item the run created → fold c.new into that item's plan.after; item.last = i; continue
       (part of: anything under tracks.<t> when Track(t) was created; leaves tracks.<t>.<id>.* of a device in
        Fx(t).after but not Fx(t).before, or of the new instrument when Instrument(t)'s (type, id) changed; clips.<c>.* when Clip(c) was created)
    plan[c.target] ??= { before: c.old, first: i }; .after = c.new; .last = i
  overlap(T, U): T == U; or one's string is a prefix of the other's at a "." boundary; or Track(t) with automation.<t>.*;
    or Instrument(t)/Fx(t) with a leaf tracks.<t>.<id>.* where <id> is a device id in that change's old or new value
  for each planned T other than Play and Lanes:
    foreign = records j after plan[T].first with run != R that touch a target overlapping T
    if some foreign j > plan[T].last → kept(T, changed_later, by: record j's source)
    elif foreign → before := old of T in the first record of mine after the last foreign j (folded as above)
    current = target_value(song, T)
    current ≈ before → already; current ≉ after → kept(T, changed_later, by: None); else revert(T, before)
    (≈: Track ignores "index"; Fx compares the (id, type) order plus the full devices in after but not before; others ==)
  Play and Lanes, element by element (an element is a placement or a stretch, equal when every field is):
    added = after − before, removed = before − after
    each a in added: in current → revert (remove a, item a) else kept(item a, changed_later, by: the last foreign record touching T, else None)
    each r in removed: after the removals, overlapping nothing in current → revert (add r back, item r) else kept(item r, changed_later)
  apply: reverts in order of plan[T].last, newest first (within one record, reverse order); each is put on a copy and kept
    when it commits; a refused put waits; repeat passes while one succeeds; the rest → kept(T, still_used, message: the error's first line)
  ```

- [ ] **Step 1: Write the failing tests** in `tests/run_undo.rs` (`StudioCore` on the commented fixture; agent edits by `apply_now` with `Origin::claude(Some(r1))`, human edits by `apply_edit` + `flush`, the run undo by `apply_now([UndoRun { run: "r1" }], Origin::human())`):
  - `undo_run_reverts_every_action_of_the_run`: r1 sets cutoff 820 and bpm 130, adds an `eq` to acid, adds a track `keys` (acid303), adds clip `lead` (notes, 1 bar, one note) and places it on `keys` at bar 1 for 2 bars; the undo leaves `target_changes(fixture song, now)` empty and `kept` empty, and is one undo step (`undo()` brings the run's state back).
  - `a_later_human_edit_is_kept`: r1 sets cutoff 820 and acid `gain_db` -6; the producer sets cutoff 700; the undo: cutoff 700 (kept, `changed_later`, by `human`), gain back to 0.
  - `the_run_after_a_human_edit_reverts_to_the_human_value`: r1 cutoff 500 → 800, the producer 800 → 600, r1 600 → 900: the undo gives 600.
  - `an_item_the_run_created_and_the_human_changed_is_kept`: r1 `add_track bass acid303`; the producer sets `tracks.bass.acid303.cutoff` 600: `tracks.bass` kept (by human), the track stays with 600.
  - `another_runs_edit_counts_as_someone_else`: r1 then r2 set cutoff: undoing r1 keeps it, `by: "claude"`.
  - `a_clip_still_used_is_kept`: r1 adds clip `lead`; the producer places it on acid: `clips.lead` kept, reason `still_used`, message from the validator; the placement stays.
  - `placements_revert_one_by_one`: r1 places `a1` on acid at bar 1 (1 bar) and at bar 2 (1 bar); the producer removes the one at 1: the undo removes the one at 2 (reverted, item at 2), keeps the one at 1 as `changed_later`, and acid's placement at 3 is untouched.
  - `section_ops_revert_with_their_placements`: r1 `add_section break 2 index=1`: the undo gives sections `[a, b]` and every placement and the lane back where they were; `# acid enters` still above `name = "b"`.
  - `remove_track_reverts_with_its_lanes`: r1 `remove_track acid`: the undo puts acid back between kick and hat with its lane; `target_changes(fixture song, now)` empty.
  - `studio_closed_hand_edit_is_kept`: through `apply_to_file`: r1 sets cutoff 820; the file is edited by hand to 900 (no log); `tune do`'s path (`apply_to_file([UndoRun])`) keeps it with `by: null` and reverts nothing else.
  - `undo_run_twice_changes_nothing`: the second undo has every target in `already`, writes nothing, pushes no undo step.
  - `after_cmd_z_the_rest_reverts`: r1 makes two edits; the producer's `undo()` reverts the second; the run undo lists it as `already` and reverts the first.
  - `no_such_run`: `UndoRun { run: "zz" }` gives "no edits of run `zz` in the edit log".
  - `do_cli.rs::undo_run_from_the_cli`: two `tune do … --run r1` edits, then `tune do <song> undo_run r1 --json` (positional, studio closed): `results[0].reverted` has both targets and the file is back to the fixture's values.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test run_undo` — Expected: FAIL.
- [ ] **Step 3: Implement** `undo_run`, the action (reading `ctx.log`), the immediate write from the UI, and the CLI text; regenerate the goldens.
- [ ] **Step 4: Run** `cargo test -p tune-studio && cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit `feat(studio): undo one agent run, keeping later edits`.

### Task 12: "Undo this run" and ⌘Z in the current studio

**Files:**
- Modify: `apps/desktop/src/store/studioStore.ts` (the notice from an `applied` event from Claude, with its run), `apps/desktop/src/screens/{ChangeNotice.tsx,StudioScreen.tsx}`, `apps/desktop/src/lib/{useStudioKeys.ts,describeChange.ts}`
- Test: `apps/desktop/src/store/studioStore.test.ts`, `apps/desktop/src/screens/{ChangeNotice.test.tsx,StudioScreen.test.tsx}`, `apps/desktop/src/lib/describeChange.test.ts`

**Interfaces:**
- Consumes: `Event::Applied.run`, `RunUndo`, `KeptTarget` (generated), the `undo_run` action.
- Produces (UI behaviour, copy verbatim):
  - The notice type is `Notice = ExternalChange & { run: RunTag | null }`. An `applied` event with `source: "claude"` sets `notice = { source: "claude", changes, run, replaced: false, merged: [], offered: [], errors: [] }` and `noticeUndoable: true` (as an `external_change` from Claude does today); an `external_change` gives `run: null`.
  - The toast header reads `Claude` and, when the run has a title, ` · <title>` in `text-ink2`. Buttons: "Undo" (aria "Undo Claude's change") while `noticeUndoable`; "Undo this run" (aria "Undo this run", agent style) when `run` is set; Dismiss.
  - "Undo this run" sends `{ cmd: "undo_run", args: { run: run.id } }`; on its `RunUndo` the toast text becomes `Undid this run: ${plural(reverted.length, "change")}.`, followed, when `kept` is not empty, by `Kept ${plural(kept.length, "change")} made since: ${kept.map(describeTarget).join(", ")}.`; with nothing reverted and nothing kept, `Nothing to undo: this run is already undone.`; then only Dismiss.
  - `describeTarget(k: KeptTarget, song)`: a leaf path as `describeChange` labels it; `tracks.<t>` "track <t>"; `tracks.<t>.instrument` "<t>'s instrument"; `tracks.<t>.fx` "<t>'s effects"; `tracks.<t>.play` "a clip on <t>"; `clips.<c>` "clip <c>"; `automation.<t>.<p>` "<p> automation on <t>" (`mix`: "mix <p> automation"); `sections` "the sections"; followed by ` (by you)` for `by: "human"`, ` (another run)` for `"claude"`, nothing otherwise.
  - ⌘Z / ⇧⌘Z (and Ctrl on other systems) run anywhere, popovers and menus included, except in a text field (`input`, `textarea`, `select`, contenteditable). Space, L, M and Esc keep today's rules.

- [ ] **Step 1: Write the failing tests.**
  - `studioStore.test.ts`: "an applied change from Claude raises the notice with its run" (event `{event: "applied", source: "claude", changes: [cutoff], run: {id: "r1", title: "open the filter"}}` → `notice.run.id == "r1"`, `noticeUndoable`); "a human edit after it ends its Undo".
  - `ChangeNotice.test.tsx`: "shows the run title", "Undo this run sends undo_run" (clicking calls `onUndoRun("r1")`), "reports what the run undo kept" (a `RunUndo` with 3 reverted and one kept `tracks.acid.acid303.cutoff` by human → `Undid this run: 3 changes.` and `Kept 1 change made since: <the cutoff label> (by you).`), "says when the run was already undone", "no Undo this run without a run".
  - `StudioScreen.test.tsx`: "Cmd+Z undoes inside an open popover" (the pattern picker open, ⌘Z sends `undo`), "Cmd+Z in a text field does nothing", "Shift+Cmd+Z redoes".
  - `describeChange.test.ts`: one case per `describeTarget` form.
- [ ] **Step 2: Run** `pnpm --filter @tune/desktop test` — Expected: FAIL.
- [ ] **Step 3: Implement** the store, toast, screen wiring, keys and `describeTarget`.
- [ ] **Step 4: Run** `pnpm -r typecheck && pnpm -r test && task ui:build` — Expected: PASS.
- [ ] **Step 5: Visual check (not committed):** copy `songs/first-acid/song.toml` into `scratch/run-check/songs/first-acid/`, start `cargo run -p tune-cli -- serve scratch/run-check/songs/first-acid/song.toml --device null --port 4580` in the background with `TUNE_STUDIO_DIR=scratch/run-check/studios`, open it with the `agent-browser` skill, run two `tune do … --run r1 --run-title "test run"` edits with the same `TUNE_STUDIO_DIR`, and check: the toast says `Claude · test run` with both buttons; turn one of the changed knobs; "Undo this run" reverts the other and says it kept the knob; ⌘Z with the pattern picker open undoes; ⌘Z in a text field does not. Stop the server.
- [ ] **Step 6: Run** `task verify`, then commit `feat(ui): undo an agent run from the toast and use Cmd+Z anywhere`.

### Task 13: The exits, end to end

**Files:**
- Create: `crates/tune-cli/tests/exit_cli.rs`

**Interfaces:**
- Consumes: everything above. The tests run the `tune` binary with `TUNE_STUDIO_DIR` set to a temp dir; the studio runs in the test process (`Studio::spawn` with a capture device that parks the live processor, as `crates/tune-studio/tests/studio.rs` does, and `ControlServer::start` on that dir).
- The starting file (the minimal song of the rulings):
  ```toml
  [song]
  version = 2
  name = "built"
  bpm = 120

  [[sections]]
  name = "main"
  bars = 4
  ```

- [ ] **Step 1: Write the tests.**
  - `a_song_built_only_with_tune_do` (spec §11 exit 1, studio closed): `add_track acid acid303`; `add_effect acid eq`; `add_effect acid eq` (id `eq2`); `add_effect track=acid module=distortion index=1`; `set_value path=tracks.acid.eq2.low_cut value=200`; `add_clip bass acid steps="C2 C2! C3~ C2 . D#2 C2~ G2!"`; `add_clip lead notes bars=1 notes='[{ start = 0, pitch = "C3", len = 4 }, { start = 8, pitch = "G3", len = 4, vel = 120 }]'`; `add_track kick kick`; `add_clip k4 drum steps=x...x...x...X...`; `add_placement acid bass at=1 bars=2`; `add_placement acid lead at=3 bars=2`; `add_placement kick k4 at=1 bars=4`. Then `tune validate --json` is ok; `tune info --json` lists tracks `acid` and `kick` and clips `bass`, `lead`, `k4`; the parsed song's acid fx ids are `eq`, `dist`, `eq2` in that order; `tune render <song> --stems --json --out <tmp>/x.wav` gives acid and kick stems with `peak_dbfs` above -60.
  - `the_running_studio_hot_reloads_without_a_click` (exit 1, studio open): a copy of that song with acid alone playing one held note (`lead` replaced by a notes clip with one note `{ start = 0, pitch = "C2", len = 64 }`, bars 4, placed over bars 1-4; kick placements removed); `tune do <song> play` starts playback; pull 1 s of frames; `tune do <song> --batch` with `[{"cmd": "add_effect", "args": {"track": "acid", "module": "eq", "index": 0}}, {"cmd": "set_value", "args": {"path": "tracks.acid.eq.high_cut", "value": 2000}}]` on stdin returns `via: "studio"`; keep pulling until the subscriber has seen the `applied` event, then 200 ms more. No `error` event arrives; the largest left-channel sample-to-sample jump from the call to the end is at most 1.5 times the largest in the 100 ms before the call (spec §7.1's measure over a window that contains the swap); the last 100 ms are not silent (RMS above 1e-3).
  - `an_agent_run_is_undone_keeping_a_later_human_edit` (exit 4): the studio open on the fixture copy; three `tune do … --run r1 --run-title "make it hit harder"` edits (cutoff 820, acid `gain_db` -2, bpm 130); a human knob release on cutoff (700) through `handle.call`; `tune do <song> undo_run r1 --json` gives `via: "studio"`, `kept` naming `tracks.acid.acid303.cutoff` by `human`, and the file has cutoff 700, gain 0, bpm 120; then one `handle.call(Undo)` brings gain -2 and bpm 130 back with cutoff still 700 (the undo stepped back exactly that one action).
- [ ] **Step 2: Run** `cargo test -p tune-cli --test exit_cli` — Expected: PASS. A failure here is a bug in an earlier task: find it there (e.g. for the jump measure, check that the swap crossfaded, `Engine::swap_fading`, before suspecting the signal), never loosen the measure.
- [ ] **Step 3: Run** `task verify`, then commit `test(cli): build, hot-reload and run-undo a song end to end`.

---

## Self-review notes

- Spec coverage: §8.1 one Rust definition per action with name, typed arguments, help and kind (Task 1, grown by Tasks 5 to 9 and 11); song-edit actions: set a value (`set_value`, Task 1), set a step (Task 1), tracks (Task 5), set or swap an instrument (Task 6), effects and device settings (Tasks 1, 5), clips and their notes and steps (Task 7, `set_value` paths), placements (Task 8), sections (Tasks 1, 8), lanes and breakpoints (Task 9), mix fields (`set_value`); scene and slot actions are Plan 10's. Live actions: transport (Task 1); session launches are Plan 10's. Stable target ids and `targets()` (Task 1). `tune actions --json` (Task 1), `tune do` with a stdin batch (Task 3), `tune set` as shorthand (Task 3). Routing through a running studio, live actions failing without one, localhost, per-user secret, other origins refused (Task 4). The parity test with exactly the five exemptions (Task 1). Generated wire types (Task 2). §8.2 one action per undo step, gestures coalesced (Tasks 1, 4), runs from flags or environment stored in the log (Task 3), undo this run with the keep rules, the studio-closed comparison and the before/after/created records (Tasks 10, 11), ⌘Z/⇧⌘Z and the toast button (Task 12), history depth 500 and run undo across restarts (rulings, Task 11 `studio_closed_hand_edit_is_kept`). §8.3 old entries readable, entries gain action and run, records hold what inversion needs (Tasks 3, 10). §7.3 the instrument swap with its clean-up and result (Task 6). §11 exits 1 and 4 (Task 13); exit 2 is Plan 7's and stays pinned; exit 3 is Plan 10's; exit 5 (parity test, `task verify`) here and the test-only module test from Plan 7.
- Not here on purpose: scenes, slots, session launches and their parity samples (Plan 10).
