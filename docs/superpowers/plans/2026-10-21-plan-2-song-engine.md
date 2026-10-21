# Plan 2 — Song format, drums and the live engine

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `songs/<name>/song.toml` file that the `tune` CLI validates, describes, renders (full mix, bar range, stems) and plays in realtime with hot reload. The audio comes from a new headless `tune-engine` with synthesized 909-style drums, a drift-free shared transport, and offline and realtime output that are bitwise identical.

**Architecture:** `tune-dsp` gains drum voices, an acid gate shared by `AcidPlayer` and the engine, index-based parameter setters and `reset`. A new `tune-song` crate parses, validates and canonically serializes the TOML song into a typed `Song`, and watches song files. A new `tune-engine` crate compiles a `Song` into an immutable `CompiledSong` and runs it sample by sample in an `Engine`. Offline rendering and a lock-free `LiveProcessor` (fed by a `LiveController` over SPSC queues; song swaps land on step boundaries) both use that `Engine`. `tune-cli` adds `validate`, `info`, `params`, `render` and `play`, each with `--json`. `cpal` is used only in `tune-cli`.

**Tech Stack:** Rust 2024 (toolchain 1.97). New dependencies: `toml` 0.9 (features `serde`, `preserve_order`), `serde` (derive), `indexmap` 2 (serde), `notify` 8, `rtrb` 0.3, `cpal` (current 0.x), `ctrlc` 3, `serde_json` 1. Already in use: `hound`, `clap`, `thiserror`, and the dev-dependencies `assert_cmd` and `tempfile`.

**Spec:** `docs/superpowers/specs/2026-10-20-tune-ai-design.md`. This plan covers delivery stage 4 of §7 and the engine half of stage 5 (realtime playback and hot reload, no GUI). The edit log, write-back conflict handling, analysis and the GUI belong to later plans. The producer decided on a CLI with `--json` instead of an MCP server, so this plan ships no `tune-mcp` crate.

## Global Constraints

- macOS on Apple Silicon only. The repo stays MIT. No JC303 (GPL) code may be copied.
- Work on branch `feat/song-engine`, cut from `main` after Plan 1 merges. Use atomic conventional commits, one concern each, and run `task verify` before every commit.
- Never commit audio. Song files under `songs/` are committed (the repo is private). Renders go to `scratch/` or `songs/*/renders/`, both gitignored. The privacy check stays green.
- `tune-dsp`, `tune-song` and `tune-engine` stay headless: no Tauri, UI, MCP, `cpal` or `ctrlc` dependency. `cpal` and `ctrlc` are used only in `tune-cli`. `tune-song`, `tune-engine` and `tune-cli` declare `#![forbid(unsafe_code)]`. `tune-dsp` declares `#![deny(unsafe_code)]`, with one local `#[allow(unsafe_code)]` on `acid303::fft::sin_cos_pair`.
- Parity: every Plan 1 parity test stays green with its tolerance unchanged (they report diff 0 today). If a `tune-dsp` refactor changes a parity result, the refactor is wrong. Do not change the test.
- Determinism: the same song, start bar and sample rate give bitwise-identical output, whether rendered offline or in realtime, at any callback block size. All noise is seeded from `[song] seed` (see Task 7 for the derivation). Iterate tracks, patterns and sections in song-file order: `IndexMap`/`Vec`, never a `HashMap`.
- The realtime audio callback must not allocate, deallocate, lock, print or block.
- Timing: 4/4 with 16 steps (16ths) per bar. Step `k` starts at sample `round(k * sample_rate * 15 / bpm)` from song start. Every track fires from this one clock.
- Bar numbering: the engine API uses 0-based bars and half-open ranges. CLI flags and printed output use 1-based bars and inclusive ranges (`--bars 33..64`). Automation points use bar offsets from the section start (`0` = the first sample of the section).
- CLI: every command takes `--json`. Exit code 0 means success and 2 means any failure. JSON goes to stdout. In text mode, errors go to stderr as `error: …` or `<file>:<line>:<col>: …`. Clap usage errors stay clap's plain text (exit 2).
- Song limits: bpm 40–300; sections 1–1024 bars each; total length ≤ 30 min; patterns 1–64 steps; `gain_db` −60…+12; `pan` −1…+1; `sample_rate` 8 000–384 000 (default 48 000).

## Review Focus

1. **Editor saves during playback** (atomic rename, or truncate followed by a chunked write, so the watcher may see a half-written file). Expected: the half file gives at most one `reload_error`, a successful reload follows once the write completes, and playback never stops. Test in Task 11.
2. **A hot reload that removes what the playhead is on** (song shortened below the playhead, a playing track deleted, the loop range now past the end). Expected: no panic. Playback continues at the same step if it still exists, otherwise the loop is clamped or dropped and playback finishes cleanly. Test in Task 10.
3. **Arrangement edge cases**: a track no section plays, a section that plays nothing, odd-length patterns (13, 15) that cycle against the bar and restart at each section start, and a slide on a section's last step into the next section. Expected: deterministic, no panic, and the slide holds the gate only when the next section's first step is a note. Test in Task 8.
4. **Automation edge cases**: a single-point lane, two points at the same bar (a jump), a lane ending before its section ends (hold), a lane on an fx param, a lane on a muted track, and seeking into the middle of a lane. Expected: param values are a pure function of song position, and the next section falls back to the static value. Test in Task 8.
5. **Many mistakes in one song file** (an unknown key typo, an error inside an inline table or `[[sections.automation]]`, a syntax error). Expected: every validation error is reported together with a correct 1-based line:column. Test in Task 5.

---

## File Structure

```
crates/tune-dsp/src/
  lib.rs                   + #![deny(unsafe_code)], pub mod drums
  param.rs                 + ParamSpec::check, find_spec
  acid303/voice.rs         + set_param_at, reset; wake -> pub(crate)
  acid303/pattern.rs       AcidPattern::new (validated), private steps + steps()
  acid303/gate.rs          AcidGate: gate countdown + trigger/slide/tie decision (shared)
  acid303/player.rs        delegates to AcidGate (behaviour unchanged, parity guards it)
  fx/distortion.rs         + set_param_at, reset, DistortionMode::{from_name, name}
  drums/{mod,noise,pattern,kick,clap,hat}.rs   Noise, DrumPattern, Kick, Clap, Hat + param tables
crates/tune-dsp/tests/drums.rs
crates/tune-song/src/
  lib.rs                   parse_song, load_song, re-exports
  model.rs                 Song, Track, Pattern, PatternSteps, Section, Lane, InstrumentKind, FxSpec
  raw.rs                   serde mirror with toml::Spanned fields, Num (int-or-float)
  validate.rs              raw -> model, collects every SongError
  error.rs                 SongError, LoadError, byte offset -> line/column
  write.rs                 Song::to_toml (canonical form)
  watch.rs                 SongWatcher, Reload
crates/tune-song/tests/{parse,roundtrip,watch}.rs, tests/fixtures/mini.toml (shared fixture)
crates/tune-engine/src/
  lib.rs, clock.rs         Clock, track_seed, hit_seed
  instrument.rs            Instrument, Effect, Cell, StepCtx
  tracks.rs                AcidTrack, trait impls for drums + Distortion, make_instrument, make_effect
  compile.rs               CompiledSong, CompiledTrack, CompiledSection, CompiledLane, ParamTarget
  mixer.rs                 pan_gains, soft_clip, Meter
  engine.rs                Engine, TrackRuntime
  offline.rs               RenderOptions, Rendered, render, render_to_files
  wav.rs                   write_wav, WavWriter (atomic tmp+rename), WavFormat
  live.rs                  SongUpdate, live(), LiveController, LiveProcessor
crates/tune-engine/tests/{common/mod.rs,tracks,engine,offline,live,live_no_alloc}.rs
crates/tune-cli/src/
  main.rs                  subcommands; render-303 keeps working
  report.rs                text/JSON output, bar-range parsing, error JSON
  cmd/{validate,info,params,render,play}.rs
  audio.rs                 cpal output stream + `null` device
  render.rs                write_wav delegates to tune_engine::wav
crates/tune-cli/tests/{song_cli,play_cli,songs}.rs
songs/first-acid/song.toml
CLAUDE.md, README.md, Taskfile.yml
```

---

### Task 1: Parameter helpers, index setters, unsafe lint

**Files:**
- Modify: `crates/tune-dsp/src/param.rs`, `acid303/voice.rs` (drop `check_range`), `fx/distortion.rs`, `lib.rs`, `acid303/fft.rs`, `crates/tune-cli/src/render.rs` (`route` uses `find_spec`)
- Test: `param.rs` (unit), `fx/distortion.rs` (unit), `crates/tune-dsp/tests/voice_behaviour.rs`

