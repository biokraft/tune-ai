# Plan 1 — Foundation and the acid303 voice

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Cargo workspace whose `tune` CLI renders a 303 acid pattern to a WAV file through a faithful Rust port of Open303 plus a distortion stage, with parity tests against the original C++.

**Architecture:** `tune-dsp` holds the ported Open303 DSP (one Rust file per ported C++ class), a step-pattern player and a distortion effect, all headless and deterministic. `open303-ref` is a dev-only crate that compiles the vendored Open303 C++ with `cc` and exposes it through a small C shim, so every ported module is checked sample-for-sample against the original. `tune-cli` turns a pattern string plus parameters into a WAV.

**Tech Stack:** Rust 2024 edition (toolchain 1.97), `cc` (dev-only C++ build), `hound` (WAV), `clap` derive (CLI), `rustfft` optional (see Task 5), Taskfile, bash.

**Spec:** `docs/superpowers/specs/2026-10-20-tune-ai-design.md` (this plan covers delivery stages 1 and 2 of §7). Later stages get their own plans.

## Global Constraints

- macOS on Apple Silicon only.
- Repo stays MIT; Open303 is MIT (Copyright (c) 2009 Robin Schmidt) — its notice ships in `third_party/open303/License.txt` and `THIRD_PARTY.md`. No JC303 (GPL) code may be copied.
- No audio files, absolute home paths, emails or machine-specific data committed; the privacy check enforces it and never contains the sensitive literals itself.
- `tune-dsp` must not depend on Tauri, UI, MCP, `cc`, or `open303-ref` (except as a dev-dependency).
- Determinism: same inputs → bitwise-identical output. No unseeded randomness, no time-dependent behaviour.
- Atomic commits, one concern each, conventional-commit messages. Work on branch `feat/acid303-foundation`.
- Parity: C++ reference compiled with `-O2 -std=c++14 -ffp-contract=off`. Module-level parity tolerance `1e-12` max abs diff; full-voice parity `1e-9` max abs diff over every test render. If a tolerance fails, find the cause (a missed `float` cast, operation order, a default) — do not loosen it. Mirror every C++ `float` cast and `float` member exactly in Rust (`as f32 as f64` where C++ narrows).
- Open303 knob ranges (from `Open303VST.cpp`, normalized 0–1 → plain): waveform lin 0–1; tuning lin 400–480 Hz; cutoff exp 314–2394 Hz; resonance lin 0–100 %; env mod lin 0–100 %; decay exp 200–2000 ms; accent lin 0–100 %; volume lin −60–0 dB. Exp mapping: `min * (max/min)^x`.
- Accent velocity convention for MIDI-style input: normal 90, accent 117 (Open303 treats velocity ≥ 100 as accent).

## Review Focus

1. Extreme settings (resonance 100 %, cutoff 2394 Hz, env mod 100 %, accent 100 %, decay 200 ms, accent on every step) — output stays finite and bounded (|x| < 4.0). Test in Task 7.
2. Sample rates 44 100 / 48 000 / 96 000 — note 69 at tuning 440 plays 440 Hz ± 1 Hz at each. Test in Task 7.
3. Pattern edge cases — all rests (silence, no panic), slide on the last step wrapping into step 0, slide into a rest (acts as normal gate), same-pitch slide (tie: note sustains, no retrigger), 1-step and 64-step patterns. Tests in Task 8.
4. Bad CLI input — unknown parameter id, out-of-range value, malformed pattern token, unwritable path — clear error message, exit code 2, no partial WAV left behind. Test in Task 10.
5. Notes pushed out of MIDI range (C-1 with accent, G9 slides) — clamped to 0–127 like the C++ (`clip(key, 0, 127)`), finite output, no panic. Test in Task 8.

---

## File Structure

