# Plan 3 — The studio: write-back, edit log, undo and a first desktop UI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A preliminary but usable studio. The producer opens a song, assembles and rearranges sections, edits 303 steps and drum hits, and twists the 303's knobs while hearing the result live. Every edit is written back to `song.toml` with its comments kept, logged to `edits.jsonl` and undoable. Claude reads the producer's edits with `tune changes`, makes tagged edits with `tune set`, and sees the UI through `tune serve` and `agent-browser`.

**Architecture:** A new headless crate `tune-studio` owns the open song: a `toml_edit` document for in-place edits, the parsed `Song`, the live engine controller, the file watcher, the edit log and a snapshot undo stack. It runs as one actor thread behind a cloneable `StudioHandle` that takes typed `Command`s and broadcasts `Event`s. Two thin transports carry the same JSON protocol: the Tauri 2 app `apps/desktop` (IPC) and `tune serve` (axum HTTP + WebSocket on loopback). The React frontend talks to a `StudioClient` interface with one implementation per transport. Audio output moves from `tune-cli` into a small `tune-audio` crate that implements `tune_engine::AudioOut`.

**Tech Stack:** Rust 2024 (toolchain 1.97). New Rust dependencies: `toml_edit` 0.25 (the release that pairs with `toml` 1.1), `jiff` 0.2 (timestamps), `serde_json` 1, `axum` 0.8 (`ws`), `tokio` 1.52 (`rt-multi-thread`, `net`, `macros`, `sync`), `tower-http` 0.6 (`fs`), `tauri` 2.11 and `tauri-build` 2.6. Dev-only: `tokio-tungstenite` (current), `ureq` 3. Frontend, mirroring sagepad's versions: pnpm 9.15.0, React 18.3.1, TypeScript 5.7.2, Vite 6.0.7, Vitest 4.1.10, Tailwind 3.4.17, `@tauri-apps/api` 2.1.1, `@tauri-apps/cli` 2.1.0, Testing Library (react 16.3.2, user-event 14.6.1, jest-dom 6.9.1), jsdom 29.1.1, eslint 9.17.0, prettier 3.4.2, `class-variance-authority` 0.7.1, `clsx` 2.1.1, `tailwind-merge` 3.6.0, `lucide-react` 1.24.0, `@radix-ui/react-popover` and `@radix-ui/react-slot` (current, pinned exactly), `@fontsource/barlow` and `@fontsource/barlow-condensed` (current, pinned exactly).

**Spec:** `docs/superpowers/specs/2026-10-20-tune-ai-design.md`. This plan covers §6.3 write-back and the conflict rule, §6.4 edit log, §8a (visible and undoable edits; writers of the song file), and a preliminary cut of §6.8 (transport, 303 panel, 303 sequencer, drum grid, arrangement, track strip). It does not cover knob recording into lanes, lane drawing, the sample browser or sends. Plan 2 (`docs/superpowers/plans/2026-10-21-plan-2-song-engine.md`) must be complete; its Interfaces blocks are this plan's inputs.

## Global Constraints

- macOS on Apple Silicon only. Repo stays MIT. No JC303 (GPL) code may be copied.
- Branch `feat/studio-ui`, stacked on `feat/song-engine`. Atomic conventional commits, one concern each. Run `task verify` before every commit.
- Never commit audio, renders, screenshots or `edits.jsonl`. Screenshots go to `scratch/` (gitignored). No absolute home paths, emails, real names or machine-specific data in committed files. The privacy check stays green and is never weakened to pass.
- `tune-dsp`, `tune-song`, `tune-engine` and `tune-studio` stay headless: no Tauri, UI, `cpal`, `tokio` or `axum` dependency. `cpal` lives only in `tune-audio`. `tokio`/`axum` only in `tune-cli` and `apps/desktop/src-tauri`. Every Rust crate declares `#![forbid(unsafe_code)]` except `tune-dsp` (unchanged).
- Determinism: offline render stays bitwise identical to Plan 2. Live-only state (knob overrides) never reaches `render`.
- The realtime audio callback must not allocate, deallocate, lock, print or block.
- Bars: engine API and wire protocol use 0-based bars and half-open ranges. Anything shown to a person (UI, CLI text) is 1-based. Position reads `bar:beat`, both 1-based, bar zero-padded to 3 digits (`033:2`).
- Song file writes are atomic: write `<dir>/.song.toml.tune-tmp`, then rename over `song.toml`. A write never produces a file that `parse_song` rejects.
- Edit log: `<song dir>/edits.jsonl`, one JSON object per line: `{"ts","source","path","old","new"}` plus optional `"file_hash"`. `source` is `"human"`, `"claude"` or `"external"`. Gitignored.
- Fixed values: write debounce 150 ms; playhead events 30 Hz (every 33 ms); change highlight 4 s; undo depth 500; own-write hash ring 8; whole-file replace when more than 50 % of the old file's non-blank lines are gone; knob drag 200 px for the full range, Shift = 10× finer; committed knob values rounded to 4 significant digits.
- `tune serve` binds `127.0.0.1` only and refuses WebSocket upgrades whose `Origin` is not its own `http://127.0.0.1:<port>` / `http://localhost:<port>` or `http://localhost:1420` (vite dev).
- CLI conventions from Plan 2 hold: every command takes `--json`, exit 0 on success and 2 on failure, JSON on stdout, text errors on stderr.

## Review Focus

1. **An external save lands while the producer holds a knob** (Claude runs `tune set` mid-drag). Expected: the drag stays audible through the reload, and the release commits onto the new file, so both edits survive. Test in Task 8.
2. **An editor writes the file in two chunks while local edits are still pending** (the first chunk is invalid TOML). Expected: the pending edits are held, not merged into the half file, then merged when the complete file arrives. Test in Task 7.
3. **Section edits under the playhead or the loop** (deleting the section that is playing, shrinking the song below the loop). Expected: no panic, playback continues or finishes cleanly, and the state's loop is clamped or cleared. Test in Task 8.
4. **Two clients at once** (the Tauri app and a browser tab, or two tabs). Expected: both start from the same state, and an edit made in one appears in the other. Test in Task 9.
5. **A held arrow key on a knob** (about 30 commits a second). Expected: one file write and one undo step per 150 ms burst, and one log entry per changed path per burst, giving the burst's first old value and last new value. Test in Task 6.

## Decisions made in this plan (flag if wrong)

- **Undo uses whole-text snapshots**, one entry per write burst and one per external change. Restoring bytes keeps comments exactly and works for any structural change, and a song file is small.
- **Edit paths** mirror the TOML layout (`tracks.acid.acid303.cutoff`, `sections[2].bars`). Sections are addressed by index, because names need not be unique.
- **Conflicts:** an external edit to the same path wins, because it was written last. The local value is offered for re-apply, never dropped.
- **Claude attribution:** `tune set` logs its entries with the resulting file's FNV-1a hash. The app tags an incoming change `claude` when the log holds that hash.
- **Knob overrides** live in the engine's track runtimes and travel across swaps. A committed knob value is rounded to 4 significant digits. Releasing a knob on a param automated in the current section writes nothing (§6.3).
- **`AudioOut` lives in `tune-engine`**, so `tune-studio` stays free of `cpal`.
- **One generic `studio` command per transport**, carrying the serde `Command`, plus one event stream. A golden JSON file pins the format for Rust and TS.
- **`tune serve` serves `apps/desktop/dist` from disk** instead of embedding it, so cargo builds do not depend on a JS build. The app takes its song from `--song`/`TUNE_SONG`, and otherwise shows a song list.
- **The help strip under the 303 panel** shows each knob's help (G8) in place of tooltips, so help never covers a knob being dragged.
- No lane editing, pattern-length editing or pattern creation in this plan. Claude does those through `tune set` or file edits.

---

## Visual direction (Decision)

"Gunmetal faceplate in a dark club." The 303 is the room's one lit instrument. The UI sits on a quiet graphite chassis. The 303 panel is a brushed gunmetal faceplate with real knob shading and silkscreen labels, which is where the plan spends its boldness. Everything else is flat. Colour carries meaning, not decoration: red means sounding now, amber means slide or loop, cyan means Claude changed this.

| Token | Hex | Use |
|---|---|---|
| `--chassis` | `#1D1F22` | app background |
| `--faceplate` | `#2C3036` → `#3A3F46` | 303 panel, brushed linear gradient plus a 3 % SVG grain |
| `--silkscreen` | `#ECE6D6` | labels and values (≥ 11:1 on the faceplate) |
| `--led` | `#FF4433` | playhead, lit step, accent, value arc |
| `--slide` | `#FFB01F` | slide flags, loop range |
| `--claude` | `#3FD0E6` | highlight for changes from Claude (external changes: the same colour, dashed outline) |

- Type: Barlow Condensed 600 for labels and section names, Barlow 500 with `tabular-nums` for values. Both are self-hosted through `@fontsource`. Labels are sentence case ("Cutoff", "Env mod"). There are no all-caps eyebrows and no monospace data labels.
- Layout (minimum window 1280×800; left-aligned; the arrangement scrolls horizontally):
  ```
  ┌ transport: ▶ ■ │ 145 BPM │ 033:2 │ loop │ first-acid │ ↶ ↷ │ change notice ──┐
  ├ arrangement: bar ruler · section blocks · one lane row per track · lane curves ┤
  ├ 303 faceplate: big knobs, dist row, help strip  ─────────────┬ track strips ───┤
  │ 303 step sequencer (selected acid pattern)                   │ gain pan M S    │
  ├ drum grid (drum patterns of the selected section) ───────────┴─────────────────┤
  ```
- Motion appears only where it carries information: the playhead, the step LED chase, and the 4 s highlight fade. All of it respects `prefers-reduced-motion`. Focus rings: 2 px `--silkscreen` outline at a 2 px offset. Knobs are SVG. Drum and step grids use real `<button>`s.

---

## File Structure