**Interfaces:**
- Produces:
  ```rust
  impl ParamSpec { pub fn check(&self, plain: f64) -> Result<(), ParamError>; }  // NaN or outside min..=max -> OutOfRange
  pub fn find_spec(table: &'static [ParamSpec], id: &str)
      -> Result<(usize, &'static ParamSpec), ParamError>;                        // UnknownId
  impl Acid303 { pub fn set_param_at(&mut self, index: usize, plain: f64); }      // index into ACID303_PARAMS, plain clamped
  impl Distortion { pub fn set_param_at(&mut self, index: usize, plain: f64); pub fn reset(&mut self); }
  impl DistortionMode { pub fn from_name(s: &str) -> Option<Self>; pub fn name(self) -> &'static str; } // "soft"|"hard"|"overdrive"
  ```
- `set_param(id, plain)` on both becomes `find_spec` + `check` + `set_param_at`. `Distortion::reset` clears the filter states and the dry delay and keeps the params and the mode.

- [ ] **Step 1: Write failing tests.** `check_accepts_bounds_rejects_outside_and_nan`: cutoff spec `check(314.0)` and `check(2394.0)` give Ok; `check(313.9)` and `check(f64::NAN)` give `OutOfRange`. `find_spec_returns_index`: `find_spec(ACID303_PARAMS, "acid303.cutoff")` gives index 2 and id `acid303.cutoff`; `"acid303.nope"` gives `UnknownId`. `set_param_at_matches_set_param`: two voices, one set through `set_param(id, max)` and the other through `set_param_at(i, max)` for every param, the same notes for 1 s, bitwise-equal output. `set_param_at_clamps`: `set_param_at(2, 1e9)` makes `cutoff() == 2394.0`. `reset_restores_fresh_state` (distortion): process 1 s of seeded noise, `reset()`, then a 440 Hz sine bitwise-equals a fresh `Distortion` given the same sine. `mode_names_round_trip`: `from_name(m.name()) == Some(m)` for all three modes, and `from_name("fuzz") == None`.
- [ ] **Step 2: Run** `cargo test -p tune-dsp` — Expected: FAIL (missing methods).
- [ ] **Step 3: Implement** the API above. Add `#![deny(unsafe_code)]` to `lib.rs` and `#[allow(unsafe_code)]` on `sin_cos_pair`. In `render.rs`, look up specs with `find_spec`.
- [ ] **Step 4: Run** `cargo test --workspace && rg -n 'allow\(unsafe_code\)' crates/` — Expected: all pass, exactly one hit (`fft.rs`).
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `refactor(dsp): add ParamSpec::check, find_spec and index setters`; `chore(dsp): deny unsafe code outside the sincos shim`.

### Task 2: Acid303::reset, validated AcidPattern, shared AcidGate

**Files:**
- Create: `crates/tune-dsp/src/acid303/gate.rs`
- Modify: `acid303/voice.rs`, `acid303/pattern.rs`, `acid303/player.rs`, `acid303/mod.rs` (export `AcidGate`), `tests/player_behaviour.rs` and `tests/parity_player.rs` (`.steps` becomes `.steps()`)
- Test: `tests/voice_behaviour.rs`, `pattern.rs` (unit), `tests/parity_player.rs` (unchanged assertions)

**Interfaces:**
- Produces:
  ```rust
  impl Acid303 { pub fn reset(&mut self); }   // DSP state = freshly constructed, params kept, idle = true
  impl AcidPattern {
      pub fn new(steps: Vec<AcidStep>) -> Result<Self, PatternError>;  // the only constructor; parse() uses it
      pub fn steps(&self) -> &[AcidStep];
  }
  pub enum PatternError { /* existing */, BadNote { index: usize, note: u8 } }   // note > 127
  pub struct AcidGate { /* note_off_count_down: i32, slide_to_next_note: bool */ }
  impl AcidGate {
      pub fn new() -> Self;                                   // count down i32::MAX, no slide pending
      pub fn tick(&mut self, voice: &mut Acid303);            // countdown; release_note() when it reaches 0
      pub fn on_step(&mut self, voice: &mut Acid303, step: &AcidStep, next_has_gate: bool, gate_samples: i32);
      pub fn reset(&mut self);
  }
  ```
- `AcidStep`'s fields stay `pub`. `AcidPattern.steps` becomes private. `Acid303::wake` becomes `pub(crate)`.
- `on_step` is the `match note.note { … }` block of `AcidPlayer::tick`, moved as is: trigger or slide, the clip to 0..=127, holding the gate when `slide && next_has_gate`, and releasing on a rest after a pending slide. `AcidPlayer::tick` then runs `gate.tick(voice)`, its own Open303 step timing, and `gate.on_step(voice, step, next_has_gate, self.step_length_in_samples())`, in that order.

- [ ] **Step 1: Write failing tests.** `reset_matches_fresh_voice`: voice A gets `with_param_defaults(48000.0)` plus resonance 85, env mod 60 and accent 100, and plays 1 s of accented and sliding notes; then `A.reset()`. Voice B is fresh with the same params. On both: `trigger_note(36, true)`, `slide_to_note(48, false)` at sample 6000, `release_note()` at 12 000, 48 000 samples. Expected: bitwise equal. `reset_is_silent`: after `reset()`, 4800 samples are all exactly `0.0`. Pattern unit `new_validates`: `new(vec![])` gives `Length(0)`, 65 steps give `Length(65)`, note 128 gives `BadNote { index: 0, note: 128 }`, and `parse` of 65 tokens still gives `Length(65)`.
- [ ] **Step 2: Run** `cargo test -p tune-dsp` — Expected: FAIL.
- [ ] **Step 3: Implement** `reset` (reset every embedded filter, envelope, slew limiter and oscillator phase, clear the note list, `current_note = -1`, `idle = true`; the test decides which members matter), `AcidPattern::new`/`steps`, and `AcidGate`, with `AcidPlayer` refactored onto it.
- [ ] **Step 4: Run** `cargo test -p tune-dsp` — Expected: PASS. `parity_player` passes with its assertions untouched.
- [ ] **Step 5: Run** `task verify`, then commit (three commits): `feat(dsp): add Acid303::reset`; `refactor(dsp): validate AcidPattern on construction`; `refactor(dsp): extract AcidGate from AcidPlayer`.

### Task 3: Seeded noise, drum step notation, kick

**Files:**
- Create: `crates/tune-dsp/src/drums/{mod.rs,noise.rs,pattern.rs,kick.rs}`
- Modify: `crates/tune-dsp/src/lib.rs` (`pub mod drums`)
- Test: `drums/pattern.rs`, `drums/noise.rs` (unit), `crates/tune-dsp/tests/drums.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Noise;  impl Noise { pub fn new(seed: u64) -> Self; pub fn reseed(&mut self, seed: u64); pub fn sample(&mut self) -> f64; } // [-1, 1)
  pub enum DrumHit { Rest, Hit, Accent }
  pub struct DrumPattern;   // 1..=64 hits, private field
  impl DrumPattern {
      pub fn new(hits: Vec<DrumHit>) -> Result<Self, DrumPatternError>;
      pub fn parse(s: &str) -> Result<Self, DrumPatternError>;
      pub fn hits(&self) -> &[DrumHit];
  }
  impl Display for DrumPattern   // canonical: groups of 4 separated by one space, e.g. "x... x... X... x..."
  pub enum DrumPatternError { BadChar { index: usize, ch: char }, Length(usize) }   // index = step index
  pub const ACCENT_GAIN_DB: f64 = 4.0;
  pub const KICK_PARAMS: &[ParamSpec];
  pub struct Kick;
  impl Kick {   // the same method set is reused by Clap and Hat in Task 4
      pub fn new(sample_rate: f64) -> Self;            // KICK_PARAMS defaults applied
      pub fn trigger(&mut self, accent: bool, seed: u64);
      pub fn get_sample(&mut self) -> f64;
      pub fn reset(&mut self);                          // silent until the next trigger
      pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError>;
      pub fn set_param_at(&mut self, index: usize, plain: f64);
  }
  ```