```
Cargo.toml                         workspace (members: crates/*)
Taskfile.yml                       setup, fmt:check, lint, test, privacy, verify, demo
CLAUDE.md                          repo working rules for agents
README.md                          what this is, how to run the demo
THIRD_PARTY.md                     Open303 attribution
.githooks/pre-commit               runs scripts/check-privacy.sh --staged
scripts/check-privacy.sh           blocks audio files + personal-data patterns
scripts/privacy-allowlist.txt      paths exempt from the name check (LICENSE files)
third_party/open303/               vendored Open303 Source/DSPCode/* + License.txt (unmodified)
crates/open303-ref/                dev-only: build.rs (cc), shim/shim.cpp, src/lib.rs safe wrappers
crates/tune-dsp/src/
  lib.rs
  param.rs                         ParamSpec, Curve, ParamError
  acid303/mod.rs                   re-exports; ACID303_PARAMS
  acid303/math.rs                  ports of GlobalFunctions / RealFunctions helpers used below
  acid303/leaky_integrator.rs      LeakyIntegrator
  acid303/one_pole.rs              OnePoleFilter
  acid303/biquad.rs                BiquadFilter
  acid303/elliptic_qb.rs           EllipticQuarterBandFilter
  acid303/decay_env.rs             DecayEnvelope
  acid303/analog_env.rs            AnalogEnvelope
  acid303/wavetable.rs             MipMappedWaveTable (+ FFT it needs)
  acid303/blend_osc.rs             BlendOscillator
  acid303/teebee.rs                TeeBeeFilter, FilterMode
  acid303/voice.rs                 Acid303 (port of Open303 minus its internal sequencer)
  acid303/pattern.rs               AcidStep, AcidPattern, parse/format
  acid303/player.rs                AcidPlayer (port of AcidSequencer timing + Open303 seq branch)
  fx/mod.rs, fx/distortion.rs      Distortion, DistortionMode, DIST_PARAMS
crates/tune-dsp/tests/parity_*.rs  parity tests per module (dev-dep open303-ref)
crates/tune-cli/src/main.rs        `tune` binary (clap)
crates/tune-cli/src/render.rs      RenderSpec, render_acid, write_wav
crates/tune-cli/tests/cli.rs       CLI integration tests
```

---

### Task 1: Workspace skeleton, privacy guard, Taskfile

**Files:**
- Create: `Cargo.toml`, `crates/tune-dsp/{Cargo.toml,src/lib.rs}`, `crates/tune-cli/{Cargo.toml,src/main.rs}`, `Taskfile.yml`, `scripts/check-privacy.sh`, `scripts/privacy-allowlist.txt`, `.githooks/pre-commit`, `CLAUDE.md`, `README.md`
- Modify: `.gitignore` (add `/scratch/`)
- Test: `scripts/test-privacy.sh`

**Interfaces:**
- Produces: `task verify` = `fmt:check` + `lint` (`cargo clippy --workspace --all-targets -- -D warnings`) + `test` (`cargo test --workspace`) + `privacy`. `task setup` runs `git config core.hooksPath .githooks`.
- Produces: `scripts/check-privacy.sh [--staged]` — exit 0 clean, exit 1 with one line per finding (`path: reason`). Without `--staged` it scans all tracked files.

- [ ] **Step 1: Write the failing privacy test** `scripts/test-privacy.sh`: in a temp git repo copy the script in, then assert:
  - staged `x.wav` (any of wav/mp3/flac/aif/aiff/ogg/m4a, case-insensitive) → exit 1, output contains `x.wav: audio file`
  - staged text file containing `/Users/` + any name, or `/home/` + any name → exit 1, `home path`
  - staged text containing an email address pattern → exit 1, `email address`; `noreply@anthropic.com` and `@example.com` addresses are allowed
  - staged file listed in `scripts/privacy-allowlist.txt` with a copyright name line → exit 0
  - clean file → exit 0
  Build the sensitive strings in the test by concatenation (`"/Us""ers/"`) so neither script contains them literally.
- [ ] **Step 2: Run** `bash scripts/test-privacy.sh` — Expected: FAIL (script missing).
- [ ] **Step 3: Implement `scripts/check-privacy.sh`** (bash, `git diff --cached --name-only --diff-filter=ACM` for `--staged`, `git ls-files` otherwise; `rg`-free so it runs anywhere — use `grep -E`). Allowlist file initially lists `LICENSE` and `third_party/open303/License.txt`.
- [ ] **Step 4: Run** `bash scripts/test-privacy.sh` — Expected: PASS. Then `bash scripts/check-privacy.sh` on the repo — Expected: exit 0 (fix any finding in committed docs).
- [ ] **Step 5: Workspace + Taskfile.** Workspace `resolver = "3"`, `edition = "2024"`, `[workspace.lints.clippy] all = "warn"`. `tune-cli` bin name `tune` with a `main` printing usage. `.githooks/pre-commit` runs `scripts/check-privacy.sh --staged`. `CLAUDE.md`: layout, `task verify` before every commit, privacy rule, parity rule from Global Constraints, never commit audio. `README.md`: one paragraph on the project, prerequisites, `task setup`, `task verify`, `task demo` (filled in Task 11).
- [ ] **Step 6: Run** `task setup && task verify` — Expected: all green.
- [ ] **Step 7: Commit** (two commits): `chore: add privacy check and pre-commit hook`; `chore: add Cargo workspace and Taskfile`.