```
crates/tune-engine/src/{engine.rs,live.rs,compile.rs}   + overrides, AudioOut, param_target
crates/tune-song/src/watch.rs                          + RawReload, SongWatcher::start_raw
crates/tune-audio/src/{lib.rs,cpal_out.rs,null_out.rs}  moved from tune-cli/src/audio.rs
crates/tune-studio/src/
  lib.rs        re-exports
  path.rs       SongPath (edit addresses, Display/FromStr)
  doc.rs        SongDoc: toml_edit in-place edits, validated on every mutation
  diff.rs       Change, diff_songs, is_whole_replace
  log.rs        Source, EditEntry, EditLog, text_hash
  error.rs      StudioError, ErrorView
  set.rs        parse_assignment, set_paths, write_atomic (tune set's write path)
  command.rs    Command, DrumHitDto
  view.rs       StudioState, SongView and friends, ErrorView
  core.rs       StudioCore: edits, debounce, flush, undo/redo, external merge
  studio.rs     Studio actor, StudioConfig, StudioHandle, Event
crates/tune-studio/tests/{doc,diff_log,core_edits,core_external,studio,protocol}.rs
crates/tune-studio/tests/fixtures/{commented.toml,protocol.json}
crates/tune-cli/src/cmd/{set.rs,changes.rs,serve.rs}; tests/{edit_cli,serve_cli}.rs
package.json, pnpm-workspace.yaml, pnpm-lock.yaml, eslint.config.js, .prettierrc, .prettierignore
packages/ui/src/{index.ts,test-setup.ts,theme/tokens.css,tailwind.preset.ts,lib/{utils,param,notes}.ts}
packages/ui/src/components/{Knob,AcidStepSequencer,DrumGrid,Arrangement,TrackStrip,TransportBar}.tsx (+ .test.tsx)
apps/desktop/{package.json,index.html,vite.config.ts,vitest.config.ts,tailwind.config.ts,postcss.config.js,tsconfig.json}
apps/desktop/src/{main.tsx,App.tsx,client/*,store/*,screens/*}
apps/desktop/src-tauri/{Cargo.toml,build.rs,tauri.conf.json,capabilities/default.json,app-icon.svg,icons/*,src/{main,lib}.rs}
Cargo.toml (members + apps/desktop/src-tauri), Taskfile.yml, .gitignore, CLAUDE.md, README.md, THIRD_PARTY.md
```

---

### Task 1: Engine knob overrides and the `AudioOut` seam

**Files:**
- Modify: `crates/tune-engine/src/{engine.rs,live.rs,compile.rs,lib.rs}`
- Test: `crates/tune-engine/tests/{overrides.rs,live.rs,live_no_alloc.rs}`

**Interfaces:**
- Consumes: Plan 2 `Engine`, `TrackRuntime`, `CompiledSong`, `ParamTarget`, `live()`, `LiveController`, `LiveProcessor`, `SongUpdate`.
- Produces:
  ```rust
  // ParamTarget already derives Clone/Copy/Debug/PartialEq/Eq (Plan 2)
  impl CompiledSong { pub fn param_target(&self, track: usize, param_id: &str) -> Option<(ParamTarget, &'static ParamSpec)>; } // thin pub wrapper over compile.rs's private resolve
  impl Engine { pub fn set_override(&mut self, track: usize, target: ParamTarget, plain: Option<f64>);
                pub fn clear_overrides(&mut self); pub fn overrides_active(&self) -> u32; }
  impl LiveController {
      pub fn set_override(&mut self, track: usize, target: ParamTarget, plain: Option<f64>) -> Result<(), LiveError>;
      pub fn swap_clearing_overrides(&mut self, song: Arc<CompiledSong>) -> Result<(), LiveError>;
      pub fn overrides_active(&self) -> u32;   // published atomic, like position()
  }
  pub trait AudioOut: Send {
      fn sample_rate(&mut self, wanted: u32) -> Result<u32, String>;              // rate the device will run at
      fn start(&mut self, processor: LiveProcessor) -> Result<Box<dyn Send>, String>; // dropping the box stops output
  }
  ```
- Override storage lives in `TrackRuntime` (`NaN` = none), preallocated in `TrackRuntime::build` for every instrument and fx param, so overrides travel with a runtime that a swap keeps (same id and kind; fx chain kept). Per-sample order gains step 3b: right after lanes and before the `on_step` loop, in both the step-boundary and non-boundary branches of `frame`, apply each active override with `set_param_at` (so trigger-time reads such as clap/hat tone at the hit see the override). Skip the loop when `overrides_active() == 0`. `set_override` plain values are clamped by the spec.
- Every `SongUpdate` gets a controller-assigned `generation: u64` (incremented per `swap`/`swap_clearing_overrides` call); the processor publishes the generation it is playing. Every override command carries the generation the controller considered current (the last one sent). The processor drops an override whose generation differs from the playing generation, because its track index may belong to another song. (Do not use `swaps_applied`: a swap replaced by a newer pending one is never counted, so counts would desync under fast edits.) Test `override_after_replaced_swap_applies`: swap(B), swap(C) before processing (B replaced by C), process past the next step boundary so C lands, then `set_override` → applied (`overrides_active() == 1`); with a swaps-applied count this would be dropped forever. The UI resends on the next pointer move. `swap_clearing_overrides` sets a flag on the `SongUpdate`, and the swap clears all overrides when it lands.