- Noise: xorshift64* (`x ^= x >> 12; x ^= x << 25; x ^= x >> 27; out = x.wrapping_mul(0x2545F4914F6CDD1D)`). A seed of 0 is replaced by `0x9E3779B97F4A7C15`. The output is `(out >> 11) as f64 / 2^53 * 2.0 - 1.0`.
- Notation: `x` is a hit, `X` an accented hit, `.` a rest. Each is one 16th step. Whitespace is ignored.
- `KICK_PARAMS` (id, unit, range, curve, default, help verbatim):
  - `kick.tune` Hz 40–80 lin 50 "Pitch the kick body settles on. Tune it to the track's root note so kick and bassline agree."
  - `kick.decay` ms 100–1500 exp 450 "How long the kick booms. Short is tight and punchy; long turns into a rumbling sub."
  - `kick.punch` % 0–100 lin 50 "How far the pitch drops at the start of each hit. More punch, more thud in the chest."
  - `kick.click` % 0–100 lin 30 "The short tick at the front of the kick that helps it cut through on small speakers."
  - `kick.drive` dB 0–24 lin 6 "Saturates the kick for a harder, rave-style knock. Too much turns the low end muddy."
- Kick algorithm, with `t` the seconds since trigger. A trigger resets the phase to 0 and reseeds the noise.
  - Frequency: `f(t) = tune * (1 + 6 * punch/100 * exp(-t / 0.012))`. Body: `sin(2π·phase)`.
  - Amp envelope: `exp(-t * ln(1000) / decay_s)`, which reaches −60 dB at `decay`.
  - Click: `c * noise * exp(-t / 0.0015)`, where `c = click/100`, multiplied by 1.5 on an accent ("brighter").
  - Output: `x = body * amp + click`; `y = tanh(g·x) / tanh(g)` with `g = db2amp(drive)`; `y * db2amp(ACCENT_GAIN_DB)` on an accent.

- [ ] **Step 1: Write failing tests.** Pattern: `parse("x...x...X... x...")` gives 16 hits with `[0] == Hit`, `[1] == Rest`, `[8] == Accent`; `Display` gives `"x... x... X... x..."`; it round-trips; `"x.o."` gives `BadChar { index: 2, ch: 'o' }`; `""` gives `Length(0)`; 65 `x` give `Length(65)`. Noise: the same seed gives equal sequences and a different seed differs; over 1e6 values all lie in `[-1, 1)`, `|mean| < 0.005` and the RMS is in 0.57..0.58. Kick (click 0 for the pitch tests): `kick_settles_on_tune`, the zero-crossing frequency over 150–250 ms is 50 ± 2 Hz at tune 50 and 80 ± 2 Hz at tune 80. `kick_punch_sweeps_pitch`: zero crossings in (0, 10 ms] number ≥ 3 at punch 50 and ≤ 1 at punch 0. `kick_decay_reaches_minus_60_db`: decay 450, drive 0, max abs over 450–470 ms ≤ 0.002, max abs over the first 50 ms ≥ 0.5. `kick_bounded`: all params at max, then all at min, accented, 20 retriggers 50 ms apart: finite, peak < 2.1. `kick_accent_louder_and_brighter`: the RMS ratio over the first 200 ms (accent/normal) is 4 ± 1 dB, and `brightness(accent) > brightness(normal)`, where `brightness = Σ|x[n]−x[n−1]| / Σ|x[n]|` over 50 ms. `kick_reset_is_silent`: trigger, 100 samples, `reset()`, the next 1000 samples are exactly `0.0`. `kick_params_validate`: `"kick.nope"` gives `UnknownId`, `("kick.tune", 100.0)` gives `OutOfRange`. `kick_seeded`: the same seed gives bitwise-equal output, a different seed differs (click 30).
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test drums && cargo test -p tune-dsp drums::` — Expected: FAIL.
- [ ] **Step 3: Implement** `noise.rs`, `pattern.rs` and `kick.rs` as specified.
- [ ] **Step 4: Run** the same commands — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(dsp): add seeded noise and drum step notation`; `feat(dsp): add 909-style kick`.

### Task 4: Clap and hats

**Files:**
- Create: `crates/tune-dsp/src/drums/{clap.rs,hat.rs}`
- Modify: `drums/mod.rs`
- Test: `crates/tune-dsp/tests/drums.rs`

**Interfaces:**
- Consumes: `Noise`, `ACCENT_GAIN_DB`, `BiquadFilter` (`Bandpass`, `Highpass12`; bandwidth in octaves).
- Produces: `Clap` and `Hat` with the same method set as `Kick`. `pub enum HatKind { Closed, Open }`; `Hat::new(kind: HatKind, sample_rate: f64)`; `Hat::params(&self) -> &'static [ParamSpec]`. Constants `CLAP_PARAMS`, `HAT_CLOSED_PARAMS`, `HAT_OPEN_PARAMS`.
- Param tables (help verbatim):
  - `clap.tone` Hz 600–2400 exp 1200 "Where the clap's noise sits. Low is thick and roomy, high is snappy and thin."
  - `clap.decay` ms 50–800 exp 250 "Length of the clap's tail. Long tails sound like a big room."
  - `clap.spread` ms 4–20 lin 10 "Gap between the hand slaps that make up one clap. Wider sounds looser, more like a crowd."
  - `hat_closed.tone` Hz 4000–12000 exp 7000 "Brightness of the hat. Higher is crisper and thinner."
  - `hat_closed.decay` ms 10–200 exp 45 "How long the hat rings. Keep it short so fast 16ths stay tight."
  - `hat_closed.metal` % 0–100 lin 40 "Blend of metallic ring and plain noise. More metal sounds like a real cymbal."
  - `hat_open.tone` Hz 4000–12000 exp 6500 "Brightness of the open hat. Higher is crisper and thinner."
  - `hat_open.decay` ms 100–1500 exp 400 "How long the open hat hisses. Long tails fill the gap between kicks."
  - `hat_open.metal` % 0–100 lin 50 "Blend of metallic ring and plain noise. More metal sounds like a real cymbal."
- Clap: noise goes through a bandpass at `tone` with a 1-octave bandwidth. The envelope is three slaps with onsets 0, s and 2s (s = spread), each falling 60 dB over s, plus a tail at level 0.6 from onset 2s that falls 60 dB over `decay`. The sum is capped at 1. Pick a fixed makeup gain so that the default peak satisfies the level test.
- Hat: a metal bank of six naive square oscillators at 205.3, 304.4, 369.6, 522.7, 540.0 and 800.0 Hz, averaged, with phases reset on trigger. The source is mixed by `metal`: `metal·bank + (1−metal)·noise`. It goes through `Highpass12` at `tone` (capped at 0.45·sr), then an envelope falling 60 dB over `decay`, then a fixed makeup gain.
- Accent on clap and hats: +`ACCENT_GAIN_DB`, and `tone` × 1.25 ("brighter").

- [ ] **Step 1: Write failing tests.** `clap_has_three_slaps`: with spread 10 ms, for b = 1 and 2 the RMS over [b·s, b·s+2 ms] is > 2× the RMS over [b·s−2 ms, b·s]. `clap_tail_follows_decay`: with decay 250, the RMS over 270–290 ms is < 0.01× the RMS over the first 30 ms. `open_hat_rings_longer`: energy after 150 ms, open > 10× closed (defaults). `brightness_order`: at defaults, `brightness(hat_closed) > brightness(clap) > brightness(kick)`. `drum_levels`: the default normal-hit peak of clap, closed hat and open hat each lies within 0.2..=1.0. For clap and both hats: accent louder (RMS +4 ± 1.5 dB) and brighter, seeded determinism, `reset` silence, param errors, and every param at max and at min with 20 accented retriggers staying finite with peak < 4.0.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test drums` — Expected: FAIL.
- [ ] **Step 3: Implement** `clap.rs` and `hat.rs`.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(dsp): add 909-style clap`; `feat(dsp): add closed and open hats`.

### Task 5: tune-song — model, parser, validation

**Files:**
- Create: `crates/tune-song/{Cargo.toml,src/lib.rs,src/model.rs,src/raw.rs,src/validate.rs,src/error.rs}`, `crates/tune-song/tests/parse.rs`, `crates/tune-song/tests/fixtures/mini.toml` (the shared fixture; engine and CLI tests read it as `concat!(env!("CARGO_MANIFEST_DIR"), "/../tune-song/tests/fixtures/mini.toml")`)
- Test: `crates/tune-song/tests/parse.rs`