### Task 2: Vendored Open303 and the C++ reference crate

**Files:**
- Create: `third_party/open303/` (copy of upstream `Source/DSPCode/*` and `License.txt` at commit `313bf0d9ade7c1dcb6b3a74f5ea1780a29d70074`, unmodified), `THIRD_PARTY.md`, `crates/open303-ref/{Cargo.toml,build.rs,shim/shim.cpp,src/lib.rs}`
- Test: `crates/open303-ref/tests/smoke.rs`

**Interfaces:**
- Produces (Rust, `open303_ref`, all `pub`, each owning its C++ object and freeing it on `Drop`):
  - `RefOpen303::new(sample_rate: f64)`; `set(&mut self, p: RefParam, v: f64)` where `RefParam` covers every Open303 setter listed in `rosic_Open303.h` (Waveform, Tuning, Cutoff, Resonance, EnvMod, Decay, Accent, Volume, AmpSustain, TanhShaperDrive, TanhShaperOffset, PreFilterHighpass, FeedbackHighpass, PostFilterHighpass, SquarePhaseShift, SlideTime, NormalAttack, AccentAttack, AccentDecay, AmpDecay, AmpRelease); `set_filter_mode(&mut self, mode: i32)`; `note_on(&mut self, note: i32, velocity: i32)`; `all_notes_off(&mut self)`; `get_sample(&mut self) -> f64`; `render(&mut self, n: usize) -> Vec<f64>`.
  - Sequencer access for player parity: `seq_set_mode(&mut self, mode: i32)` (`OFF` = 0, `KEY_SYNC` = 1), `seq_set_bpm(&mut self, bpm: f64)`, `seq_set_step(&mut self, i: usize, key: i32, octave: i32, accent: bool, slide: bool, gate: bool)`, `seq_set_num_steps(&mut self, n: usize)`.
  - Per-module handles with a `get_sample`/`process` and the setters each parity test needs: `RefLeakyIntegrator`, `RefOnePole`, `RefBiquad`, `RefEllipticQB`, `RefDecayEnv`, `RefAnalogEnv`, `RefWaveTable` (`sample_table(waveform, index) -> Vec<f64>` of the mip-map level tables), `RefBlendOsc`, `RefTeeBee`.
- Shim: `extern "C"` functions only, opaque pointers, no exceptions across the boundary.

- [ ] **Step 1: Write the failing smoke test** `smoke.rs`: `RefOpen303::new(48000.0)`, `note_on(48, 100)`, `render(4800)` → at least one `abs() > 1e-3`, all finite; then `note_on(48, 0)` and render 96 000 more → last 4800 samples all `abs() < 1e-4`.
- [ ] **Step 2: Run** `cargo test -p open303-ref` — Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** `build.rs` compiling every `.cpp` in `third_party/open303` plus `fft4g.c` and `shim/shim.cpp` with `cc::Build::new().cpp(true).flag("-std=c++14").flag("-O2").flag("-ffp-contract=off").warnings(false)`; `Cargo.toml` with `publish = false`. Write `THIRD_PARTY.md` (name, URL, commit, MIT text pointer, which Rust files are ports).
- [ ] **Step 4: Run** `cargo test -p open303-ref` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit (two commits): `chore: vendor Open303 DSP sources (MIT)`; `test: add open303-ref C++ reference crate`.

### Task 3: Parameter model and small filters

**Files:**
- Create: `crates/tune-dsp/src/param.rs`, `acid303/{mod.rs,math.rs,leaky_integrator.rs,one_pole.rs,biquad.rs,elliptic_qb.rs}`
- Test: `crates/tune-dsp/src/param.rs` (unit), `crates/tune-dsp/tests/parity_filters.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Curve { Linear, Exponential }
  pub struct ParamSpec {
      pub id: &'static str, pub label: &'static str, pub unit: &'static str,
      pub min: f64, pub max: f64, pub curve: Curve, pub default: f64, pub help: &'static str,
  }
  impl ParamSpec {
      pub fn to_plain(&self, normalized: f64) -> f64;   // clamps normalized to 0..=1
      pub fn to_normalized(&self, plain: f64) -> f64;   // clamps plain to min..=max
  }
  #[derive(Debug, thiserror::Error)]
  pub enum ParamError { UnknownId(String), OutOfRange { id: String, value: f64, min: f64, max: f64 } }
  ```