- [ ] **Step 1: Write failing tests.** `overrides.rs` (Plan 2's Probe via `Engine::from_parts`, track 1 = `acid` in mini): `override_wins_over_static_and_lane` uses mini section `b` (cutoff lane) with `set_override(1, ParamTarget::Instrument(2), Some(2000.0))` at sample 100. Every recorded cutoff value from sample 100 on is 2000.0. After `set_override(1, Instrument(2), None)` at sample 200, sample 201 sees the lane value again. `clear_overrides_restores_static`: in section `a`, an override then `clear_overrides()` gives the next sample static 500.0, and `overrides_active() == 0`. `param_target_resolves`: `param_target(1, "acid303.cutoff") == Some((Instrument(2), _))`, `"dist.drive"` gives `Fx { slot: 0, index: 0 }`, and `"kick.tune"` on track 1 gives `None`. Plan 2's offline and determinism tests stay unchanged. `live.rs`: `live_override_equals_static_value`, where live(mini **with section b's cutoff lane removed**) with `set_override(acid, cutoff, Some(2394.0))` before the first `process` bitwise-equals `render(that song with "acid303.cutoff" = 2394)` over bars 0..4 (with the lane present, the override wins over the lane but a static 2394 would not, so the comparison would be wrong). `override_survives_swap`: continue, `swap(B)` where B is the same song with `hat` muted. Output after the boundary equals render(B with cutoff 2394) from the boundary. `stale_override_dropped`: `swap(B)` then `set_override(..)` before any `process` leaves `overrides_active() == 0` after the swap lands. `clearing_swap`: `overrides_active()` goes 1, then 0 after `swap_clearing_overrides` lands. Extend `live_no_alloc.rs` with an override set, a clear, and a clearing swap. Expected: 0 allocations.
- [ ] **Step 2: Run** `cargo test -p tune-engine` — Expected: FAIL (missing methods).
- [ ] **Step 3: Implement** the API above.
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS, Plan 2 tests unchanged.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(engine): add live knob overrides`; `feat(engine): add AudioOut output seam`.

### Task 2: `tune-audio` crate

**Files:**
- Create: `crates/tune-audio/{Cargo.toml,src/lib.rs,src/cpal_out.rs,src/null_out.rs}`, `crates/tune-audio/tests/null_out.rs`
- Delete: `crates/tune-cli/src/audio.rs`. Modify: `crates/tune-cli/{Cargo.toml,src/cmd/play.rs}` (drop `cpal`, add `tune-audio`), `CLAUDE.md` layout line.

**Interfaces:**
- Consumes: `AudioOut`, `LiveProcessor`, `live`.
- Produces: `pub enum Device { Default, Null }` with `FromStr` (`"default"`, `"null"`, otherwise `unknown device`). `pub struct CpalOut`, `pub struct NullOut`, both `::new()` and `impl AudioOut`. `pub fn output(device: Device) -> Box<dyn AudioOut>`.
- `CpalOut` keeps Plan 2's `audio.rs` behaviour as is: f32 output, asks for the wanted rate and falls back to the device default, maps L/R onto channels 0 and 1 with a 16 384-frame preallocated scratch. The stream lives on its own thread, because a cpal `Stream` may not be `Send`. The returned handle is `Send`, and dropping it signals the thread and joins it. `NullOut` echoes the wanted rate and runs a real-time-paced 512-frame thread. `play.rs` prints Plan 2's `note: playing at …` line by comparing `sample_rate(wanted)` with the song's rate.

- [ ] **Step 1: Write failing tests** (`null_out.rs`, mini): `null_out_runs_in_real_time`: after `start` and a 500 ms sleep, `controller.position()` lies in 19 200..=28 800 (0.4–0.6 s at 48 kHz). `drop_stops_output`: after dropping the handle, position changes by < 960 frames over the next 200 ms. `null_rate_echoes`: `NullOut::new().sample_rate(44100) == Ok(44100)`. `device_from_str`: `"null"` parses and `"usb"` errors with `unknown device`.
- [ ] **Step 2: Run** `cargo test -p tune-audio` — Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** the crate, move the code, switch `play.rs`.
- [ ] **Step 4: Run** `cargo test -p tune-audio -p tune-cli && ! cargo tree -p tune-engine -p tune-song | rg -q cpal` — Expected: PASS (`play_cli` unchanged), and the `cargo tree` check exits 0.
- [ ] **Step 5: Run** `task verify`, then commit: `refactor(audio): move cpal output into tune-audio`.

### Task 3: `tune-studio` — edit paths and the in-place song document

**Files:**
- Create: `crates/tune-studio/{Cargo.toml,src/lib.rs,src/path.rs,src/doc.rs}`, `crates/tune-studio/tests/{doc.rs,fixtures/commented.toml}`
- `commented.toml` is mini (Plan 2 fixture) plus four comments: `# the body` above `[tracks.kick]`, a trailing `# headroom` after `gain_db = -3`, `# the squelch` above `[tracks.acid]`, and `# acid enters` above the second `[[sections]]`.

**Interfaces:**
- Consumes: `parse_song`, `Song`, `SongError`, `AcidPattern`/`DrumPattern` `Display`, every param table.
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq, Hash)]
  pub enum SongPath { Bpm, TrackField { track: String, field: TrackField }, TrackParam { track: String, param: String },
      FxMode { track: String }, Track { track: String }, Pattern { pattern: String },
      SectionBars(usize), SectionName(usize), SectionPlay { section: usize, track: String }, SectionLanes(usize), Sections }
  pub enum TrackField { GainDb, Pan, Mute, Solo }
  impl Display for SongPath; impl FromStr for SongPath { type Err = String; }  // serde as the string
  pub struct SongDoc;   // toml_edit::DocumentMut + validated Song, always in agreement
  impl SongDoc {
      pub fn parse(text: &str) -> Result<Self, Vec<SongError>>;
      pub fn text(&self) -> String; pub fn song(&self) -> &Song;
      pub fn get(&self, path: &SongPath) -> Option<serde_json::Value>;   // effective value (defaults filled)
      pub fn set(&mut self, path: &SongPath, value: &serde_json::Value) -> Result<(), EditError>;
      pub fn move_section(&mut self, from: usize, to: usize) -> Result<(), EditError>;
      pub fn duplicate_section(&mut self, index: usize) -> Result<(), EditError>;   // copy inserted after index
      pub fn delete_section(&mut self, index: usize) -> Result<(), EditError>;
  }
  pub enum EditError { UnknownPath(String), ReadOnly(String), NoSuchTarget(String), BadValue { path: String, message: String },
      OutOfBounds { index: usize, len: usize }, LastSection, Invalid(Vec<SongError>) }
  // thiserror Display; NoSuchTarget(p) prints "`<p>` does not exist in this song", LastSection "a song needs at least one section"
  ```
- Path strings: `song.bpm`, `tracks.<t>.gain_db|pan|mute|solo`, `tracks.<t>.<param id>` (instrument or fx param, e.g. `tracks.acid.acid303.cutoff`, `tracks.acid.dist.drive`), `tracks.<t>.dist.mode`, `tracks.<t>`, `patterns.<p>.steps`, `sections[<i>].bars|name|lanes`, `sections[<i>].play.<t>`, `sections`. `tracks.<t>`, `sections[<i>].lanes` and `sections` can be read and diffed but not set (`ReadOnly`). The section ops above edit `sections`.
- `set` values: numbers for `bpm`/`gain_db`/`pan`/params/`bars`, bools for `mute`/`solo`, strings for `dist.mode`/`steps`/`name`, and a string or `null` (key removed) for `play`. A param missing from `params` is inserted, and the inline table is created when absent. Numbers that are exact integers are written as integers (`-6`, not `-6.0`).
- Every mutation runs on a clone of the document, re-parses the clone's text with `parse_song`, and commits only on success. Otherwise it returns `Invalid` and leaves the document untouched. Section moves carry each table's decor (its leading comments) and its nested `[[sections.automation]]` with it, and reassign table positions so the document renders in the new order.

- [ ] **Step 1: Write failing tests** (`doc.rs`): `paths_round_trip`: every example string above survives `parse` then `to_string`. `"foo.bar"`, `"sections[x].bars"` and `"tracks.Acid.gain_db"` give `Err`. `set_param_preserves_comments`: on `commented.toml`, setting `tracks.acid.acid303.cutoff = 820` changes exactly one line, and that line contains `"acid303.cutoff" = 820`. All four comments stay byte-identical. `set_param_inserts_missing_key`: `tracks.kick.kick.tune = 60` makes `song().tracks["kick"].params["kick.tune"] == 60.0`. `set_rejects_invalid`: cutoff 99999 gives `Invalid` with `is out of range`, and `text()` is unchanged. `integers_written_without_fraction`: `gain_db = -6.0` gives the line `gain_db = -6`, and `pan = 0.25` gives `pan = 0.25`. `play_null_removes_key`: `sections[1].play.acid = null` leaves `b` playing `kick` and `hat` only. `move_section_carries_comments_and_lanes`: `move_section(1, 0)` gives names `[b, a]`, `# acid enters` before the first `[[sections]]`, and `b` keeps its lane. `duplicate_and_delete`: duplicate 0 gives `[a, a, b]`, then delete 2 gives `[a, a]`, delete 0 gives `[a]`, and a further delete gives `LastSection`. `bars_and_mode`: `sections[0].bars = 8` makes `total_bars() == 10`, and `tracks.acid.dist.mode = "hard"` sets Hard. `no_such_target`: `tracks.kick.dist.drive` gives `NoSuchTarget`. `read_only`: setting `sections` gives `ReadOnly`. `get_effective`: `get(tracks.acid.acid303.tuning) == 440`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test doc` — Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** `path.rs` and `doc.rs` (deps: `tune-song`, `tune-dsp`, `toml_edit`, `serde`, `serde_json`, `thiserror`, `indexmap`).
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(studio): edit song files in place by path`.

### Task 4: Song diff and the edit log

**Files:**
- Create: `crates/tune-studio/src/{diff.rs,log.rs}`, `crates/tune-studio/tests/diff_log.rs`
- Modify: `.gitignore` (add `songs/*/edits.jsonl`)

**Interfaces:**
- Consumes: `SongPath`, `SongDoc::get` semantics (effective values).
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Change { pub path: SongPath, pub old: Value, pub new: Value }   // path serialized as its string
  pub fn diff_songs(old: &Song, new: &Song) -> Vec<Change>;                  // song-file order
  pub fn is_whole_replace(old_text: &str, new_text: &str) -> bool;           // > 50 % of old non-blank lines gone
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
  pub enum Source { Human, Claude, External }
  pub struct EditEntry { pub ts: jiff::Timestamp, pub source: Source, pub path: String, pub old: Value, pub new: Value,
                         pub file_hash: Option<String> }   // serde; ts as RFC 3339 UTC, exactly 3 fraction digits; file_hash skipped when None
  pub struct EditLog;
  impl EditLog {
      pub fn for_song(song_path: &Path) -> Self;                       // <dir>/edits.jsonl
      pub fn append(&self, entries: &[EditEntry]) -> io::Result<()>;   // one O_APPEND write of all lines
      pub fn read_since(&self, since: Option<jiff::Timestamp>) -> io::Result<Vec<EditEntry>>;  // ts > since; bad lines skipped
      pub fn has_claude_hash(&self, hash: &str) -> io::Result<bool>;   // scans the last 256 lines
  }
  pub fn text_hash(text: &str) -> String;   // FNV-1a 64 of the UTF-8 bytes, 16 lowercase hex digits
  ```
- Diff rules: compare per path with effective values. A changed section count or order gives one `sections` change whose values are arrays of `"<name>:<bars>"`. Otherwise it gives per-section `bars`, `name`, `play.<t>` and `lanes` changes, where lanes are compared as a whole and valued `[{track,param,points}]`. A track that is added or removed gives `tracks.<t>` with `null` on the missing side and `{"instrument": …}` on the other, and no per-field changes for that track.

- [ ] **Step 1: Write failing tests.** `diff_param_change`: mini against mini with cutoff 820 gives exactly `[Change { tracks.acid.acid303.cutoff, 500, 820 }]`. `diff_removed_param_shows_default`: deleting the explicit resonance gives old 70 and new 0. `diff_steps`: a changed `a1` gives `patterns.a1.steps` with both canonical strings. `diff_sections_reorder`: swapping a and b gives one `sections` change, `["a:2","b:2"] → ["b:2","a:2"]`. `diff_bars_only`: gives `sections[0].bars` 2 → 4. `diff_track_added`: a new clap track gives `tracks.clap`, `null → {"instrument":"clap"}`. `diff_identical_is_empty`. `whole_replace_threshold`: changing 3 lines of mini gives false, and mini against Plan 2's `songs/first-acid/song.toml` gives true. `log_append_and_read_since`: with entries at t1 < t2 < t3, `read_since(Some(t1))` returns t2 and t3, and `read_since(None)` returns all 3. `malformed_line_skipped`: a garbage line between two entries leaves 2 read. `concurrent_appends_do_not_interleave`: two threads each doing 200 single-entry appends leave 400 parseable lines. `hash_pinned`: `text_hash("") == "cbf29ce484222325"` and `text_hash("a") == "af63dc4c8601ec8c"`. `has_claude_hash`: true only for a `claude` entry with that hash. `entry_json_shape`: an entry serializes to exactly `{"ts":"2026-10-21T22:15:03.120Z","source":"human","path":"song.bpm","old":120,"new":121}`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test diff_log` — Expected: FAIL.
- [ ] **Step 3: Implement** `diff.rs` and `log.rs`, and add the ignore rule.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(studio): diff songs by edit path`; `feat(studio): add the per-song edit log`.

### Task 5: `tune set` and `tune changes`

**Files:**
- Create: `crates/tune-studio/src/{set.rs,error.rs}`, `crates/tune-cli/src/cmd/{set.rs,changes.rs}`, `crates/tune-cli/tests/edit_cli.rs`
- Modify: `crates/tune-cli/{Cargo.toml,src/main.rs,src/cmd/mod.rs}`

**Interfaces:**
- Consumes: `SongDoc`, `diff_songs`, `EditLog`, `text_hash`, Plan 2 `report` helpers.
- Produces:
  ```rust
  pub fn parse_assignment(s: &str) -> Result<(SongPath, Value), String>;  // split at the first '='
  pub struct SetOutcome { pub changes: Vec<Change>, pub file_hash: String }
  pub fn set_paths(song_path: &Path, assignments: &[(SongPath, Value)], source: Source) -> Result<SetOutcome, StudioError>;
  pub fn write_atomic(song_path: &Path, text: &str) -> io::Result<()>;   // Global Constraints tmp+rename
  // error.rs (thiserror; serde Serialize for ErrorView):
  pub struct ErrorView { pub line: Option<usize>, pub column: Option<usize>, pub message: String }  // From<&SongError>
  pub enum StudioError { Io(String), Load(Vec<ErrorView>), Edit(EditError), FileInvalid, NoSong, NotAnEdit, Audio(String), Timeout }
  // FileInvalid prints "the song file has errors; fix it first", NoSong "no song is open"
  ```
- Values: the text after `=` is parsed as a TOML value (`820`, `true`, `"hard"`). If that fails, the raw text is a string (`hard`). An empty value means `null` (`sections[1].play.acid=`).
- `set_paths` applies every assignment to one `SongDoc`, writes once, and appends one entry per `diff_songs` change, all carrying `file_hash`. If any assignment fails, nothing is written.
- CLI: `tune set <SONG> <PATH=VALUE>... [--json]`. Text output: `set tracks.acid.acid303.cutoff 500 -> 820`. JSON: `{"ok":true,"changes":[{"path","old","new"}],"file_hash"}`. `tune changes <SONG> [--since <RFC3339>] [--source human|claude|external] [--json]`. Text output: `22:15:03 human tracks.acid.acid303.cutoff 500 -> 820`, using local time. JSON: `{"ok":true,"until":<last ts or since or null>,"changes":[entries]}`. A missing log gives an empty list.

- [ ] **Step 1: Write failing tests** (`edit_cli.rs`, `commented.toml` copied into a tempdir): `set_writes_and_logs_claude`: `set song.toml tracks.acid.acid303.cutoff=820 --json` gives `changes[0].old == 500` and `.new == 820`. All four comments are still in the file. The last `edits.jsonl` line has `source == "claude"` and `file_hash == text_hash(file)`. `set_is_all_or_nothing`: `cutoff=820 tracks.acid.acid303.resonance=999` gives exit 2 with `is out of range`, a byte-identical file and no log file. `set_empty_value_removes_play` and `set_bare_string_mode` (`tracks.acid.dist.mode=hard`). `set_unknown_path`: `tracks.acid.cutof=1` gives exit 2 with `does not exist in this song`. `changes_since`: with a hand-written log at t1 < t2 < t3, `changes --since <t1> --json` gives 2 entries and `until == t3`, and `--source human` filters. `changes_missing_log`: `changes == []` and `until == null`.
- [ ] **Step 2: Run** `cargo test -p tune-cli --test edit_cli` — Expected: FAIL.
- [ ] **Step 3: Implement** `set.rs` and both commands.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(cli): add tune set for tagged song edits`; `feat(cli): add tune changes to read the edit log`.

### Task 6: Studio core — commands, write-back, undo

**Files:**
- Create: `crates/tune-studio/src/{command.rs,view.rs,core.rs}`, `crates/tune-studio/tests/core_edits.rs`

**Interfaces:**
- Consumes: Tasks 3–5.
- Produces:
  ```rust
  #[derive(Clone, Debug, Serialize, Deserialize)] #[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
  pub enum Command {
      ListSongs, OpenSong { path: String }, Play { from_bar: Option<u32> }, Stop, SeekBar { bar: u32 },
      SetLoop { bars: Option<[u32; 2]> },
      SetParam { track: String, param: String, value: f64, commit: bool },   // commit=false: live drag only
      SetFxMode { track: String, mode: String },
      SetAcidStep { pattern: String, index: usize, note: Option<u8>, accent: bool, slide: bool },
      SetDrumStep { pattern: String, index: usize, hit: DrumHitDto },
      MoveSection { from: usize, to: usize }, ResizeSection { index: usize, bars: u32 },
      DuplicateSection { index: usize }, DeleteSection { index: usize },
      SetSectionPlay { section: usize, track: String, pattern: Option<String> },
      SetTrackMix { track: String, gain_db: Option<f64>, pan: Option<f64>, mute: Option<bool>, solo: Option<bool> },
      Undo, Redo, ResolveOffered { reapply: bool },
  }
  #[serde(rename_all = "lowercase")] pub enum DrumHitDto { Rest, Hit, Accent }
  // view.rs (all Serialize; ErrorView and StudioError come from Task 5's error.rs):
  pub struct StudioState { pub path: Option<String>, pub song: Option<SongView>, pub playing: bool, pub cursor_bar: u32,
      pub loop_bars: Option<[u32; 2]>, pub can_undo: bool, pub can_redo: bool, pub offered: Vec<Change>, pub file_errors: Vec<ErrorView> }
  pub struct SongView { pub name: String, pub bpm: f64, pub total_bars: u32, pub tracks: Vec<TrackView>,
      pub patterns: Vec<PatternView>, pub sections: Vec<SectionView> }
  pub struct TrackView { pub id: String, pub instrument: String, pub gain_db: f64, pub pan: f64, pub mute: bool, pub solo: bool,
      pub params: Vec<ParamView>, pub fx: Vec<FxView> }
  pub struct ParamView { pub id: String, pub label: String, pub unit: String, pub min: f64, pub max: f64,
      pub curve: String /* "linear"|"exponential" */, pub default: f64, pub help: String, pub value: f64, pub explicit: bool }
  pub struct FxView { pub kind: String, pub mode: String, pub modes: Vec<String>, pub params: Vec<ParamView> }
  #[serde(tag = "kind", rename_all = "lowercase")]
  pub enum PatternView { Acid { id: String, track: String, steps: Vec<AcidStepView> }, Drum { id: String, track: String, hits: Vec<DrumHitDto> } }
  pub struct AcidStepView { pub note: Option<u8>, pub accent: bool, pub slide: bool }
  pub struct SectionView { pub name: String, pub start_bar: u32, pub bars: u32, pub play: IndexMap<String, String>, pub lanes: Vec<LaneView> }
  pub struct LaneView { pub track: String, pub param: String, pub points: Vec<[f64; 2]> }
  pub struct UndoEntry { pub before: String, pub after: String, pub source: Source, pub changes: Vec<Change> }
  pub struct StudioCore;
  impl StudioCore {
      pub fn open(path: &Path) -> Result<Self, StudioError>;
      pub fn apply_edit(&mut self, cmd: &Command, now: Instant) -> Result<Vec<Change>, StudioError>; // in memory, pending
      pub fn flush_due(&self, now: Instant) -> bool;     // pending and ≥ 150 ms since the last edit
      pub fn flush(&mut self) -> Result<Vec<Change>, StudioError>;  // see below; returns the burst's changes
      pub fn undo(&mut self) -> Result<Vec<Change>, StudioError>; pub fn redo(&mut self) -> Result<Vec<Change>, StudioError>;
      pub fn is_automated(&self, track: &str, param: &str, bar: u32) -> bool;   // lane in the section holding `bar`
      pub fn song(&self) -> &Song; pub fn path(&self) -> &Path; pub fn view(&self) -> SongView;
      pub fn can_undo(&self) -> bool; pub fn can_redo(&self) -> bool;
  }
  ```
- `apply_edit` maps each edit command onto `SongDoc`. `SetParam { commit: true }` sets `tracks.<t>.<param>`, rounded to 4 significant digits. `SetAcidStep`/`SetDrumStep` rebuild the pattern from the model, change one step, and set `patterns.<p>.steps` to its `Display` string. `ResizeSection` sets `sections[i].bars`. `SetTrackMix` sets each `Some` field, rounding the numbers to 4 significant digits. `SetParam { commit: false }` and the non-edit commands return `NotAnEdit`.
- Pending edits are kept as the applied `Command`s, each with the changes it produced, since the last flush (Task 7 re-applies them when merging).
- `flush` writes the document atomically (Task 7 adds a disk check in front of it), records the hash in the own-write ring, pushes one `UndoEntry { before: last disk text, after, Human, changes }` (dropping the oldest past 500, clearing redo), and appends one `human` log entry per `diff_songs(before, after)` change. `undo` flushes any pending edits, then writes `before` of the top entry, moves the entry to redo, and logs the reversed diff as `human`. `redo` mirrors it.

- [ ] **Step 1: Write failing tests** (`core_edits.rs`, `commented.toml` in a tempdir, `t0 = Instant::now()`): `edit_pending_until_debounce`: after `SetParam{acid, acid303.cutoff, 820, commit: true}` at t0, the file is unchanged, `flush_due(t0+100ms)` is false and `flush_due(t0+150ms)` is true. `flush()` puts 820 in the file with the comments intact, and adds exactly one log entry `{source: human, path: tracks.acid.acid303.cutoff, old: 500, new: 820}`. `burst_is_one_undo_step`: three `SetDrumStep` on `h` at t0, +50 ms and +100 ms, then `flush`, then `undo` restores the original text byte for byte and `can_redo()`. `redo` restores the flushed text. `held_arrow_coalesces` (**Review Focus 5**): 30 `SetParam` commits 10 ms apart (cutoff 500 → 529), with `flush_due(now)` checked after each one and a flush whenever it is true, then a final check at +160 ms. Expected: exactly 1 flush, 1 undo entry, and 1 log entry with `old 500, new 529`. `undo_flushes_pending_first`. `new_edit_clears_redo`. `undo_logged_as_human_reversed`. `knob_value_rounded`: 823.456789 is written as `823.5`. `commands_map_to_paths` (table: command, then expected change paths): SetParam resonance → `tracks.acid.acid303.resonance`; SetFxMode hard → `tracks.acid.dist.mode`; SetAcidStep{a1,0,Some(48),true,false} → `patterns.a1.steps` with a new value starting `"C3! "`; SetDrumStep{h,0,accent} → `patterns.h.steps` with a new value starting `"X.x."`; MoveSection{1,0}, DuplicateSection{0} and DeleteSection{0} → `sections`; ResizeSection{0,8} → `sections[0].bars`; SetSectionPlay{0,acid,Some(a1)} → `sections[0].play.acid`; SetTrackMix{hat, gain −6, mute true} → `tracks.hat.gain_db`, `tracks.hat.mute`. `step_index_out_of_range`: `SetDrumStep { h, 99, hit }` gives `Edit(OutOfBounds { index: 99, len: 16 })`. `no_tmp_left`: after a flush the dir holds exactly `song.toml` and `edits.jsonl`. `undo_depth_capped`: 501 flushes give 500 undos, then `can_undo()` is false.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test core_edits` — Expected: FAIL.
- [ ] **Step 3: Implement** `command.rs`, `view.rs` and `core.rs` (edits, flush, undo/redo).
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(studio): apply edits with debounced write-back and undo`.

### Task 7: Studio core — external changes, own writes, merge

**Files:**
- Modify: `crates/tune-studio/src/core.rs`
- Test: `crates/tune-studio/tests/core_external.rs`

**Interfaces:**
- Consumes: Task 6, `is_whole_replace`, `EditLog::has_claude_hash`, `set_paths`.
- Produces:
  ```rust
  #[derive(Clone, Debug, Serialize)]
  pub struct ExternalChange { pub source: Source, pub changes: Vec<Change>, pub replaced: bool,
      pub merged: Vec<Change>, pub offered: Vec<Change>, pub errors: Vec<ErrorView> }
  impl StudioCore {
      pub fn on_disk_text(&mut self, text: String) -> Option<ExternalChange>;   // None for own writes and no-ops
      pub fn resolve_offered(&mut self, reapply: bool) -> Result<Vec<Change>, StudioError>;
      pub fn offered(&self) -> &[Change]; pub fn file_errors(&self) -> &[ErrorView];
  }
  ```
- `flush` now starts by reading the file. If its hash is neither the last disk hash nor an own-write hash, `flush` runs `on_disk_text` on it before writing.
- Rules, in order:
  1. If the hash is in the own-write ring or equals the last disk hash, return `None`.
  2. If the text does not parse, set `file_errors`, keep the last good document and the pending edits, and return `Some` with `errors` only. While `file_errors` is non-empty, `apply_edit`, `undo` and `redo` return `FileInvalid`, and `flush` writes nothing.
  3. Source: `Claude` if `has_claude_hash(hash)`, else `External`. `External` changes are logged with that source. `Claude` changes are not logged again, because `tune set` already logged them.
  4. Push `UndoEntry { before: last disk text, after: text, source, changes }` and clear redo.
  5. With no pending edits: adopt the text.
  6. With pending edits and `is_whole_replace`: adopt the text, move the pending edits to `offered`, and set `replaced = true`.
  7. Otherwise merge by path. Each pending command whose changes touch no path in the external diff is re-applied, in order, onto the new document; its changes go to `merged`. A pending command that touches a path the external diff touched keeps the external value, because it was written last, and goes to `offered`. A command that no longer applies (`NoSuchTarget`, `OutOfBounds`, `Invalid`) also goes to `offered`. After a merge, flush immediately, so the merge becomes its own `Human` undo entry.
  8. `offered()` lists the offered commands' changes for display. `resolve_offered(true)` re-applies the stored commands that still apply, as one human edit, then flushes. `resolve_offered(false)` drops them.

- [ ] **Step 1: Write failing tests** (`core_external.rs`): `own_write_ignored`: after a flush, `on_disk_text(file text)` is `None`, with no new log line and no new undo entry. `external_edit_logged_and_undoable`: text with `bpm = 121` gives `source: External`, changes `[song.bpm 120→121]`, `replaced == false` and one `external` log line. Then `undo` writes bpm 120 back to disk. `claude_edit_attributed`: `set_paths(.., Claude)` then `on_disk_text(file)` gives `source: Claude` and the log line count is unchanged. `merge_keeps_both_sides` (§8 no lost edits): pending cutoff 820 (not flushed) plus external bpm 121 gives a file with both `bpm = 121` and `820`, and `merged == [cutoff]`. One undo gives cutoff 500 with bpm 121, and a second gives bpm 120. `pending_section_move_merges`: pending `MoveSection { 1, 0 }` plus external bpm 121 gives a file with bpm 121 and sections `[b, a]`. `same_path_conflict_offers_local`: pending cutoff 820 and external cutoff 700 give 700 on disk and `offered == [cutoff →820]`. `resolve_offered(true)` puts 820 on disk. `whole_replace_offers_pending`: pending cutoff 820, then the text of `songs/first-acid/song.toml` arrives. Expected: `replaced`, the file equals that text byte for byte, and `offered.len() == 1`. `resolve_offered(false)` clears it, and `undo` restores the commented fixture byte for byte. `pending_survives_invalid_external` (**Review Focus 2**): pending cutoff 820, then `"[song\n"` gives `errors[0].line == Some(1)`. `apply_edit` returns `FileInvalid`. The full valid text with bpm 121 then gives a disk file with bpm 121 and cutoff 820. `flush_detects_unseen_external`: pending cutoff 820, a direct `fs::write` of bpm 121 with no `on_disk_text` call, then `flush` gives a disk file holding both.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test core_external` — Expected: FAIL.
- [ ] **Step 3: Implement** the rules above.
- [ ] **Step 4: Run** `cargo test -p tune-studio` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(studio): merge external song changes without losing edits`.

### Task 8: Studio actor — live audio, watcher, events

**Files:**
- Create: `crates/tune-studio/src/studio.rs`, `crates/tune-studio/tests/studio.rs`
- Modify: `crates/tune-song/src/watch.rs` (+ test in `tests/watch.rs`), `crates/tune-studio/Cargo.toml` (`tune-engine`)

**Interfaces:**
- Consumes: `StudioCore`, `SongWatcher`, `CompiledSong`, `live`, `LiveController` (Task 1), `AudioOut`.
- Produces:
  ```rust
  pub enum RawReload { Changed(String), Io(String) }   // tune-song; same debounce and filtering as start()
  impl SongWatcher { pub fn start_raw(path: &Path, tx: Sender<RawReload>) -> Result<Self, notify::Error>; }
  #[derive(Clone, Debug, Serialize)] #[serde(tag = "event", rename_all = "snake_case")]
  pub enum Event { State(StudioState), Playhead { step: u64, bar: u32, beat: u32, playing: bool },
      Applied { source: Source, changes: Vec<Change> }, ExternalChange(ExternalChange), Error { message: String } }
  pub struct StudioConfig { pub audio: Box<dyn AudioOut>, pub songs_root: PathBuf }
  pub struct Studio;
  impl Studio { pub fn spawn(config: StudioConfig, song: Option<PathBuf>) -> Result<StudioHandle, StudioError>; }
  #[derive(Clone)] pub struct StudioHandle;
  impl StudioHandle {
      pub fn call(&self, cmd: Command) -> Result<serde_json::Value, StudioError>;   // blocks, 5 s timeout
      pub fn subscribe(&self) -> Receiver<Event>;   // first event is the current State
      pub fn shutdown(&self);
  }
  ```
- One actor thread owns the `StudioCore`, the `LiveController` and the audio handle. Commands and watcher events arrive as one `Msg` channel; a forwarding thread maps `RawReload` into it. The loop runs `recv_timeout(33 ms)`. On each tick: emit `Playhead` while playing (`step = clock.step_at(position)`, `bar = step / 16`, `beat = step % 16 / 4`), collect garbage, flush when due, and stop on `finished`.
- After every applied edit, undo/redo or adopted external text, while playing: compile at the running rate and `swap`, using `swap_clearing_overrides` for a committed `SetParam`. Then emit `Applied` (human) or `ExternalChange`, followed by `State`.
- `Play { from_bar }` (default `cursor_bar`): `audio.sample_rate(song rate or 48 000)`, `live(compiled, bar, loop)`, `audio.start`. `Stop` drops the handle and sets `cursor_bar` to the current bar. `SeekBar` updates `cursor_bar` and seeks when playing. `SetLoop` stores the range, clamped to the song, and forwards it.
- `SetParam { commit: false }` resolves `param_target` and calls `set_override`. It writes nothing and returns `null`. `SetParam { commit: true }` on a param that `is_automated` at the current bar (the playing bar, else `cursor_bar`) clears the override, writes nothing, and returns `{"automated": true}`.
- After a song change, the stored loop is clamped (or cleared if its start lies past the end) and re-sent. `cursor_bar` is clamped too. `ListSongs` returns the sorted `songs/*/song.toml` paths under `songs_root`, relative to it. `OpenSong` swaps the core and the watcher and keeps audio stopped. Results are `null` except `ListSongs` (an array), `Undo`/`Redo` (the changes) and the automated case.

- [ ] **Step 1: Write failing tests.** In `tune-song/tests/watch.rs`, `raw_reports_text`: a rewrite sends `Changed(full text)`. In `studio.rs`, use a `CaptureOut` test double whose `start` parks the processor in an `Arc<Mutex<Option<LiveProcessor>>>` so the test pulls frames by hand. Every wait is ≤ 2 s:
  - `subscribe_starts_with_state`: `song.name == "mini"`, `playing == false`.
  - `play_emits_playhead`: Play, pull 48 000 frames, then a `Playhead { step: 8, bar: 0, beat: 2 }` arrives.
  - `knob_drag_is_live_only`: a drag of cutoff to 2394 while playing leaves the file unchanged after 300 ms. The next 4800 pulled frames differ from `render(mini)` at the same position.
  - `release_commits`: `commit: true` gives the new value in the file within 500 ms, an `Applied { source: Human }`, and one `human` log line.
  - `external_edit_hot_reloads`: an `fs::write` with bpm 121 gives `ExternalChange`, a `State` with `bpm == 121`, and still `playing`.
  - `own_write_is_quiet`: no `ExternalChange` within 1 s after a commit's flush.
  - `automated_release_writes_nothing`: seek to bar 2 (section b) and commit cutoff gives a result `{"automated": true}` and an unchanged file.
  - `external_edit_during_drag_keeps_gesture` (**Review Focus 1**): drag cutoff to 2000 (no release), `set_paths(resonance=90, Claude)`, wait for the `ExternalChange`, then release at 2000. The file holds resonance 90 and cutoff 2000.
  - `delete_section_under_playhead` (**Review Focus 3**): loop 2..4, play at bar 3, delete section 1. No panic, the state's `loop_bars` is `None`, `cursor_bar` < `total_bars`, and pulling 200 000 more frames succeeds.
  - `stop_keeps_cursor`: play from 0, pull 96 000 frames, Stop, then Play with `from_bar: None` gives a first Playhead with `bar == 1`.
  - `list_and_open`: with two song dirs in a tempdir root, `ListSongs` gives `["songs/a/song.toml","songs/b/song.toml"]`, and `OpenSong` b gives a State whose `path` ends with `songs/b/song.toml`.
- [ ] **Step 2: Run** `cargo test -p tune-song --test watch && cargo test -p tune-studio --test studio` — Expected: FAIL.
- [ ] **Step 3: Implement** `start_raw` and `studio.rs`.
- [ ] **Step 4: Run** the same commands, then `! cargo tree -p tune-studio | rg -q 'cpal|tokio|axum|tauri'` — Expected: PASS, and the `cargo tree` check exits 0.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(song): expose raw watcher reloads`; `feat(studio): run the studio actor with live audio and events`.

### Task 9: `tune serve` and the wire protocol

**Files:**
- Create: `crates/tune-cli/src/cmd/serve.rs`, `crates/tune-cli/tests/serve_cli.rs`, `crates/tune-studio/tests/protocol.rs`, `crates/tune-studio/tests/fixtures/protocol.json`
- Modify: `crates/tune-cli/{Cargo.toml,src/main.rs}`

**Interfaces:**
- Consumes: `Studio`, `StudioHandle`, `Command`, `Event`, `tune_audio::output`.
- Produces: `tune serve <SONG> [--port N] [--device default|null] [--ui <DIR>] [--json]`.
  - `--port` defaults to 4580, and `0` picks a free port. `--ui` defaults to `concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/desktop/dist")`. If `<ui>/index.html` is missing, exit 2 with ``UI not built: run `task ui:build` ``.
  - It prints `serving http://127.0.0.1:<port>/` or `{"event":"serving","url":…}` and runs until Ctrl-C (`stopped`).
  - Routes: `GET /*` uses `tower-http` `ServeDir` with an `index.html` fallback. `GET /ws` is the WebSocket.
  - Wire format. Client to server: `{"id":<u64>,"cmd":…,"args":…}`, the `Command` serde form plus `id`. Server to client: `{"reply":<id>,"ok":true,"result":…}` or `{"reply":<id>,"ok":false,"error":"<Display>"}`, plus every `Event` as its serde form. A frame that does not parse gives `{"reply":<id or null>,"ok":false,"error":…}`.
  - Every connection gets its own `subscribe()`, forwarded from a std thread into a tokio mpsc. Calls run in `spawn_blocking`.
- Golden fixture: `protocol.json` is `{"commands":[one per Command variant],"events":[one per Event variant]}`. The `protocol.rs` test serializes fixed samples and compares them with the file; `UPDATE_GOLDEN=1` rewrites it. The frontend tests read the same file (Task 13).

- [ ] **Step 1: Write failing tests.** `protocol.rs`: `golden_matches`, `commands_round_trip` (each golden command deserializes, then reserializes identical). `serve_cli.rs` (child `serve <copy of commented.toml> --port 0 --device null --json --ui <tempdir holding index.html "hello">`, with the URL read from the first line):
  - `serves_index`: `ureq` GET `/` gives 200 and a body containing `hello`. The URL's host is `127.0.0.1`.
  - `ws_first_message_is_state`: `event == "state"` and `song.name == "mini"`.
  - `ws_command_round_trip`: `set_param` commit cutoff 820 (id 1) gives a frame with `reply == 1` and `ok == true`, then within 2 s the file holds 820 and the log has a `human` line.
  - `ws_errors`: `{"id":7,"cmd":"nope"}` gives `reply 7, ok false`, with an error containing `unknown variant`.
  - `rejects_foreign_origin`: a WS upgrade with `Origin: http://evil.example` gives HTTP 403.
  - `two_clients_share_state` (**Review Focus 4**): two sockets; a `set_track_mix` mute on `hat` from socket A makes socket B receive a `state` with `hat.mute == true`.
  - `missing_ui_dir`: `--ui <empty dir>` gives exit 2 with `UI not built`.
- [ ] **Step 2: Run** `cargo test -p tune-studio --test protocol && cargo test -p tune-cli --test serve_cli` — Expected: FAIL.
- [ ] **Step 3: Implement** `serve.rs` and write the golden file with `UPDATE_GOLDEN=1`, then review it by eye.
- [ ] **Step 4: Run** the same commands — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `test(studio): pin the wire protocol with a golden file`; `feat(cli): serve the studio over local HTTP and WebSocket`.

### Task 10: JS workspace, theme, and the Knob

**Files:**
- Create: `package.json` (name `tune-ai`, `packageManager: pnpm@9.15.0`, scripts `lint`, `format:check`, `typecheck`, `test`), `pnpm-workspace.yaml` (`apps/desktop`, `packages/*`), `pnpm-lock.yaml`, `eslint.config.js` (sagepad's, plus `ignores` for `target/` and `crates/`), `.prettierrc` (sagepad's), `.prettierignore` (`target`, `crates`, `songs`, `docs`, `pnpm-lock.yaml`)
- Create: `packages/ui/{package.json (@tune/ui),tsconfig.json,vitest.config.ts,components.json}`, `packages/ui/src/{index.ts,test-setup.ts,tailwind.preset.ts,theme/tokens.css,lib/utils.ts,lib/param.ts,lib/param.test.ts,components/Knob.tsx,components/Knob.test.tsx}`
- Modify: `Taskfile.yml`, `THIRD_PARTY.md` (Barlow typeface family, SIL Open Font License 1.1, https://github.com/jpt/barlow, bundled into app builds)

**Interfaces:**
- Produces (TS):
  ```ts
  export type Curve = "linear" | "exponential";
  export type ParamSpecView = { id: string; label: string; unit: string; min: number; max: number; curve: Curve;
    default: number; help: string; value: number; explicit: boolean };       // = Rust ParamView
  export function toPlain(spec: ParamSpecView, normalized: number): number;   // mirrors ParamSpec::to_plain
  export function toNormalized(spec: ParamSpecView, plain: number): number;
  export function formatValue(spec: ParamSpecView, plain: number): string;
  export type Highlight = "claude" | "external" | null;
  export type KnobProps = { spec: ParamSpecView; value: number; size?: "lg" | "md" | "sm";
    onChange: (plain: number) => void;   // live, every pointer move
    onCommit: (plain: number) => void;   // release, keyboard step, double-click reset
    onHelp?: (spec: ParamSpecView | null) => void; highlight?: Highlight; automated?: boolean };
  export function Knob(props: KnobProps): JSX.Element;
  ```
- Formats: Hz `867 Hz` below 1000 and `1.20 kHz` above; `%` `70%`; ms `400 ms`; dB one decimal (`-3.0 dB`); unitless two decimals (`0.50`). Knob is SVG with a 270° sweep, an LED-red value arc, a pointer line, the label below and the value under it. `role="slider"`, `aria-label` = label, `aria-valuemin/max/now`, `aria-valuetext` = the formatted value. Pointer capture on drag. ArrowUp/Right +0.01 normalized, Shift 0.001, PageUp/PageDown ±0.1, Home/End min/max, each committing. `automated` shows a small amber "lane" mark. `highlight` draws a cyan ring.
- `tokens.css` defines the Visual direction tokens as HSL custom properties in the shadcn naming (`--background` = chassis and so on, plus `--faceplate`, `--led`, `--slide`, `--claude`), dark only. It imports the `@fontsource` weights used. `tailwind.preset.ts` maps them to colours and sets `fontFamily.label`/`fontFamily.value`.
- Taskfile: `install` (`pnpm install --frozen-lockfile`), `ui:build` (`pnpm --filter @tune/desktop build`), `js:check` (`pnpm lint && pnpm format:check && pnpm -r typecheck && pnpm -r test`). `verify` becomes `fmt:check`, `install`, `js:check`, `ui:build`, `lint`, `test`, `privacy`. `ui:build` precedes the cargo steps because the Tauri crate needs `dist/` (Task 14). Until Task 13 lands, `ui:build` is a no-op guarded by `test -f apps/desktop/package.json`.

- [ ] **Step 1: Write failing tests.** `param.test.ts`: `toPlain(cutoff, 0.5)` is close to `Math.sqrt(314 * 2394)` (1e-9); `toNormalized(cutoff, 314) === 0`; `toPlain(resonance, 0.7) === 70`; a round trip over 11 points for both curves within 1e-12; `formatValue(cutoff, 867.04) === "867 Hz"`, `(cutoff, 1200) === "1.20 kHz"`, `(gain, -3) === "-3.0 dB"`, `(waveform, 0.5) === "0.50"`. `Knob.test.tsx`: `exposes slider semantics` (role, name "Cutoff", valuetext "500 Hz"). `vertical drag maps 200 px to full range`: from normalized 0, pointerdown at y 300 and pointermove to y 200 give `onChange(toPlain(spec, 0.5))`; pointerup gives `onCommit` once with the same value. `shift drag is ten times finer`: the same move with shift gives normalized 0.05. `double click resets to default`: `onCommit(spec.default)`. `keyboard steps`: ArrowUp from n gives `toPlain(n + 0.01)`, PageUp n + 0.1, Home min, End max. `clamps at the ends`: ArrowUp at max gives `onCommit(max)`. `focus reports help`: focus gives `onHelp(spec)` and blur gives `onHelp(null)`.
- [ ] **Step 2: Run** `pnpm install && pnpm --filter @tune/ui test` — Expected: FAIL.
- [ ] **Step 3: Implement** the workspace files, tokens, preset, `param.ts` and `Knob.tsx`.
- [ ] **Step 4: Run** `task js:check && bash scripts/check-privacy.sh` — Expected: PASS. If the privacy check flags `pnpm-lock.yaml` (a lock line can look like an email), stop and report the line. Do not weaken the check.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `build(ui): add the pnpm workspace and frontend checks`; `feat(ui): add the instrument theme and Knob control`.

### Task 11: 303 step sequencer and drum grid

**Files:**
- Create: `packages/ui/src/lib/notes.ts`, `packages/ui/src/components/{AcidStepSequencer,DrumGrid}.tsx` (+ `.test.tsx`, `lib/notes.test.ts`)
- Modify: `packages/ui/{package.json (@radix-ui/react-popover),src/index.ts}`

**Interfaces:**
- Produces:
  ```ts
  export type AcidStep = { note: number | null; accent: boolean; slide: boolean };   // = Rust AcidStepView
  export type DrumHit = "rest" | "hit" | "accent";
  export function noteName(midi: number): string;   // Plan 1 notation, sharps: 36 -> "C2", 61 -> "C#3"
  export function nextHit(h: DrumHit): DrumHit;     // rest -> hit -> accent -> rest
  export function AcidStepSequencer(p: { steps: AcidStep[]; playingStep: number | null; highlighted?: Set<number>;
    onChange: (index: number, step: AcidStep) => void }): JSX.Element;
  export function DrumGrid(p: { rows: { pattern: string; label: string; hits: DrumHit[]; highlighted?: Set<number> }[];
    playingStep: number | null; onSet: (pattern: string, index: number, hit: DrumHit) => void }): JSX.Element;
  ```
- Sequencer: one column per step, grouped in fours. Each column has an LED (`data-playing`), a note button showing `noteName` or `—`, an accent toggle and a slide toggle (`aria-pressed`), all buttons. The note button opens a popover piano: 12 keys, octave −/+ (octaves −1..9), and a "Rest" key. On a focused note button: ArrowUp/Down ±1 semitone, Shift ±12, clamped 0..127; `a` toggles accent; `s` toggles slide; `r` or Delete toggles rest. Leaving a rest restores the last note this column held, or 36 if it never held one. ArrowLeft/Right move focus between columns. `playingStep` is the pattern-relative index.
- Drum grid: one row per pattern, cells are buttons with `aria-label` `"<label> step <n>: <hit>"` (1-based `n`). Click sets `nextHit`. Accent cells are filled with `--led`, hits are outlined, and rests are dim.

- [ ] **Step 1: Write failing tests.** `noteName(36) === "C2"`, `noteName(61) === "C#3"`, `noteName(0) === "C-1"`; `nextHit` cycles. Sequencer: `accent toggle` gives `onChange(0, {…, accent: true})`; `arrow up raises a semitone` (36 → 37); `shift arrow raises an octave` (36 → 48); `piano picks a note` (open column 0, choose octave 2, click `D#`, giving note 39); `rest toggle round trip` (36 → null → 36); `playing step is lit` (column 3 has `data-playing="true"`). Drum grid: clicking `"kick step 1: rest"` gives `onSet("k4", 0, "hit")`; clicking a hit gives `"accent"` and an accent gives `"rest"`.
- [ ] **Step 2: Run** `pnpm --filter @tune/ui test` — Expected: FAIL.
- [ ] **Step 3: Implement** the two components and `notes.ts`.
- [ ] **Step 4: Run** `task js:check` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(ui): add 303 step sequencer and drum grid`.

### Task 12: Arrangement, track strip, transport bar

**Files:**
- Create: `packages/ui/src/components/{Arrangement,TrackStrip,TransportBar}.tsx` (+ `.test.tsx`), `packages/ui/src/lib/arrange.ts` (+ test)

**Interfaces:**
- Produces:
  ```ts
  export type SectionView = { name: string; start_bar: number; bars: number; play: Record<string, string>;
    lanes: { track: string; param: string; points: [number, number][] }[] };
  export const PX_PER_BAR = 12;
  export function snapBars(bars: number, fine: boolean): number;      // fine: max(1, round); else max(8, round to 8)
  export function dropIndex(sections: SectionView[], x: number): number;   // by section midpoints
  export function formatPosition(bar: number, beat: number): string;  // 0-based in, "033:2" out
  export function Arrangement(p: { sections: SectionView[]; tracks: { id: string; instrument: string }[];
    patterns: { id: string; track: string }[]; playhead: number /* fractional 0-based bar */; selected: number | null;
    loop: [number, number] | null; highlighted?: Set<string> /* edit paths */;
    onSelect(i: number): void; onSeek(bar: number): void; onMove(from: number, to: number): void;
    onResize(i: number, bars: number): void; onDuplicate(i: number): void; onDelete(i: number): void;
    onSetPlay(section: number, track: string, pattern: string | null): void }): JSX.Element;
  export function TrackStrip(p: { track: { id: string; gain_db: number; pan: number; mute: boolean; solo: boolean };
    onMix(patch: { gain_db?: number; pan?: number; mute?: boolean; solo?: boolean }): void }): JSX.Element;
  export function TransportBar(p: { playing: boolean; bpm: number; bar: number; beat: number; loopOn: boolean;
    canLoop: boolean; canUndo: boolean; canRedo: boolean; songName: string; notice?: ReactNode;
    onPlay(): void; onStop(): void; onToggleLoop(): void; onUndo(): void; onRedo(): void }): JSX.Element;
  ```
- Arrangement: an SVG ruler with a tick every 8 bars, labelled with 1-based bars. Section blocks have `width = bars * PX_PER_BAR`. Dragging a block moves it to `dropIndex`. Dragging the right edge resizes it with `snapBars` (Alt = fine), shows a preview, and calls `onResize` on release only when the result differs. Under each block there is one cell per track: a button with the pattern id or `—` that opens a popover listing the track's patterns plus "None". Each lane draws a thin `--slide` polyline inside its track row. Click selects, double-click seeks to `start_bar`, and the playhead is a `--led` line at `playhead * PX_PER_BAR`. With a section selected: Cmd+D duplicates, Delete/Backspace deletes, Alt+←/→ moves. `highlighted` paths (`sections`, `sections[i].*`) ring the block in `--claude`.
- TrackStrip: gain and pan as `size="sm"` `Knob`s, committing through `onMix` on release (specs: gain −60..12 dB, pan −1..1, both linear), with M/S toggles (`aria-pressed`). TransportBar: Space toggles play/stop (handled by the app, Task 13), and the buttons have `aria-label`s "Play", "Stop", "Loop selected section", "Undo", "Redo". The loop button is disabled when `!canLoop`.

- [ ] **Step 1: Write failing tests.** `snapBars(21, false) === 24`, `snapBars(3, false) === 8`, `snapBars(21, true) === 21`, `snapBars(0, true) === 1`; `formatPosition(32, 1) === "033:2"`; `dropIndex` over `[16, 16, 32]` at x = 10 gives 0 and at x = 300 gives 2. Arrangement: `blocks sized by bars` (rect width 192 for 16 bars); `drag body reorders` (block 1 dragged to x 5 gives `onMove(1, 0)`); `edge drag snaps to 8` (+60 px on a 16-bar block gives `onResize(0, 24)`, and with Alt gives 21); `tiny edge jitter does nothing` (+2 px gives no call); `double click seeks` (`onSeek(16)` for section 1 of `[16, 16]`); `cell picks a pattern` (`onSetPlay(0, "acid", "a1")`) and `None` gives `null`; `lanes are drawn` (one `polyline` per lane); `playhead x` = 4.5 × 12. TrackStrip: `M` gives `onMix({ mute: true })`. TransportBar: `loop disabled without selection`; Play click gives `onPlay`.
- [ ] **Step 2: Run** `pnpm --filter @tune/ui test` — Expected: FAIL.
- [ ] **Step 3: Implement** the components and `arrange.ts`.
- [ ] **Step 4: Run** `task js:check` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(ui): add arrangement timeline, track strip and transport bar`.