**Interfaces:**
- Consumes: `ACID303_PARAMS`, `DIST_PARAMS`, `KICK_PARAMS`, `CLAP_PARAMS`, `HAT_*_PARAMS`, `find_spec`, `ParamSpec::check`, `AcidPattern::parse`, `DrumPattern::parse`, `DistortionMode::from_name`.
- Produces (all derive `Clone, Debug, PartialEq`):
  ```rust
  pub struct Song { pub name: String, pub bpm: f64, pub seed: u64, pub sample_rate: Option<u32>,
      pub tracks: IndexMap<String, Track>, pub patterns: IndexMap<String, Pattern>, pub sections: Vec<Section> }
  pub enum InstrumentKind { Acid303, Kick, Clap, HatClosed, HatOpen }
  impl InstrumentKind { pub fn name(self) -> &'static str; pub fn from_name(s: &str) -> Option<Self>;
      pub fn params(self) -> &'static [ParamSpec]; pub fn is_drum(self) -> bool; }
  pub struct Track { pub instrument: InstrumentKind, pub params: IndexMap<String, f64>, // explicit plain values only
      pub gain_db: f64, pub pan: f64, pub mute: bool, pub solo: bool, pub fx: Vec<FxSpec> }
  pub enum FxSpec { Distortion { mode: DistortionMode, params: IndexMap<String, f64> } }
  impl FxSpec { pub fn type_name(&self) -> &'static str; pub fn params_table(&self) -> &'static [ParamSpec]; }
  pub struct Pattern { pub track: String, pub steps: PatternSteps }
  pub enum PatternSteps { Acid(AcidPattern), Drum(DrumPattern) }
  pub struct Section { pub name: String, pub bars: u32, pub play: IndexMap<String, String>, pub automation: Vec<Lane> }
  pub struct Lane { pub track: String, pub param: String, pub points: Vec<(f64, f64)> } // (bar offset, normalized)
  impl Song { pub fn total_bars(&self) -> u32; pub fn section_start_bars(&self) -> Vec<u32>; } // 0-based
  pub struct SongError { pub line: Option<usize>, pub column: Option<usize>, pub message: String } // 1-based
  pub enum LoadError { Io { path: PathBuf, source: std::io::Error }, Invalid(Vec<SongError>) }
  pub fn parse_song(src: &str) -> Result<Song, Vec<SongError>>;
  pub fn load_song(path: &Path) -> Result<Song, LoadError>;
  ```
- File shape (binding):
  - `[song]` has `name`, `bpm`, optional `seed` (default 0) and optional `sample_rate`.
  - `[tracks.<id>]` has `instrument`, `params = { "<param id>" = plain }`, `gain_db` (0), `pan` (0), `mute` (false), `solo` (false), and `fx = [{ type = "distortion", mode = "soft", params = {…} }]` (mode defaults to soft).
  - `[patterns.<id>]` has `track` and `steps`. `steps` is Plan 1 acid notation for an acid303 track and drum notation otherwise.
  - `[[sections]]` has `name`, `bars` and `play = { <track> = "<pattern>" }`; an absent track is silent. Each `[[sections.automation]]` has `param = "<track id>.<param id>"` and `points = [[bar, normalized], …]`.
- Rules:
  - `deny_unknown_fields` on every table.
  - Every number accepts an integer or a float (`raw::Num`).
  - Track and pattern ids match `[a-z0-9_]+`.
  - Every validation error is collected, ordered by position. Positions come from `toml::Spanned` byte offsets, converted to line and column (the column counts chars).
  - Syntax errors take their span from `toml::de::Error::span()`.
  - Automation `param` splits at the first `.`. It resolves against the instrument table first and then each fx table, and a track may have at most one fx of each type.
  - Lane points: bar offsets lie in `0..=bars` and never decrease (two equal bars form a jump), normalized values lie in `0..=1`, and there is at most one lane per param per section.
- Messages contain these needles: ``unknown track `X` ``, ``unknown pattern `X` ``, ``pattern `P` belongs to track `T` ``, ``unknown parameter `X` for kick``, `is out of range`, ``unknown field `gian_db` ``, ``unknown instrument `X` ``, `bad step`, `needs 1 to 64 steps`, `bars must be 1 to 1024`, `bpm`, `outside the section`, `go backwards`, `outside 0..=1`, `only one distortion`, `may only contain a-z, 0-9 and _`, `longer than 30 minutes`, `at least one section`.
- `mini.toml` fixture: 120 BPM, seed 7, three tracks in this order:
  - `kick` (kick, gain −3).
  - `acid` (acid303, params cutoff 500, resonance 70, env_mod 50, decay 400, accent 55; fx soft distortion with `dist.drive` 12).
  - `hat` (hat_closed, gain −10, pan 0.3).

  Patterns `k4 = "x...x...x...X..."`, `a1 = "C2 C2! C3~ C2 . D#2 C2~ G2!"`, `h = "..x...x...x...x."`. Section `a` (2 bars) plays kick and hat. Section `b` (2 bars) plays all three, with the lane `acid.acid303.cutoff` and points `[[0, 0.1], [2, 0.8]]`.

- [ ] **Step 1: Write failing tests.** `parses_mini`: the fields above, track order `[kick, acid, hat]`, `total_bars() == 4`, `section_start_bars() == [0, 2]`. `integers_and_floats_accepted`: `bpm = 145` and `bpm = 145.0` give equal songs. `reports_all_errors_with_positions` (**Review Focus 5**): one source with `gian_db` on line 6, an out-of-range `"kick.tune" = 100` inside an inline `params` table on line 7, ``play = { kick = "nope" }`` on line 14 and a lane point `[3, 0.5]` in a 2-bar section on line 18. Expected: exactly 4 errors in line order with lines 6, 7, 14 and 18, and the column of the offending value for the line 7 and line 14 errors. `syntax_error_has_position`: `"[song\nname = 1"` gives one error with `line == Some(1)`. `validation_rules`: a table of (mutation of mini, needle), one row per needle above; each row expects `Err` whose messages contain the needle. `load_song_io_error`: a missing path gives `LoadError::Io` whose `path` is the missing path.
- [ ] **Step 2: Run** `cargo test -p tune-song` — Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** the crate: a serde mirror in `raw.rs`, the checks in `validate.rs`, AcidPattern and DrumPattern built only through `parse` (that is, `new`). Add the crate to the workspace (the glob `crates/*` already picks it up).
- [ ] **Step 4: Run** `cargo test -p tune-song` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(song): parse and validate TOML song files`.

### Task 6: Canonical serializer

**Files:**
- Create: `crates/tune-song/src/write.rs`
- Test: `crates/tune-song/tests/roundtrip.rs`

**Interfaces:**
- Produces: `impl Song { pub fn to_toml(&self) -> String; }`. Strings are quoted through `toml::Value::String(..).to_string()`. Numbers are written with Rust's shortest round-trip `Display` (`145`, `0.15`), which the parser accepts as int-or-float. Comments are not preserved (see Decision in the report). Plan 3 does in-place edits.
- Canonical layout (exact; `sample_rate` is written only when it is `Some`, `fx` only when non-empty, params only when explicit, in model order):
  ```toml
  [song]
  name = "mini"
  bpm = 120
  seed = 7

  [tracks.acid]
  instrument = "acid303"
  gain_db = 0
  pan = 0
  mute = false
  solo = false
  params = { "acid303.cutoff" = 500 }
  fx = [{ type = "distortion", mode = "soft", params = { "dist.drive" = 12 } }]

  [patterns.a1]
  track = "acid"
  steps = "C2 C2! C3~ ."

  [[sections]]
  name = "a"
  bars = 2
  play = { acid = "a1" }

  [[sections.automation]]
  param = "acid.acid303.cutoff"
  points = [[0, 0.1], [2, 0.8]]
  ```
  Drum steps are written in `DrumPattern`'s `Display` form (`"x... x... X... x..."`).

- [ ] **Step 1: Write failing tests.** `canonical_layout`: parsing the block above and serializing gives exactly that text. `round_trip_mini`: `parse(to_toml(parse(mini))) == parse(mini)`. `canonical_is_stable`: `to_toml` is idempotent over a second round. `numbers_round_trip_exactly`: params 0.1, 1.0/3.0, 1e-7 and 2394.0 come back bitwise equal. `strings_escaped`: the name `Acid "Mk2" ü \ end` round-trips.
- [ ] **Step 2: Run** `cargo test -p tune-song --test roundtrip` — Expected: FAIL.
- [ ] **Step 3: Implement** `write.rs` (a hand-written writer, so the layout is fixed).
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(song): add canonical TOML serializer`.

### Task 7: tune-engine — clock, traits, track wrappers

**Files:**
- Create: `crates/tune-engine/{Cargo.toml,src/lib.rs,src/clock.rs,src/instrument.rs,src/tracks.rs}`
- Test: `clock.rs` (unit), `crates/tune-engine/tests/tracks.rs`