- Produces: Rust ports with the same method names as C++ in snake_case (`set_sample_rate`, `set_cutoff`, `set_mode`, `get_sample`, `reset`, …) for `LeakyIntegrator`, `OnePoleFilter` (+ `OnePoleMode`), `BiquadFilter` (+ `BiquadMode`), `EllipticQuarterBandFilter`. `LeakyIntegrator::get_normalizer(tc1: f64, tc2: f64, fs: f64) -> f64` as an associated fn.
- Port only the helpers from `GlobalFunctions.h`/`rosic_RealFunctions.h` that these and later files call (e.g. `dB2amp`, `amp2dB`, `pitchToFreq`, `pitchOffsetToFreqFactor`, `linToLin`, `linToExp`, `expToLin`, `clip`, `roundToInt`, `beatsToSeconds`) into `math.rs`.

- [ ] **Step 1: Write failing tests.** `param.rs` unit tests: cutoff spec (exp 314–2394) `to_plain(0.0)==314.0`, `to_plain(1.0)==2394.0`, `to_plain(0.5)` ≈ `867.016` (±0.001), `to_normalized(to_plain(x))≈x` for x in {0, 0.25, 0.7, 1}; out-of-range normalized clamps. `parity_filters.rs`: for each filter, a deterministic input (seeded xorshift noise, 1 s at 48 kHz) and each mode/cutoff combo in {20, 150, 1000, 8000 Hz} → Rust vs `open303_ref` max abs diff ≤ 1e-12.
- [ ] **Step 2: Run** `cargo test -p tune-dsp` — Expected: FAIL (types missing).
- [ ] **Step 3: Implement** the ports, line-by-line from `third_party/open303`, keeping C++ structure and mirroring `float` usage.
- [ ] **Step 4: Run** `cargo test -p tune-dsp` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, commit: `feat(dsp): add parameter model and Open303 filter primitives`.

### Task 4: Envelopes

**Files:**
- Create: `acid303/decay_env.rs`, `acid303/analog_env.rs`
- Test: `crates/tune-dsp/tests/parity_envelopes.rs`

**Interfaces:**
- Produces: `DecayEnvelope` (`set_sample_rate`, `set_decay_time_constant`, `get_decay_time_constant`, `set_normalize_sum`, `trigger`, `get_sample`), `AnalogEnvelope` (`set_sample_rate`, `set_attack`, `set_decay`, `set_sustain_level`, `set_sustain_in_decibels`, `set_release`, `set_tau_scale`, `note_on(start_from_current: bool, key: i32, vel: i32)`, `note_off`, `is_note_on`, `end_is_reached`, `get_sample`) — names follow the C++ headers.

- [ ] **Step 1: Write failing parity tests**: DecayEnvelope with decay 200/1000/2000 ms, retriggered at samples 0, 7000, 7001, 30 000; AnalogEnvelope note-on at 0, note-off at 12 000, note-on again at 20 000 during release, with Open303's settings (attack 0, decay 1230, sustain 0, release 0.5 and 50). 2 s at 48 kHz each, max abs diff ≤ 1e-12.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test parity_envelopes` — Expected: FAIL.
- [ ] **Step 3: Implement** the ports.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, commit: `feat(dsp): port Open303 envelopes`.

### Task 5: Wavetables and blend oscillator

**Files:**
- Create: `acid303/wavetable.rs`, `acid303/blend_osc.rs`
- Test: `crates/tune-dsp/tests/parity_osc.rs`

**Interfaces:**
- Produces: `WaveTable` (port of `MipMappedWaveTable`) with `Waveform::{Saw303, Square303}` plus whatever other enum members the oscillator path touches, `set_303_square_phase_shift`; `BlendOscillator` (`set_sample_rate`, `set_wave_table_1/2` — in Rust the oscillator owns both tables, `set_blend_factor`, `set_pulse_width`, `set_frequency`, `calculate_increment`, `reset_phase`, `get_sample`).
- FFT: either port `rosic_FourierTransformerRadix2` + the parts of `fft4g.c` it calls, or use `rustfft`. Whichever passes table parity ≤ 1e-12; prefer the port if `rustfft` misses it.