### Task 13: Studio frontend — clients, store, screens

**Files:**
- Create: `apps/desktop/{package.json (@tune/desktop),index.html,vite.config.ts,vitest.config.ts,tailwind.config.ts,postcss.config.js,tsconfig.json}` (sagepad's shape: port 1420, jsdom, preset from `@tune/ui`)
- Create: `apps/desktop/src/{main.tsx,App.tsx,test-setup.ts}`, `src/client/{protocol.ts,StudioClient.ts,wsClient.ts,tauriClient.ts,pickClient.ts}`, `src/store/{studioStore.ts,changedSteps.ts}`, `src/screens/{StudioScreen,OpenSongScreen,Panel303,HelpStrip,ChangeNotice}.tsx`, tests beside each

**Interfaces:**
- Consumes: the wire protocol (Task 9 golden file), the `@tune/ui` components.
- Produces:
  ```ts
  // protocol.ts: hand-written mirrors of Command, Event, StudioState, SongView, Change, ExternalChange,
  // plus `export const COMMAND_NAMES: readonly string[]` and `EVENT_NAMES` (the snake_case tags)
  export interface StudioClient { call<T = unknown>(cmd: Command): Promise<T>; subscribe(h: (e: StudioEvent) => void): () => void; }
  export function createWsClient(url: string, Ws?: typeof WebSocket): StudioClient;   // queues until open
  export function createTauriClient(): StudioClient;    // invoke("studio", { command }), listen("studio-event")
  export function pickClient(): StudioClient;           // "__TAURI_INTERNALS__" in window ? Tauri : Ws(`ws://${location.host}/ws`, or VITE_STUDIO_WS)
  export function changedSteps(oldSteps: string, newSteps: string): number[];   // acid: by token; drum: by char, spaces dropped
  export type StudioView = { state: StudioState | null; playhead: { step: number; bar: number; beat: number } | null;
    highlights: Map<string, "claude" | "external">; notice: ExternalChange | null; selectedSection: number | null;
    connection: "open" | "lost" };
  export function createStudioStore(client: StudioClient): { get(): StudioView; subscribe(l: () => void): () => void;
    send(cmd: Command): Promise<unknown>; select(i: number | null): void };   // consumed via useSyncExternalStore
  ```
- `wsClient`: increasing ids and a pending map. `ok:false` rejects with `new Error(error)`. On close every pending call is rejected with "Lost connection to the studio", and `connection` becomes `lost`. The UI then shows a banner telling the person to restart `tune serve` and reload. There is no auto-reconnect.
- Store: `state` replaces the snapshot. `external_change` sets `notice` and highlights every changed path, with `patterns.<p>.steps` expanded to step indices through `changedSteps`, for 4 s. `applied` from a human source sets nothing.
- Screens:
  - `StudioScreen` lays out the Visual direction wireframe.
  - `Panel303` shows the first `acid303` track, with tabs when there are several. The big knob row is Tuning, Cutoff, Resonance, Env mod, Decay, Accent. The smaller row is Waveform, Slide time, Volume, then a distortion group (Drive, Mix, Output and a three-position mode switch: soft, hard, overdrive). The step sequencer shows the acid pattern the selected section (or the section under the playhead) plays on that track. If none is played, it says "This section plays no 303 pattern. Pick one in the arrangement."
  - Knob `onChange` sends `set_param commit:false`, throttled to one per animation frame. `onCommit` sends `commit:true`.
  - `HelpStrip` shows the hovered or focused knob's label and help, and by default "Hover or focus a knob to learn what it does."
  - The drum grid shows the drum patterns the selected section plays.
  - `ChangeNotice` reads "Claude changed 2 things: Cutoff 500 → 820 Hz, Resonance 70 → 90%" with an "Undo" button. On `replaced` it reads "The song file was replaced. Re-apply 1 unsaved edit?" with "Re-apply" and "Discard".
  - `OpenSongScreen` appears when `state.path` is null. It lists `list_songs` results as buttons; an empty list reads "No songs yet. Ask Claude to draft one in songs/<name>/song.toml."
  - A file error banner reads "song.toml has 2 errors. Still playing the last good version." and lists `line:column message`.
  - Keys: Space play/stop, Cmd+Z undo, Shift+Cmd+Z redo, L loop selected section, ignored while focus is in a popover.

- [ ] **Step 1: Write failing tests.** `wsClient.test.ts` (fake WebSocket): `call sends id, cmd and args, and resolves on its reply`; `ok false rejects with the server error`; `events reach subscribers`; `close rejects pending calls`; `calls before open are queued`. `changedSteps.test.ts`: `("C2 . D2", "C2 C2 D2")` gives `[1]`, and `("x... x...", "x... X...")` gives `[4]`. `studioStore.test.ts` (fake timers): `state replaces snapshot`; `external change highlights for 4 s` (the path is present at 3.9 s and gone at 4.1 s). `protocol.test.ts`: every golden command has a `cmd` in `COMMAND_NAMES`, every golden event has an `event` in `EVENT_NAMES`, and both lists are exactly the golden sets. `App.test.tsx` (fake client emitting the golden `state`): the 303 panel shows sliders named Cutoff, Resonance, Env mod, Decay, Accent and Tuning. Space calls `play`. Dragging Cutoff then releasing calls `set_param` with `commit:false`, then once with `commit:true`. A golden `external_change` with `replaced:true` shows "Re-apply", whose click sends `resolve_offered { reapply: true }`. A null path shows the song list.
- [ ] **Step 2: Run** `pnpm --filter @tune/desktop test` — Expected: FAIL.
- [ ] **Step 3: Implement** the app files above.
- [ ] **Step 4: Run** `task js:check && task ui:build` — Expected: PASS, `apps/desktop/dist/index.html` exists.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(desktop): add studio clients and store`; `feat(desktop): add the studio screens`.

### Task 14: Tauri host

**Files:**
- Create: `apps/desktop/src-tauri/{Cargo.toml (package tune-desktop, lib tune_desktop_lib),build.rs,tauri.conf.json,capabilities/default.json,app-icon.svg,icons/*,src/main.rs,src/lib.rs}`
- Modify: root `Cargo.toml` (`members = ["crates/*", "apps/desktop/src-tauri"]`), `.gitignore` (`apps/desktop/src-tauri/gen/`), `Taskfile.yml` (`studio`, `serve`), `apps/desktop/package.json` (`tauri` script)

**Interfaces:**
- Consumes: `Studio`, `StudioConfig`, `StudioHandle`, `Command`, `Event`, `tune_audio::CpalOut`.
- Produces: `pub fn song_arg(args: &[String], env: Option<String>) -> Option<PathBuf>` (`--song <p>` first, then the `TUNE_SONG` value). `#[tauri::command] async fn studio(command: Command, state: State<StudioHandle>) -> Result<serde_json::Value, String>` (`spawn_blocking` on `call`). In setup: `Studio::spawn(StudioConfig { audio: Box::new(CpalOut::new()), songs_root: TUNE_ROOT or cwd }, song_arg(..))`, plus a thread that forwards `subscribe()` events as `app.emit("studio-event", &event)`. `tauri.conf.json`: product "tune", identifier `ai.tune.studio`, window title "tune", 1440×900 (min 1280×800), `frontendDist: "../dist"`, `devUrl: http://localhost:1420`. Capability: `core:default`. Icons come from `pnpm tauri icon src-tauri/app-icon.svg`, where the SVG is a red LED dot on a gunmetal square.
- Taskfile: `studio` runs `cd apps/desktop && TUNE_ROOT={{.ROOT_DIR}} TUNE_SONG={{.ROOT_DIR}}/{{.SONG}} pnpm tauri dev` (needs `SONG=`). `serve` runs `task ui:build`, then `cargo run --release -p tune-cli -- serve {{.SONG}}`.

- [ ] **Step 1: Write the failing test** in `lib.rs`: `song_arg_prefers_flag` (`["tune-desktop","--song","a.toml"]` with env `b.toml` gives `a.toml`), `song_arg_env_fallback`, `song_arg_none`.
- [ ] **Step 2: Run** `cargo test -p tune-desktop` — Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** the crate and config, and generate the icons.
- [ ] **Step 4: Run** `cargo test -p tune-desktop && (cd apps/desktop && pnpm tauri build --debug --no-bundle)` — Expected: PASS, and the build produces `target/debug/tune-desktop`. Manual check: `task studio SONG=songs/first-acid/song.toml` opens the window. Play sounds, and a Cutoff drag is audible.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(desktop): host the studio in a Tauri window`.

### Task 15: End-to-end check with agent-browser, and the docs

**Files:**
- Modify: `CLAUDE.md` (layout: `tune-audio`, `tune-studio`, `apps/desktop`, `packages/ui`; "How Claude uses tune" additions), `README.md` ("Open the studio")

**Interfaces:**
- Consumes: everything above.
- `CLAUDE.md` additions:
  - `tune changes <song> --since <until from last time> --json` is how Claude sees the producer's knob moves and step edits. Start each turn with it when the app may have been open.
  - Use `tune set <song> path=value… --json` for edits while the app is open, so they are tagged `claude` and highlighted. Plain file edits still work and show up as `external`. Include the path grammar table from Task 3.
  - `tune serve <song> --device null --port 4580` plus the agent-browser recipe: start the agent Chrome, pick a session, `open http://127.0.0.1:4580/`, `snapshot`, interact, and `screenshot scratch/<name>.png`. Read the PNG to see the UI. Never commit screenshots.
  - `edits.jsonl` is local and gitignored.
- README "Open the studio": `task install`, `task studio SONG=songs/first-acid/song.toml` (desktop window), or `task serve SONG=…` then open http://127.0.0.1:4580/. Add the keyboard shortcuts.

- [ ] **Step 1: Prepare.** Run `task ui:build`, `mkdir -p scratch/e2e`, and `cp songs/first-acid/song.toml scratch/e2e/song.toml`. Start `cargo run --release -p tune-cli -- serve scratch/e2e/song.toml --device null --port 4580 --json` in the background. Expected: a `serving` line.
- [ ] **Step 2: Drive the page** following the `agent-browser` skill (start the agent Chrome, `session id --prefix tune-e2e`, and `skills get core` for the exact command syntax). Open `http://127.0.0.1:4580/`, take a snapshot, and click "Play". Read the position text twice, 1 s apart. Expected: it advances. Drag the "Cutoff" slider upward about 80 px with the guide's mouse/drag command, then focus it and press ArrowUp 3 times. Expected: `rg -n 'acid303.cutoff' scratch/e2e/song.toml` shows a new value, and `cargo run -q -p tune-cli -- changes scratch/e2e/song.toml --json | jq -r '.changes[-1].source'` prints `human`.
- [ ] **Step 3: Check Claude's side.** Run `cargo run -q -p tune-cli -- set scratch/e2e/song.toml tracks.acid.acid303.resonance=92 --json` (use the first-acid acid track id). Expected: the page shows "Claude changed" with Resonance ringed in cyan. Click "Undo". Expected: the resonance in the file goes back, and the log's last entry is `human`. Take a screenshot to `scratch/e2e/studio.png` and read it. Check the layout against the Visual direction (faceplate is the brightest surface, labels readable, no overlap at 1440×900). Fix what is off in the owning component, then re-run its tests.
- [ ] **Step 4: Stop** the server (Ctrl-C prints `stopped`) and close the agent-browser session. Write the docs.
- [ ] **Step 5: Run** `task verify && git status --short scratch` — Expected: PASS, nothing under `scratch` tracked. Commit: `docs: document the studio, tune set, tune changes and tune serve`.
- [ ] **Step 6: Session gate (producer).** The producer runs `task studio SONG=songs/first-acid/song.toml`, plays, reorders a section, edits a few steps, and twists Cutoff and Resonance. Claude then runs `tune changes` and describes what it sees. Record the producer's verdict and wishes as input for the next plan.