**Interfaces:**
- Consumes: the `tune-dsp` voices, `AcidGate`, `Distortion`, and `tune_song::{InstrumentKind, FxSpec}`.
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Clock { pub sample_rate: u32, pub bpm: f64, pub samples_per_step: f64 }  // sr * 15 / bpm
  impl Clock { pub fn new(bpm: f64, sample_rate: u32) -> Self;
      pub fn step_start(&self, step: u64) -> u64;      // (step as f64 * sps).round() as u64
      pub fn step_at(&self, pos: u64) -> u64;          // largest k with step_start(k) <= pos
      pub fn bar_start(&self, bar: u32) -> u64;        // step_start(16 * bar)
      pub fn gate_samples(&self) -> i32;               // round_to_int(0.5 * sps)
      pub fn seconds(&self, pos: u64) -> f64; }
  pub fn track_seed(song_seed: u64, track_id: &str) -> u64;   // splitmix64(song_seed ^ fnv1a64(id))
  pub fn hit_seed(track_seed: u64, step: u64) -> u64;         // splitmix64(track_seed ^ step)
  pub enum Cell { Rest, Hit { accent: bool }, Note(AcidStep) }
  pub struct StepCtx { pub step: u64, pub next_has_gate: bool, pub gate_samples: i32, pub seed: u64 }
  pub trait Instrument: Send {
      fn params(&self) -> &'static [ParamSpec];
      fn set_param_at(&mut self, index: usize, plain: f64);
      fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> { /* find_spec + check + set_param_at */ }
      fn on_step(&mut self, cell: &Cell, ctx: &StepCtx);   // called before process() in the same sample
      fn process(&mut self) -> f64;                         // one mono sample
      fn reset(&mut self);
  }
  pub trait Effect: Send { fn params(&self) -> &'static [ParamSpec]; fn set_param_at(&mut self, index: usize, plain: f64);
      fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> { /* provided */ }
      fn process(&mut self, x: f64) -> f64; fn reset(&mut self); }
  pub struct AcidTrack { /* Acid303::with_param_defaults, AcidGate, pending step */ }
  pub fn make_instrument(kind: InstrumentKind, sample_rate: f64) -> Box<dyn Instrument>;
  pub fn make_effect(fx: &FxSpec, sample_rate: f64) -> Box<dyn Effect>;   // mode set; params left at defaults
  ```
- `splitmix64` and `fnv1a64` are the standard algorithms (FNV offset `0xcbf29ce484222325`, prime `0x100000001b3`). Pinned values: `track_seed(7, "kick") == 0x33d8839d0e2386ea` and `hit_seed(0x33d8839d0e2386ea, 16) == 0x08653e8277ee190e`.
- `AcidTrack::process` keeps `AcidPlayer`'s order: `gate.tick(voice)`, then any pending `gate.on_step(voice, step, ctx.next_has_gate, ctx.gate_samples)`, then `voice.get_sample()`. `Cell::Hit` and `Cell::Rest` both act as a rest. Drum wrappers call `trigger(accent, ctx.seed)` on a `Hit`, ignore the other cells, and implement `Instrument` directly for `Kick`, `Clap` and `Hat`. `Distortion` implements `Effect`.

- [ ] **Step 1: Write failing tests.** Clock: at 145 BPM and 48 kHz, `step_start(1) == 4966`, `step_start(16) == 79448`, `bar_start(64) == 5084690`, `step_at(4965) == 0`, `step_at(4966) == 1`. At 120 BPM, `step_start(k) == 6000 * k` for k < 10 000 and `gate_samples() == 3000`. Seeds: the pinned values, and different ids or steps give different seeds. `acid_track_gate_closes_half_step` (120 BPM, cells driven at `step_start(k)`, pattern `C2 .`): RMS over [3600, 6000) < 0.01 × RMS over [0, 3000). `acid_track_tie_holds_gate` (`C2~ C2`): RMS over [3600, 6000) > 0.3 × RMS over [0, 3000). `drum_track_hits_only`: kick wrapper fed `Rest` cells outputs exactly 0.0, fed a `Hit` it outputs nonzero. `set_param_by_id`: `make_instrument(Kick).set_param("kick.tune", 60.0)` is Ok and `"acid303.cutoff"` gives `UnknownId`. `params_tables`: `make_instrument(k).params()` ids equal `k.params()` ids for all five kinds.
- [ ] **Step 2: Run** `cargo test -p tune-engine` — Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** the crate skeleton (deps: `tune-dsp`, `tune-song`, `thiserror`) and the files above.
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(engine): add transport clock and instrument traits`.

### Task 8: Compiled song and the Engine

**Files:**
- Create: `crates/tune-engine/src/{compile.rs,mixer.rs,engine.rs}`, `crates/tune-engine/tests/common/mod.rs` (helper `compiled(src: &str) -> Arc<CompiledSong>`, plus a `Probe` instrument that records every `on_step` as (sample index, cell, ctx) and every `set_param_at` as (sample index, param index, plain), with the sample index taken from its own `process` count)
- Test: `mixer.rs` (unit), `crates/tune-engine/tests/engine.rs`

**Interfaces:**
- Consumes: Task 5 `Song`, Task 7 `Clock`, `Cell`, `Instrument`, `Effect`, `make_*`, and the seeds.
- Produces:
  ```rust
  pub struct CompiledSong { pub name: String, pub clock: Clock, pub total_bars: u32,
      pub tracks: Vec<CompiledTrack>, pub patterns: Vec<Vec<Cell>>, pub sections: Vec<CompiledSection> }
  impl CompiledSong { pub fn new(song: &Song, sample_rate: u32) -> Self;   // song must be validated; infallible
      pub fn track_index(&self, id: &str) -> Option<usize>; pub fn end_sample(&self) -> u64; }
  pub struct CompiledTrack { pub id: String, pub kind: InstrumentKind, pub params: Vec<f64>, // full table, plain, defaults filled
      pub fx: Vec<CompiledFx>, pub gain: f64, pub pan_gains: (f64, f64), pub mute: bool, pub solo: bool, pub seed: u64 }
  pub struct CompiledFx { pub spec: FxSpec, pub params: Vec<f64> }
  pub struct CompiledSection { pub name: String, pub start_bar: u32, pub bars: u32,
      pub play: Vec<Option<usize>>, pub lanes: Vec<CompiledLane> }   // play is indexed by track
  pub struct CompiledLane { pub track: usize, pub target: ParamTarget, pub spec: &'static ParamSpec, pub points: Vec<(f64, f64)> }
  pub enum ParamTarget { Instrument(usize), Fx { slot: usize, index: usize } }
  pub struct TrackRuntime { pub instrument: Box<dyn Instrument>, pub fx: Vec<Box<dyn Effect>> }
  impl TrackRuntime { pub fn build(track: &CompiledTrack, sample_rate: f64) -> Self; }
  pub struct Meter { pub peak: f64, pub pre_clip_peak: f64, pub knee_samples: u64 }
  pub const CLIP_KNEE: f64;      // db2amp(-6.0) = 0.501187…
  pub const CLIP_CEILING: f64;   // db2amp(-1.0) = 0.891251…
  pub fn soft_clip(x: f64) -> f64;   // |x| <= K: x; else sign(x) * (K + (C-K) * tanh((|x|-K)/(C-K)))
  pub fn pan_gains(pan: f64) -> (f64, f64);   // θ = (pan+1)·π/4 -> (cos θ, sin θ)
  pub struct Engine { /* song, runtimes, pos, step, section, loop, meter, pending swap */ }
  impl Engine {
      pub fn new(song: Arc<CompiledSong>) -> Self;
      pub fn from_parts(song: Arc<CompiledSong>, tracks: Vec<TrackRuntime>) -> Self;
      pub fn seek(&mut self, bar: u32);                 // clamps to total_bars; resets every instrument and effect
      pub fn set_loop(&mut self, bars: Option<Range<u32>>);   // clamped to the song; empty range = None
      pub fn process(&mut self, out: &mut [f32]);       // interleaved stereo
      pub fn process_stems(&mut self, mix: &mut [f32], stems: &mut [&mut [f32]]);  // one stereo slice per track
      pub fn position(&self) -> u64; pub fn bar(&self) -> u32; pub fn finished(&self) -> bool;
      pub fn meter(&self) -> Meter; pub fn reset_meter(&mut self); pub fn song(&self) -> &Arc<CompiledSong>;
  }
  ```