- [ ] **Step 1: Write failing parity tests**: every mip-map level of Saw303 and Square303 (square phase shift 0 and 180) matches `RefWaveTable` ≤ 1e-12; BlendOscillator at sample rate 192 000 (Open303's 4× oversampled rate for 48 kHz), blend 0 / 0.5 / 1, frequencies 41.2 / 440 / 3520 Hz, plus a frequency change mid-render, 1 s → ≤ 1e-12.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test parity_osc` — Expected: FAIL.
- [ ] **Step 3: Implement** the ports.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, commit: `feat(dsp): port Open303 wavetables and blend oscillator`.

### Task 6: TeeBee filter

**Files:**
- Create: `acid303/teebee.rs`
- Test: `crates/tune-dsp/tests/parity_teebee.rs`

**Interfaces:**
- Produces: `TeeBeeFilter` (`set_sample_rate`, `set_cutoff`, `set_resonance`, `set_drive`, `set_mode(FilterMode)`, `set_feedback_highpass_cutoff`, `reset`, `get_sample`), `FilterMode` enum with every C++ mode in order, `#[repr(i32)]`, default `Tb303`.

- [ ] **Step 1: Write failing parity tests**: seeded saw-plus-noise input at 192 kHz; every `FilterMode`; resonance {0, 50, 85, 100}; cutoff constant {100, 1000, 6000} and a per-sample exponential sweep 100→8000 Hz; 1 s each → ≤ 1e-12.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test parity_teebee` — Expected: FAIL.
- [ ] **Step 3: Implement** the port.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, commit: `feat(dsp): port TeeBee 303 filter`.

### Task 7: The Acid303 voice

**Files:**
- Create: `acid303/voice.rs`; extend `acid303/mod.rs` with `ACID303_PARAMS`
- Test: `crates/tune-dsp/tests/parity_voice.rs`, `crates/tune-dsp/tests/voice_behaviour.rs`

**Interfaces:**
- Consumes: Tasks 3–6 types.
- Produces:
  ```rust
  pub struct Acid303 { /* mirrors Open303 members except `sequencer` */ }
  impl Acid303 {
      pub fn new(sample_rate: f64) -> Self;            // same state as the C++ constructor
      pub fn set_sample_rate(&mut self, sr: f64);
      // one setter per C++ setter, snake_case: set_waveform, set_tuning, set_cutoff, set_resonance,
      // set_env_mod, set_decay, set_accent, set_volume, set_amp_sustain, set_tanh_shaper_drive,
      // set_tanh_shaper_offset, set_pre_filter_highpass, set_feedback_highpass,
      // set_post_filter_highpass, set_square_phase_shift, set_slide_time, set_normal_attack,
      // set_accent_attack, set_accent_decay, set_amp_decay, set_amp_release, set_filter_mode
      pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError>;
      pub fn note_on(&mut self, note: u8, velocity: u8);   // MIDI path, velocity 0 = note off
      pub fn all_notes_off(&mut self);
      pub fn trigger_note(&mut self, note: u8, accent: bool);   // C++ triggerNote
      pub fn slide_to_note(&mut self, note: u8, accent: bool);  // C++ slideToNote
      pub fn release_note(&mut self);                           // C++ releaseNote(currentNote)
      pub fn get_sample(&mut self) -> f64;  // C++ getSample minus the sequencer branch
  }
  pub const ACID303_PARAMS: &[ParamSpec];
  ```
- `ACID303_PARAMS` — ids, ranges and curves from Global Constraints, `default` = the plain value the C++ constructor sets, plus `acid303.slide_time` (Linear 0–200 ms, default 60). Help copy, verbatim:
  - `acid303.waveform` "Blends saw (0) into square (1). Saw is brighter and buzzier; square is hollower."
  - `acid303.tuning` "Master pitch. 440 Hz is standard tuning."
  - `acid303.cutoff` "How bright or muffled the sound is. Slowly opening this over many bars is the classic acid build."
  - `acid303.resonance` "A whistling peak at the cutoff. High values make the line squeal and turn rubbery."
  - `acid303.env_mod` "How far the filter jumps open on each note. This is the 'wow' on every step."
  - `acid303.decay` "How fast the filter closes again after each note. Short is plucky, long is smeared."
  - `acid303.accent` "How much harder and brighter accented steps hit. Back-to-back accents stack into a rising squelch."
  - `acid303.volume` "Output level of the 303."
  - `acid303.slide_time` "How long a slide takes to glide into the next note. This is the wobble."
- `set_param` rejects unknown ids and values outside `min..=max` with `ParamError`.

- [ ] **Step 1: Write failing parity tests** (`parity_voice.rs`, 48 kHz, ≤ 1e-9): (a) defaults, `note_on(36,90)` held 1 s; (b) a scripted MIDI sequence with distinct pitches: overlapping notes (slide), accents (vel 117), note-offs, re-trigger from idle; (c) the same with resonance 90, env mod 80, accent 100, decay 300, cutoff swept per sample from 314 to 2394 Hz; (d) every `FilterMode`. Drive the C++ through `RefOpen303` with identical calls at identical sample indices.
- [ ] **Step 2: Write failing behaviour tests** (`voice_behaviour.rs`): slide does not retrigger the filter envelope (`trigger_note` then `slide_to_note` at sample 6000 → spectral-centroid proxy, e.g. mean |diff| of output over 500 samples after the slide, stays within 20 % of the 500 samples before; a `trigger_note` instead jumps > 50 %); accent stacking: three consecutive accented `trigger_note`s 1/16 apart at 145 BPM, accent 100 → peak abs of each successive step strictly increases; **Review Focus 1** extreme settings over 64 accented steps → all finite, `abs() < 4.0`; **Review Focus 2** note 69, resonance 0, env mod 0, cutoff 2394, saw, at 44 100 / 48 000 / 96 000 → measured fundamental 440 ± 1 Hz (autocorrelation or FFT peak); `set_param("acid303.nope", 1.0)` and `set_param("acid303.cutoff", 5000.0)` → the two `ParamError` variants.
- [ ] **Step 3: Run** `cargo test -p tune-dsp --test parity_voice --test voice_behaviour` — Expected: FAIL.
- [ ] **Step 4: Implement** `Acid303` by porting `rosic_Open303.{h,cpp}`, keeping the note list (`MidiNoteEvent` equality is by key only, as in C++).
- [ ] **Step 5: Run** the same command — Expected: PASS.
- [ ] **Step 6: Run** `task verify`, commit: `feat(dsp): add Acid303 voice ported from Open303`.

### Task 8: Step patterns and the acid player

**Files:**
- Create: `acid303/pattern.rs`, `acid303/player.rs`
- Test: `crates/tune-dsp/src/acid303/pattern.rs` (unit), `crates/tune-dsp/tests/parity_player.rs`, `crates/tune-dsp/tests/player_behaviour.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct AcidStep { pub note: Option<u8>, pub accent: bool, pub slide: bool }
  #[derive(Clone, Debug, PartialEq)]
  pub struct AcidPattern { pub steps: Vec<AcidStep> }   // 1..=64 steps
  impl AcidPattern { pub fn parse(s: &str) -> Result<Self, PatternError>; }
  impl std::fmt::Display for AcidPattern { /* canonical form, round-trips through parse */ }
  pub enum PatternError { BadToken { index: usize, token: String }, ModifierOnRest { index: usize }, Length(usize) }

  pub struct AcidPlayer { /* pattern, bpm, sample rate, step, countdown, drift error, slide flag */ }
  impl AcidPlayer {
      pub fn new(pattern: AcidPattern, bpm: f64, sample_rate: f64) -> Self;
      pub fn set_pattern(&mut self, pattern: AcidPattern);  // takes effect at the next step boundary
      pub fn tick(&mut self, voice: &mut Acid303);          // call exactly once before each voice.get_sample()
  }
  ```
- Notation (one token per 16th step, whitespace-separated): `.` = rest; note = letter `A`–`G`, optional `#` or `b`, octave `-1`–`9`, scientific pitch (`C4` = MIDI 60, `C2` = 36); suffixes in any order: `!` accent, `~` slide (slide from this step into the next). Canonical `Display`: notes with `#` (never `b`), suffix order `!~`, single spaces. Modifiers on a rest → `ModifierOnRest`.
- Player semantics = a port of `AcidSequencer::getNote` (16th-note step timing with `roundToInt` and drift compensation) plus the sequencer branch of `Open303::getSample`, calling `trigger_note` / `slide_to_note` / `release_note` instead of the internal sequencer: gate length `roundToInt(sr * 0.5 * beatsToSeconds(0.25, bpm))`; if a step has `slide` and the next step has a note, no release and the next note slides (same pitch = tie, sustains); notes clamp to 0–127.

- [ ] **Step 1: Write failing unit tests (pattern):** `parse("C2 . D#2! C3~ Bb1!~")` → 5 steps with the expected notes 36/None/39/48/46 and flags; `Display` gives `"C2 . D#2! C3~ A#1!~"`; round-trip for every test pattern; `"H2"` → `BadToken{index:0,..}`; `".!"` → `ModifierOnRest{index:0}`; empty string and 65 tokens → `Length`.
- [ ] **Step 2: Write failing parity test** (`parity_player.rs`, ≤ 1e-9): for several 16-step patterns inside the range C1..C4 (the C++ pattern stores key 0–12 + octave −1..+1 relative to a held root; hold root 36 via `note_on(36,100)` with `seq_set_mode(KEY_SYNC)` and convert our steps to key/octave), at 145 and 133.7 BPM, 8 bars at 48 kHz: `AcidPlayer`+`Acid303` vs `RefOpen303` in sequencer mode. Include slides, accents, rests and a same-pitch slide.
- [ ] **Step 3: Write failing behaviour tests** (`player_behaviour.rs`, **Review Focus 3 and 5**): all-rest pattern → all-zero output, no panic; slide on last step wraps into step 0 (no release at the loop point); slide into a rest acts as a normal gate; same-pitch slide sustains (no amp dip > 3 dB at the step boundary); 1-step and 64-step patterns render 4 bars finite; notes `C-1!` and `G9~` render finite without panic.
- [ ] **Step 4: Run** `cargo test -p tune-dsp` — Expected: FAIL.
- [ ] **Step 5: Implement** `pattern.rs` and `player.rs`.
- [ ] **Step 6: Run** `cargo test -p tune-dsp` — Expected: PASS.
- [ ] **Step 7: Run** `task verify`, commit (two commits): `feat(dsp): add acid step pattern notation`; `feat(dsp): add acid pattern player`.

### Task 9: Distortion

**Files:**
- Create: `crates/tune-dsp/src/fx/{mod.rs,distortion.rs}`
- Test: `crates/tune-dsp/src/fx/distortion.rs` (unit)

**Interfaces:**
- Consumes: `EllipticQuarterBandFilter`, `OnePoleFilter` (Task 3), `ParamSpec`.
- Produces:
  ```rust
  pub enum DistortionMode { Soft, Hard, Overdrive }
  pub struct Distortion;
  impl Distortion {
      pub fn new(sample_rate: f64) -> Self;
      pub fn set_mode(&mut self, m: DistortionMode);
      pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError>;
      pub fn process(&mut self, x: f64) -> f64;
  }
  pub const DIST_PARAMS: &[ParamSpec];
  // dist.drive  Linear 0–48 dB, default 12, help "How hard the 303 is pushed into distortion. More drive, more grit and sustain."
  // dist.mix    Linear 0–1, default 1,    help "Blend of distorted and clean signal."
  // dist.output Linear -24–6 dB, default -6, help "Level after the distortion, to keep loudness in check."
  ```
- Algorithm: gain `g = dB2amp(drive)`; 4× oversampling (zero-stuff ×4 with gain 4, one `EllipticQuarterBandFilter` before and one after the shaper, keep every 4th sample); shapers: Soft `tanh(g·x)`, Hard `clamp(g·x, −1, 1)`, Overdrive `tanh(g·x + 0.25) − tanh(0.25)`; then a 10 Hz `OnePoleFilter` highpass as DC blocker; then `mix` crossfade with the dry input (dry delayed to match latency is the builder's call); then output gain.

- [ ] **Step 1: Write failing tests:** Soft, drive 0, mix 1, output 0 dB, sine amplitude 0.01 → output RMS within 1 % of input RMS after 0.1 s; Hard, drive 24 dB, output 0 dB, sine amplitude 1, 2 s → max abs over the last second ≤ 1.2 (elliptic filter overshoot); Overdrive, 1 kHz sine, 2 s → mean of last second `abs() < 1e-3`; all modes with drive 48 dB on seeded noise → finite; unknown id / out of range → `ParamError`.
- [ ] **Step 2: Run** `cargo test -p tune-dsp fx::` — Expected: FAIL.
- [ ] **Step 3: Implement** `Distortion`.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, commit: `feat(dsp): add oversampled distortion`.

### Task 10: Offline render and the `tune render-303` command

**Files:**
- Create: `crates/tune-cli/src/render.rs`, `crates/tune-cli/src/lib.rs` (exports `render`), `crates/tune-cli/tests/cli.rs`
- Modify: `crates/tune-cli/src/main.rs`, `crates/tune-cli/Cargo.toml` (`clap` with `derive`, `hound`; dev-deps `assert_cmd`, `tempfile`)

**Interfaces:**
- Consumes: `Acid303`, `AcidPlayer`, `AcidPattern`, `Distortion`, `ACID303_PARAMS`, `DIST_PARAMS`.
- Produces:
  ```rust
  pub struct Sweep { pub id: String, pub from_norm: f64, pub to_norm: f64 }  // linear in normalized space over the whole render
  pub struct RenderSpec {
      pub pattern: AcidPattern, pub bpm: f64, pub bars: u32, pub sample_rate: u32,
      pub params: Vec<(String, f64)>,          // plain values, acid303.* or dist.*
      pub sweeps: Vec<Sweep>,
      pub distortion: Option<DistortionMode>,  // None = bypass
  }
  pub fn render_acid(spec: &RenderSpec) -> Result<Vec<f32>, RenderError>;   // mono
  pub fn write_wav(path: &Path, mono: &[f32], sample_rate: u32) -> Result<(), RenderError>;
  ```
- Render length: `bars * 4 beats`, `round(bars * 4 * 60 / bpm * sample_rate)` samples. Sweeps applied every sample via `to_plain`. Output WAV: stereo (dual mono), 24-bit PCM, written to `<path>.tmp` then renamed; on any error no file at `path` and the temp file is removed.
- CLI: `tune render-303 (--pattern <STR> | --demo) [--bpm 145] [--bars 4] [--sample-rate 48000] [--set ID=VALUE]... [--sweep ID=FROM:TO]... [--dist soft|hard|overdrive] --out <FILE.wav>`. `--sweep` values are normalized 0–1. On success prints `rendered <secs>s, peak <dBFS> dBFS` and, if any sample exceeds ±1.0, `warning: <n> samples clipped`. Errors: message on stderr, exit code 2.
- `--demo` = pattern `C2 C2! C3~ C2 . D#2 C2~ G2! . C2 C3 C2~ A#1 C2! . F2~`, 145 BPM, 8 bars, `acid303.cutoff`-sweep 0.15→0.85, `acid303.resonance` 85, `acid303.env_mod` 60, `acid303.decay` 400, `acid303.accent` 80, `--dist soft` with `dist.drive` 12. Explicit flags override demo values.

- [ ] **Step 1: Write failing tests** (`cli.rs`): `--demo --out x.wav` → exit 0, WAV 48 000 Hz, 2 channels, 24-bit, length `round(8*4*60/145*48000)` frames; same command twice → byte-identical files (determinism); `render_acid` called twice with the same spec → bitwise-equal vectors; **Review Focus 4**: `--set acid303.nope=1`, `--set acid303.cutoff=9000`, `--pattern "C2 H2"`, and `--out /nonexistent-dir/x.wav` each → exit 2, stderr names the problem (`unknown parameter`, `out of range`, `bad token`, the path), and no file or `.tmp` left in the target dir.
- [ ] **Step 2: Run** `cargo test -p tune-cli` — Expected: FAIL.
- [ ] **Step 3: Implement** `render.rs` and the `render-303` subcommand.
- [ ] **Step 4: Run** `cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, commit: `feat(cli): render 303 patterns to WAV`.

### Task 11: Demo task and the listening gate

**Files:**
- Modify: `Taskfile.yml` (add `demo`), `README.md` (demo section)

**Interfaces:**
- Produces: `task demo` → `cargo run --release -p tune-cli -- render-303 --demo --out scratch/demo.wav && afplay scratch/demo.wav` (`scratch/` is gitignored).

- [ ] **Step 1: Add** the `demo` task and README section ("Hear the 303": `task demo`; how to tweak with `--set`/`--sweep`; how to A/B against a reference track kept outside the repo).
- [ ] **Step 2: Run** `task verify && task demo` — Expected: green checks, audible acid line with an opening filter.
- [ ] **Step 3: Commit:** `docs: add 303 demo task`.
- [ ] **Step 4: Listening gate (producer).** Ask the producer to run `task demo`, then A/B against their reference track. Record their verdict and any voicing wishes (more squelch, harder accents, different distortion) as input for the next plan. Stage 2 is done when the parity tests pass and the producer says it squelches (spec §8a).