- Per-sample order (`pos` = song sample position, `step` = next step to fire). Determinism depends on this order:
  1. If finished, write 0.0 and return.
  2. If `pos == clock.step_start(step)`:
     - If `step == 16·loop.end`, set `step = 16·loop.start` and `pos = step_start(step)`. Nothing is reset.
     - If `step == 16·total_bars`, set finished and write 0.0.
     - If the section of `step` differs from the current one (or this is the first step after `new`, `seek` or a swap), set every track's instrument and fx params to their static values.
     - For each track in order, call `on_step(cell_at(t, step), StepCtx { step, next_has_gate: cell_at(t, step+1) is a Note, gate_samples, seed: hit_seed(track.seed, step) })`. `cell_at` returns `Rest` when the section does not play the track or when the step lies past the end. Otherwise it returns `pattern[(step − section_start_step) % len]`, so patterns restart at each section.
     - Then `step += 1`.
  3. For each lane of the current section: `x = (pos − section_start_sample) / (16·sps)`. `norm` holds the first point's value before the first point, interpolates linearly between points, holds the last point's value after it, and at equal bars takes the later point. Call `set_param_at(index, spec.to_plain(norm))` on the target.
  4. For each track: `y = instrument.process()`, then each fx in order. `(l, r) = (y·gain·pl, y·gain·pr)`. Write the stem. Add to the master when audible, where audible means `!mute && (no track solos || solo)`.
  5. Update the meter (pre-clip and post-clip peak in f64, plus the count of samples above the knee), then output `soft_clip(master)` as f32 and increment `pos`.
- Muted tracks are still processed, so their state and stems continue.

- [ ] **Step 1: Write failing tests** (unit `mixer.rs` and `engine.rs`):
  - Mixer: `soft_clip(0.25) == 0.25`; `soft_clip(1e6) <= CLIP_CEILING`; monotonic over a 1e4-point grid in −10..10; `|soft_clip(K+1e-9) − (K+1e-9)| < 1e-12`. Pan: `pan_gains(0.0) == (0.7071067811865476, 0.7071067811865476)`, `pan_gains(1.0).0 < 1e-15`, and `gl² + gr² = 1 ± 1e-12` for pans −1..1 in steps of 0.1.
  - `steps_fire_on_drift_free_clock`: a 145 BPM song with one probe track over 64 bars; probe step k fires at `clock.step_start(k)` for all k < 1024.
  - `odd_length_pattern_restarts_each_section` (**Review Focus 3**): a 13-step pattern across two 1-bar sections; the probe sees pattern index 0 at steps 0, 13 and 16.
  - `slide_across_section_boundary` (**Review Focus 3**): step 15 has a slide. `ctx.next_has_gate == true` when section 2 starts with a note on this track and `false` when section 2 does not play it.
  - `empty_section_and_unplayed_track`: a section with an empty `play` and a track no section plays render without panic; the output in the empty section is the decaying tail only (last-bar RMS < 1e-4).
  - `automation_pure_function_of_position` (**Review Focus 4**, probe on an acid303-kind track with static `"acid303.cutoff" = 500`, cutoff index 2): the lane `[[0, 0.0], [1, 1.0]]` in a 2-bar section gives a plain value at half a bar equal to `ACID303_PARAMS[2].to_plain(0.5)` within 1e-9, and 2394.0 after bar 1. A single-point lane `[[0.5, 0.3]]` holds `to_plain(0.3)` throughout. `[[1, 0.2], [1, 0.9]]` jumps at bar 1 to `to_plain(0.9)`. The next section without a lane sees the static value 500.0 at its first sample. `seek(1)` gives the same value at the same position as playing through, bitwise.
  - `fx_lane_and_muted_track`: a lane `acid.dist.drive` resolves to `ParamTarget::Fx { slot: 0, index: 0 }`, and a lane on a muted track still applies (the probe records it).
  - `mute_and_solo`: soloing `hat` in mini makes the mix bitwise equal to the hat stem (the hat stays below the clipper knee). Muting `kick` makes the mix bitwise equal to the mix of mini with the kick track removed.
  - `loop_wraps_without_reset` (mini, 120 BPM, bar = 96 000 samples): with `set_loop(Some(0..1))`, `position() == 48000` after 144 000 frames.
  - `finished_outputs_silence`: after `end_sample()` the engine outputs zeros and `finished()` is true.
- [ ] **Step 2: Run** `cargo test -p tune-engine` — Expected: FAIL.
- [ ] **Step 3: Implement** `compile.rs`, `mixer.rs` and `engine.rs` (section lookup by binary search on start steps; no allocation inside `process`).
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(engine): compile songs into a playable arrangement`; `feat(engine): add engine with automation and mixer`.

### Task 9: Offline render, stems, WAV

**Files:**
- Create: `crates/tune-engine/src/{offline.rs,wav.rs}`
- Modify: `crates/tune-engine/Cargo.toml` (`hound`), `crates/tune-cli/Cargo.toml` (`tune-engine`), `crates/tune-cli/src/render.rs` (`write_wav` becomes a thin wrapper over `tune_engine::wav::write_wav`; its tests stay unchanged)
- Test: `crates/tune-engine/tests/offline.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct RenderOptions { pub start_bar: u32, pub end_bar: u32, pub stems: bool }   // 0-based, half-open
  pub struct Rendered { pub sample_rate: u32, pub mix: Vec<f32>, pub stems: Vec<(String, Vec<f32>)>, pub meter: Meter }
  pub struct RenderReport { pub files: Vec<PathBuf>, pub frames: u64, pub seconds: f64, pub meter: Meter }
  pub enum RenderError { BadRange { start: u32, end: u32, total: u32 }, Wav(WavError) }
  pub fn render(song: Arc<CompiledSong>, opts: &RenderOptions) -> Result<Rendered, RenderError>;
  pub fn render_to_files(song: Arc<CompiledSong>, opts: &RenderOptions, out: &Path) -> Result<RenderReport, RenderError>;
  pub enum WavFormat { Pcm24, Float32 }
  pub fn write_wav(path: &Path, interleaved: &[f32], channels: u16, sample_rate: u32, format: WavFormat) -> Result<(), WavError>;
  pub struct WavWriter;  // create(path, channels, sr, format) -> writes <path>.tmp; write(&[f32]); finish() renames; Drop without finish removes the tmp
  ```
- `render` is `Engine::new` + `seek(start_bar)` + `process_stems` in 4096-frame chunks, run until `bar_start(end_bar)`. `render_to_files` streams the same chunks. With `stems == false`, `out` is the mix file (24-bit PCM stereo). With `stems == true`, `out` is a directory receiving `mix.wav` (24-bit) and `<track id>.wav` (32-bit float, so values above ±1 are kept). Stems ignore mute and solo and skip the clipper.

- [ ] **Step 1: Write failing tests.** On mini: a full render has `mix.len() == 2 * 384000`; bars 1..3 give `2 * 192000`; with `stems`, the names are `["kick", "acid", "hat"]` and each stem's length equals the mix length; with `kick` muted the kick stem is nonzero and the mix bitwise equals the mix of mini without the kick track. `render_is_deterministic`: two renders are bitwise equal. `block_size_independent`: `Engine::process` with block sizes cycling `[1, 7, 64, 333, 4096]` equals `render` bitwise, meter included. `hot_mix_is_clipped_not_broken`: every gain at +12 dB gives finite output, `meter.pre_clip_peak > 1.0` and `meter.peak <= CLIP_CEILING`. `bad_range`: `3..2` and `0..5` on a 4-bar song give `BadRange`. `files_written`: in a tempdir, the mix WAV is 2 channels, 24 bits, 48 kHz and 384 000 frames; with `stems`, the dir holds `mix.wav kick.wav acid.wav hat.wav` and the stems are float32. `no_tmp_on_error`: when `out`'s parent is a file, the result is `Err` and no `.tmp` is left. `cargo test -p tune-cli` stays green.
- [ ] **Step 2: Run** `cargo test -p tune-engine --test offline` — Expected: FAIL.
- [ ] **Step 3: Implement** `offline.rs` and `wav.rs`, and switch `render.rs` over.
- [ ] **Step 4: Run** `cargo test -p tune-engine -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(engine): add atomic WAV writer`; `feat(engine): render songs, bar ranges and stems offline`.

### Task 10: Live engine — lock-free swap, seek, loop

**Files:**
- Create: `crates/tune-engine/src/live.rs`, `crates/tune-engine/tests/{live.rs,live_no_alloc.rs}`
- Modify: `engine.rs` (pending swap applied at the next step boundary), `Cargo.toml` (`rtrb`)

**Interfaces:**
- Produces:
  ```rust
  pub struct SongUpdate { /* song: Arc<CompiledSong>, tracks: Vec<Option<TrackRuntime>> (fresh, one per track) */ }
  impl SongUpdate { pub fn new(song: Arc<CompiledSong>) -> Box<Self>; }   // allocates; call off the audio thread
  pub fn live(song: Arc<CompiledSong>, start_bar: u32, looping: Option<Range<u32>>) -> (LiveController, LiveProcessor);
  pub enum LiveError { QueueFull, SampleRateMismatch { song: u32, engine: u32 } }
  impl LiveController {
      pub fn swap(&mut self, song: Arc<CompiledSong>) -> Result<(), LiveError>;
      pub fn seek(&mut self, bar: u32) -> Result<(), LiveError>;
      pub fn set_loop(&mut self, bars: Option<Range<u32>>) -> Result<(), LiveError>;
      pub fn position(&self) -> u64; pub fn bar(&self) -> u32; pub fn finished(&self) -> bool;
      pub fn swaps_applied(&self) -> u64;
      pub fn collect_garbage(&mut self) -> usize;   // drops returned updates; returns how many
  }
  impl LiveProcessor { pub fn process(&mut self, out: &mut [f32]); pub fn sample_rate(&self) -> u32; }  // Send
  ```
- Transport: two `rtrb` SPSC queues. Commands (capacity 16) go controller → processor. Garbage (capacity 16) goes back as `Box<SongUpdate>`, reusing the same box. The processor publishes position, bar, finished and swaps_applied through atomics after each `process` call. Seek and loop apply at the next sample. A swap is popped only when the garbage queue has room. Otherwise it waits in the queue until a later block.
- Swap at the next step boundary `k`:
  - For each new track, take the old runtime with the same id and kind (the instrument keeps its state); keep the old fx chain if its fx types match, else use the fresh one.
  - Set the new clock; `pos = new_clock.step_start(k)`, which keeps the step index across a bpm change.
  - Re-apply static params.
  - Clamp the loop's end to the new song's end, and drop the loop if its start lies past the end.
  - If `k` is past the end: jump to the loop start if a loop remains, else finish.
  - Push the old song `Arc`, the old runtimes and the unused fresh runtimes back in the same box.

- [ ] **Step 1: Write failing tests** (`live.rs`, all bitwise, songs built from mini variants):
  - `live_matches_offline_any_block_size`: the processor run with block sizes cycling `[1, 7, 64, 333, 512, 4096]` equals `render(0..4)`.
  - `swap_at_next_step_keeps_playhead`: song B = A with `kick` muted, swapped after 1000 frames. Live frames `[0, 6000)` equal render(A), frames `[6000, end)` equal render(B), and `position()` equals frames processed.
  - `swap_new_bpm_keeps_step`: A at 120 BPM, B at 150 BPM, swapped after 1000 frames. After 6100 frames `position() == 4800 + 100`.
  - `swap_removing_track`: B lacks `hat`. No panic, and the output after the boundary equals render(B).
  - `swap_shorter_song` (**Review Focus 2**): playing at bar 3 (0-based) of a 4-bar A, swap B of 2 bars. Without a loop: `finished()` at the boundary, zeros afterwards. With loop 0..4: the loop is clamped to 0..2 and the position jumps to 0 at the boundary.
  - `seek_and_loop_commands`: `seek(2)` makes the next block start at `bar_start(2)`. `set_loop(Some(0..1))` wraps.
  - `garbage_returns`: after an applied swap, `collect_garbage() == 1`.
  - `queue_full`: 17 swaps without processing make the 17th return `QueueFull`.

  `live_no_alloc.rs`: a counting `#[global_allocator]` with a thread-local "audio" flag counts allocations and deallocations on the processing thread across blocks that include a swap, a seek, a loop wrap and the song end. Expected: 0.
- [ ] **Step 2: Run** `cargo test -p tune-engine --test live --test live_no_alloc` — Expected: FAIL.
- [ ] **Step 3: Implement** `live.rs` and the boundary swap in `engine.rs`.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(engine): add lock-free live engine with song swap`.

### Task 11: Song file watcher

**Files:**
- Create: `crates/tune-song/src/watch.rs`, `crates/tune-song/tests/watch.rs`
- Modify: `crates/tune-song/Cargo.toml` (`notify`)

**Interfaces:**
- Produces:
  ```rust
  pub enum Reload { Ok(Song), Invalid(Vec<SongError>), Io(String) }
  pub struct SongWatcher { /* notify watcher + debounce thread; stops on drop */ }
  impl SongWatcher { pub fn start(path: &Path, tx: std::sync::mpsc::Sender<Reload>) -> Result<Self, notify::Error>; }
  ```
- Behaviour:
  - Watch the parent directory non-recursively, since editors save by renaming, and filter events to the file's name.
  - Debounce: wait for 75 ms of quiet, then read the file.
  - If the file is missing (mid-rename), wait for the next event. Any other IO error sends `Io`.
  - Skip a read whose bytes equal the last bytes sent. Otherwise parse and send.

- [ ] **Step 1: Write failing tests** (tempdir, each waiting up to 5 s for an event). `valid_edit_reports_ok`: rewriting `mini` with `bpm = 121` sends `Ok` with that bpm. `atomic_rename_save`: writing `song.toml.tmp` and renaming it over the file sends `Ok`. `partial_write_then_complete` (**Review Focus 1**): truncate, write half the bytes, sleep 200 ms, write the rest. The last event is `Ok` with the full content, and at most one `Invalid` comes before it. `identical_rewrite_ignored`: rewriting the same bytes sends no event within 1 s. `broken_toml_reports_invalid`: `"[song\n"` sends `Invalid` with line 1.
- [ ] **Step 2: Run** `cargo test -p tune-song --test watch` — Expected: FAIL.
- [ ] **Step 3: Implement** `watch.rs`.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(song): watch song files for changes`.

### Task 12: CLI — validate, info, params, render

**Files:**
- Create: `crates/tune-cli/src/report.rs`, `crates/tune-cli/src/cmd/{mod.rs,validate.rs,info.rs,params.rs,render.rs}`, `crates/tune-cli/tests/song_cli.rs`
- Modify: `crates/tune-cli/src/main.rs` (subcommands, and `--json` on `render-303`), `crates/tune-cli/Cargo.toml` (`tune-song`, `serde`, `serde_json`), `crates/tune-cli/src/lib.rs`

**Interfaces:**
- Consumes: `load_song`, `CompiledSong::new`, `render_to_files`, `Clock`, every param table.
- Produces:
  - `tune validate <SONG> [--json]`. Text: `ok: mini, 2 sections, 4 bars, 0:08.0 at 120 BPM`. JSON: `{"ok":true,"name","bpm","bars","seconds","sections","tracks"}`.
  - `tune info <SONG> [--json]`. Text: a header line, then one row per section (1-based start and end bar, start time `m:ss.s`, tracks played, automated params) and one row per track. JSON: `{"ok":true,"name","bpm","sample_rate","bars","seconds","sections":[{"name","bars","start_bar","end_bar","start_seconds","play":{…},"automation":["acid.acid303.cutoff"]}],"tracks":[{"id","instrument","gain_db","pan","mute","solo","fx":["distortion"]}]}`.
  - `tune params [--json]` lists every instrument and effect table: id, label, unit, min, max, curve, default and help. JSON: `{"ok":true,"instruments":{"acid303":[…],"kick":[…],…},"effects":{"distortion":[…]}}`.
  - `tune render <SONG> [--bars A..B] [--stems] --out <PATH> [--json]`. Text: `rendered bars 1..4 (0:08.0) to <path>, peak -3.2 dBFS, pre-clip peak -3.2 dBFS`, plus `warning: master clipper engaged on <n> samples` when n > 0. JSON: `{"ok":true,"files":[…],"frames","seconds","peak_dbfs","pre_clip_peak_dbfs","knee_samples"}`.
  - `tune render-303 … [--json]` emits `{"ok":true,"file","seconds","peak_dbfs","clipped_samples"}`; its text output is unchanged.
  - Errors: exit 2. Text mode prints `<file>:<line>:<col>: <message>` lines (or `error: <message>`) on stderr. JSON mode prints `{"ok":false,"errors":[{"file","line","column","message"}]}` on stdout; `line` and `column` are `null` when unknown.
  - `report::parse_bar_range(s: &str, total_bars: u32) -> Result<Range<u32>, String>`: 1-based inclusive `A..B` → 0-based half-open. It rejects A < 1 (`bars start at 1`), B < A, and B > total (`past the end (4 bars)`).
  - Render path rules: without `--stems`, `--out` must end in `.wav`. With `--stems`, `--out` must not end in `.wav` (`--stems writes a directory`).

- [ ] **Step 1: Write failing tests** (`song_cli.rs`, on fixture `mini.toml`, tempdirs for outputs):
  - `validate --json` gives exit 0 and `bars == 4`, `seconds == 8.0`, `sections == 2`. A copy whose `b` section plays `acid = "nope"` gives exit 2 with `errors[0].line` equal to that line's number (found by searching the copy), and text mode prints `mini.toml:<that line>:` on stderr.
  - `info --json` gives `sections[1].start_bar == 3`, `end_bar == 4`, `start_seconds == 4.0` and `tracks[1].fx == ["distortion"]`.
  - `params --json` gives `instruments.kick[0].id == "kick.tune"` and a non-empty help on every entry.
  - `render --json --out x.wav` gives `frames == 384000` and `peak_dbfs <= -1.0`; the WAV is 2 channels, 24 bits, 384 000 frames. `--bars 2..3` gives `frames == 192000`. `--stems --out dir` writes 4 files and `files.len() == 4`. Rendering twice gives byte-identical WAVs.
  - `--bars 0..2`, `--bars 3..9`, `--stems --out x.wav` and a missing song each give exit 2 with the needle on stderr (or in JSON `errors[0].message`), and the out dir stays empty.
  - `render-303 --demo --json --out d.wav` gives `ok == true`, and the existing `cli.rs` tests pass unchanged.
- [ ] **Step 2: Run** `cargo test -p tune-cli --test song_cli` — Expected: FAIL.
- [ ] **Step 3: Implement** `report.rs` and the four commands.
- [ ] **Step 4: Run** `cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(cli): add validate, info and params commands`; `feat(cli): render songs with bar ranges and stems`.

### Task 13: `tune play` with hot reload, and the agent docs

**Files:**
- Create: `crates/tune-cli/src/audio.rs`, `crates/tune-cli/src/cmd/play.rs`, `crates/tune-cli/tests/play_cli.rs`
- Modify: `main.rs`, `Cargo.toml` (`cpal`, `ctrlc`), `CLAUDE.md` (layout + a new "How Claude uses tune" section), `README.md` ("Songs" section)

**Interfaces:**
- Consumes: `live`, `SongUpdate`, `SongWatcher`, `Reload`, `CompiledSong::new`, `report::parse_bar_range`.
- Produces: `tune play <SONG> [--from-bar N] [--loop A..B] [--device default|null] [--json]`.
  - `audio.rs` opens the default output device as f32. It asks for the song's sample rate and falls back to the device default, printing `note: playing at <r> Hz (song asks <s>)`. The callback calls `LiveProcessor::process`. For channel counts other than 2, it writes L/R to channels 0 and 1 and zeros the rest, using a scratch buffer preallocated for 16 384 frames and chunking larger callbacks.
  - `--device null` runs the processor on a plain thread paced at real time, in 512-frame blocks. This is for tests and headless checks.
  - The main loop polls every 50 ms. It drains watcher events: on `Ok` it compiles and swaps; on `Invalid`/`Io` it prints and keeps the previous version playing. It prints reloads when `swaps_applied` grows, collects garbage, and exits on `finished` or Ctrl-C (`ctrlc` sets a flag).
  - Events (text / JSON line):
    - `playing mini from bar 1 at 120 BPM (48000 Hz, null) - Ctrl-C to stop` / `{"event":"playing","song","bar","bpm","sample_rate","device"}`
    - `reloaded at bar 3` / `{"event":"reloaded","bar"}`
    - `reload failed, still playing the previous version:` followed by error lines / `{"event":"reload_error","errors":[…]}`
    - `finished` / `{"event":"finished"}`
    - `stopped at bar 3` / `{"event":"stopped","bar"}`. Exit code 0.
  - `--from-bar` beyond the song gives exit 2 with `past the end`.
- `CLAUDE.md` "How Claude uses tune" covers:
  - The loop: edit `songs/<name>/song.toml`, run `tune validate --json` and fix positional errors, run `tune info --json` for the bar/time map, run `tune render --bars A..B --json --out scratch/…` for the passage in question, and read `peak_dbfs`/`pre_clip_peak_dbfs`.
  - Notation cheat sheet (acid tokens, drum `x X .`, lanes with normalized values and bar offsets).
  - `tune params --json` as the source of param ids and ranges.
  - Every command takes `--json`; exit 2 means failure.
  - `tune play` is for the producer; Claude may run it with `--device null --json` to check a reload.
  - Never commit renders.

- [ ] **Step 1: Write failing tests** (`play_cli.rs`, spawned child, stdout read line by line with a 5 s timeout per event):
  - `hot_reload_end_to_end`: `play <copy of mini.toml> --loop 1..4 --device null --json` (copy in a tempdir). Expect `playing`. Rewrite with `bpm = 121` and expect `reloaded`. Write `"[song\n"` and expect `reload_error` with `errors[0].line == 1`. Restore and expect `reloaded`. Then `kill -INT <pid>` (via the `kill` command) gives a `stopped` event and exit 0 within 2 s.
  - `plays_to_the_end`: `play mini.toml --from-bar 4 --device null --json` gives `playing` with `bar == 4`, then `finished` within 4 s, exit 0.
  - `from_bar_past_end`: `--from-bar 9` gives exit 2 and `past the end`.
- [ ] **Step 2: Run** `cargo test -p tune-cli --test play_cli` — Expected: FAIL.
- [ ] **Step 3: Implement** `audio.rs` and `play.rs`; write the docs.
- [ ] **Step 4: Run** `cargo test -p tune-cli` — Expected: PASS. Manual check on the real device: copy `crates/tune-song/tests/fixtures/mini.toml` to `scratch/mini.toml`, run `cargo run --release -p tune-cli -- play scratch/mini.toml --loop 1..4 --json`, then edit its `gain_db` and see `reloaded` with no `reload_error`; Ctrl-C prints `stopped`.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `feat(cli): play songs live with hot reload`; `docs: document the song workflow for Claude`.

### Task 14: First acid song

**Files:**
- Create: `songs/first-acid/song.toml`, `crates/tune-cli/tests/songs.rs`
- Modify: `Taskfile.yml` (add `first-acid`: render to `scratch/first-acid.wav` and play it with `afplay`), `README.md` (one line)

**Interfaces:**
- Consumes: the whole CLI.
- Song (Claude drafts it, with comments explaining each musical choice for the producer). 145 BPM, seed 1, 48 kHz. Every section length is a multiple of 8 bars:

  | # | Section | Bars | Start | Plays | Automation (normalized) |
  |---|---|---|---|---|---|
  | 1 | intro | 16 | 1 | kick, hat_closed | — |
  | 2 | intro_acid | 16 | 17 | + acid | cutoff 0.02 → 0.12 |
  | 3 | build | 32 | 33 | + clap, hat_open | cutoff 0.12 → 0.55, resonance 0.80 → 0.85 |
  | 4 | breakdown | 16 | 65 | acid, hat_closed | cutoff 0.35 → 0.25, resonance 0.95 |
  | 5 | drop | 16 | 81 | all | cutoff 0.55 → 0.80, env_mod 0.60 → 0.70 |
  | 6 | drop_peak | 16 | 97 | all, acid variation pattern (octave jumps) | cutoff 0.80 → 0.95 |
  | 7 | outro | 16 | 113 | kick, hats, clap, acid | cutoff 0.60 → 0.05 |
  | 8 | outro_drums | 16 | 129 | kick, hat_closed | — |

  - The 303 uses the demo-like voicing: resonance 85, env mod 60, decay 400, accent 55 (the producer's choice), and soft distortion with drive 12. The main pattern is the Plan 1 demo line. Kick `x...x...x...x...`, closed hat `x.x.x.x.x.x.x.x.` with accents on the offbeats, open hat `..x...x...x...x.`, clap `....x.......x...`.
  - Starting gains: kick −3, clap −9, hat_closed −12 (pan −0.15), hat_open −11 (pan 0.15), acid −2.

- [ ] **Step 1: Write the failing test** `songs.rs`: every `songs/*/song.toml` passes `tune validate --json`. For first-acid, `tune info --json` gives `bpm == 145`, `bars == 144`, section names and bars equal to the table above, and `seconds` in 238.3..238.4 (11 440 552 frames at 48 kHz).
- [ ] **Step 2: Run** `cargo test -p tune-cli --test songs` — Expected: FAIL (song missing).
- [ ] **Step 3: Write** `songs/first-acid/song.toml` and the `first-acid` task.
- [ ] **Step 4: Run** `cargo test -p tune-cli --test songs` — Expected: PASS. Then `cargo run --release -p tune-cli -- render songs/first-acid/song.toml --out scratch/first-acid.wav --json`. Expected: `frames == 11440552`, `peak_dbfs <= -1.0` and `pre_clip_peak_dbfs <= 0.0`. If the pre-clip peak is above 0 dBFS, lower `gain_db` (acid first, then kick) in 1 dB steps, re-render, and record the final values in a song comment.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(songs): draft first acid track`.
- [ ] **Step 6: Listening gate (producer).** The producer runs `task first-acid` and `tune play songs/first-acid/song.toml --from-bar 33`, edits a knob value in the file while it plays, and judges the result. Record the verdict and wishes (drum voicing, levels, arrangement) as input for Plan 3.
