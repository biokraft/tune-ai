# Plan 5 — Effects and mixer: EQ, delay, reverb, ducking, limiter, sends and the ride

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The rest of the G1 palette and the mixer. A track can carry an EQ, send to a shared dotted-eighth delay and a short reverb, and duck under the kick. The master can run a true-peak limiter, so `first-acid` reaches −8 to −6 LUFS with true peak ≤ −1 dBTP. The 909 kit gains its ride. All of it lives in the song TOML, is listed by `tune params`, editable with `tune set`, automatable by lanes, hot-reloadable, and has knobs in the studio's mixer.

**Architecture:** `tune-dsp` gains the effects (`fx::{Biquad, Equalizer, Delay, Reverb, Ducker, Limiter}`), the strip and master param tables, and `HatKind::Ride`. `tune-song` gains the `eq` fx type, per-track strip params (sends and ducking, stored in the track's `params` table) and a `[mix]` table (master mode, duck source, delay/reverb/master params), addressed as the pseudo-track `mix` (an id already reserved). `tune-engine` compiles them into `CompiledTrack.strip` and `CompiledMix`, runs a fixed per-sample signal flow (track → fx → duck → fader/pan → sends → buses → master gain → clip or limit) and keeps the bus runtimes across hot reloads. `tune-studio` extends `SongPath`, `SongDoc`, the diff and the views; the UI adds send knobs to each strip and an FX/master panel. A song without any of the new keys renders bitwise as before.

**Tech Stack:** Rust 2024 (toolchain 1.97), React 18 + TypeScript + Vitest. No new runtime dependencies. New dev-dependencies only: `tune-analysis` (path) in `tune-dsp` and `tune-engine`, as the true-peak referee in tests (`tune-analysis` depends on neither, so there is no cycle).

**Spec:** `docs/superpowers/specs/2026-10-20-tune-ai-design.md`. This plan covers the remaining parts of §6.7 (EQ, dotted-eighth delay, short reverb, kick-triggered ducking, master clipper and limiter, the ride) and §6.8's mixer sends, under the constraints of §5, §6.2 (determinism), §6.3 (song as text, stable param ids, hot reload, automation) and §8. Sources for default values: `docs/research/2026-10-20-genres.md` §1.6 (303 chain: low cut ~85 Hz, dotted-eighth delay with a high-pass, short spring-type reverb), §6 (kick owns the sub, 303 duck release 50–100 ms, "a volume envelope locked to the kick grid is cleaner than a compressor", −8 to −6 LUFS, TP ≤ −1 dBTP). Plans 1–4 are built; this plan builds on `tune_dsp::{ParamSpec, find_spec, Curve, db2amp}`, `tune_engine::{CompiledSong, Engine, TrackRuntime, SongUpdate, ParamTarget, Meter, render}`, `tune_song::{Song, FxSpec, InstrumentKind, parse_song}`, `tune_studio::{SongPath, SongDoc, diff_songs, SongView}` and `tune analyze`/`tune compare`. Out of scope (Plan 6): sampler, sample browser, knob recording into lanes.

## Global Constraints

- macOS on Apple Silicon only. Repo stays MIT. No GPL code copied (JC303, GPL Freeverb ports): the reverb and limiter are written from the published algorithm descriptions named in their tasks.
- Branch `feat/effects-mixer`, stacked on `feat/ears`. Atomic conventional commits, one concern each. Run `task verify` before every commit. Every commit message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NTWqQ3vUQ6Q116sMFSQZbt
  ```
- Never commit audio, renders, PNGs, screenshots, `tune.local.toml` or anything under `refs/` or `scratch/`. No absolute home paths, emails, real names or machine-specific data in committed files. The privacy check stays green and is never weakened. Never play audio: `tune play`/`tune serve` only with `--device null`.
- Headless crates stay headless: `tune-dsp`, `tune-song`, `tune-engine`, `tune-studio` gain no Tauri, UI, `cpal`, `tokio` or `axum` dependency. `tune-dsp` keeps `#![deny(unsafe_code)]`; the others keep `#![forbid(unsafe_code)]`. No Plan 1 parity test changes, and no file under `crates/tune-dsp/src/acid303/` changes.
- Determinism: the same song, start bar and sample rate give bitwise-identical output offline and live, at any block size. No randomness in any effect (the reverb is a fixed comb/allpass network; nothing is seeded). Sums over tracks run in song-file order. No flush-to-zero tricks or platform-dependent math.
- Backward compatibility: a song with no `[mix]` table, no strip params and no `eq` renders bitwise identically to the code before this plan (pinned by `mini_renders_as_before`, Task 8). The master default stays the soft clipper (`mix.master = "clip"`).
- Realtime: `Engine::process` never allocates, frees, locks or blocks. Every effect buffer is sized in its constructor, which runs off the audio thread (`Engine::new`, `SongUpdate::new`). The delay buffer holds `DELAY_MAX_STEPS` (8) steps at the song's bpm.
- Param ids are stable once committed: `eq.*`, `delay.*`, `reverb.*`, `master.*`, `send.*`, `duck.*`, `ride.*`. Adding a param is allowed; renaming or removing one breaks songs.
- Lane values stay normalized 0..1 over the param's `min..max` (`ParamSpec::to_plain`). Bars: 1-based inclusive in CLI flags and text, 0-based in the engine.
- CLI conventions hold: `--json`, exit 0/2, stable JSON keys (adding keys is allowed).
- Acid targets (spec §6.6): integrated −8 to −6 LUFS, true peak ≤ −1 dBTP.

## Review Focus

1. **Delay buffer at the extremes and bpm changes on reload** (bpm 40 at 384 kHz with `delay.time = 8`; a hot reload that changes bpm while the delay rings). Expected: reads never leave the buffer, the bus is rebuilt off the audio thread, no allocation in `process`, no panic. Tests `buffer_holds_eight_steps_at_40_bpm` (Task 2) and `bpm_swap_rebuilds_mix_without_alloc` (Task 9).
2. **A send that only a lane turns on** (no static `send.delay`, a lane rising from 0). Expected: the bus is built at compile time and the throw is heard; at −60 dB the send is exactly off. Test `lane_only_send_activates_bus` (Task 8).
3. **Ducking edge cases:** a muted duck source, a section that does not play it, accented hits, an explicit source that a reload deletes, a track ducking itself. Expected: triggers follow the source's hits in the arrangement regardless of mute/solo; no hits, no ducking; a dangling or self-ducking source is a validation error with line:col (the reload is refused, playback continues). Tests `mix_errors_have_positions`, `duck_needs_a_source` (Task 7), `muted_source_still_ducks` (Task 8).
4. **Limiter on worst-case material** (inter-sample peaks at fs/4, hot noise, `master.gain = 24`, DC). Expected: sample peak ≤ ceiling, `tune-analysis` true peak ≤ ceiling + 0.3 dB, output finite. Tests `true_peak_stays_under_ceiling`, `extreme_gain_stays_finite` (Task 5).
5. **Old files edited with new paths:** `tune set` creating `[mix]` in a commented file that has none, a strip param on a track with no `params` key, `mix.duck_source=` (clear). Expected: comments kept byte for byte, `[mix]` placed right after `[song]`, file valid, canonical write round-trips. Tests `set_mix_param_creates_mix_table`, `set_strip_param_on_track_without_params`, `clear_duck_source` (Task 10).

## Decisions made in this plan (flag if wrong)

- **Master:** `[mix] master = "clip"` (default, today's soft clipper, bitwise unchanged) or `"limit"` (true-peak limiter; the soft clipper is not used). `master.gain` drives either stage. No master EQ and no clipper-before-limiter stage (YAGNI; both can be added as params later).
- **EQ** is a track fx `{ type = "eq" }`: low cut (24 dB/oct Butterworth), one peaking mid band (freq, gain, Q), a high shelf at 8 kHz, high cut (12 dB/oct). Low cut at 20 Hz, high cut at 20 kHz and 0 dB gains are exact bypasses. This covers the research chain (303 low cut ~85 Hz, mud cut 120–300 Hz, presence/high boost, taming distortion fizz) with six knobs.
- **Strip params** per track, stored in the track's `params` table with the instrument's: `send.delay`, `send.reverb` (dB, −60 = off exactly), `duck.depth` (dB, 0 = off), `duck.release` (ms). Sends are post-fader, pre-pan, mono into the bus; a muted (or solo-excluded) track sends nothing; bus returns stay audible under solo.
- **Mix params** live in `[mix] params = { "delay.feedback" = 40, … }` and are addressed as the pseudo-track `mix`: lanes `param = "mix.reverb.size"`, paths `mix.delay.feedback`, studio knobs `SetParam { track: "mix", … }`. `mix` is already a reserved track id.
- **Ducking** is trigger-based: every hit of the duck source in the arrangement (mute and solo ignored) restarts each ducking track's envelope: 2 ms linear attack, then a raised-cosine recovery over `duck.release`. Source = `[mix] duck_source`, else the first `kick` track.
- **Buses run only when a track sends to them** (an explicit `send.*` param, or a lane on it). Bus params without sends are kept but inactive.
- **Delay:** stereo, `delay.width` blends mono (0) to ping-pong (100); time in 16ths (1–8, default 3 = dotted eighth); high-pass and low-pass inside the feedback loop. **Reverb:** the Freeverb network (a public-domain design), with pre-delay and an input low cut.
- **Limiter:** 1.5 ms lookahead, 4× windowed-sinc true-peak detector, min-hold + boxcar attack, exponential release, default ceiling −1.5 dBTP (margin for detector differences against BS.1770). Its latency (78 frames at 48 kHz) is not compensated: limit-mode renders start 1.6 ms late.
- **Ride** = `HatKind::Ride` on the hat's metal-bank voice (new constants and param table, `ride.*`).
- **Stems** carry each dry track (eq and ducking included, no bus returns).
- **UI:** no new commands. Send/duck knobs on every strip and an FX/master panel; master mode, duck source and per-track EQ are shown read-only or left to TOML/`tune set`.

- Pre-flight F1: T8 tests varying a static `send.delay` (`send_reaches_delay_bus`, `mute_stops_sends`, `solo_keeps_returns`) drop the `acid.send.delay` lane from their variant.
- Pre-flight F2: T9 `mix_override_and_clear` strips `master.gain` from its clip variant, so the ratio is db2amp(12).
- Pre-flight F3: T7 `mix_errors_have_positions` uses three inputs (no `duck_source`; `"kik"`; `"acid"`).
- Pre-flight F4: T11 `set_mix_errors` asserts "is not a parameter of track `mix`"; the error type stays.
- Pre-flight F5: T11 asserts `peak_dbfs <= -1.5 + 1e-9`.
- Pre-flight F6: T7 converts six irrefutable `FxSpec::Distortion` test patterns to `let … else`; added to its Files.
- Pre-flight F7: reuse the existing `noise(seed, n)`; add only `sine`, `rms_db`, `gain_db`, `fnv64`.
- Pre-flight F8: T1 re-exports `acid303::math::db2amp` from `tune-dsp/src/lib.rs`.
- Pre-flight F9: T10 edits only `protocol.ts`; T12 adds the fixtures to `test-fixtures.ts`; `fakeClient.ts` is not edited.
- Pre-flight F10: T11 song tests go in `song_cli.rs`.
- Pre-flight F11: the T8 baseline constants are recorded from a detached `feat/ears` worktree in the scratchpad, before any engine change.
- Pre-flight F12: the EQ struct is `Equalizer`; `FxSpec::Eq`, the `"eq"` type and `eq.*` ids stay.
- Pre-flight F13: T12 `describeChange.findParam` also searches `t.strip`.
- Pre-flight F14: T9 swap order is feedback 45, then `master = "clip"` at bpm 120, then bpm 121.
- Pre-flight F15: T12 extracts a shared `useParamDrag` hook for `Panel303` and `MixPanel`.
- Pre-flight F16: T8 frame code treats a missing runtime bus as off until T9 adds the swap.
- Pre-flight F17: T9 counts mix overrides in `overrides_active` and the `apply_swap` recount.
- Pre-flight F18: a send drag with no bus built is silent until release (v1); FxPanel note and T13 docs say so.
- Pre-flight F19: T7 `fill` skips strip-table ids only and keeps the panic for ids in no known table.
- Pre-flight F20: `Biquad` gets a doc comment saying why it exists beside `acid303::BiquadFilter`.
- Pre-flight F21: the shared `ParamSpec::clamp_plain` refactor is not part of this plan.

---

## File Structure

```
crates/tune-dsp/src/fx/{biquad.rs,eq.rs,delay.rs,reverb.rs,duck.rs,strip.rs,limiter.rs,mod.rs}; src/drums/{hat.rs,mod.rs}; src/lib.rs
crates/tune-dsp/tests/{fx_eq.rs,fx_delay.rs,fx_reverb.rs,fx_duck.rs,fx_limiter.rs,drums.rs}; Cargo.toml (dev-dep)
crates/tune-song/src/{model.rs,raw.rs,validate.rs,write.rs,lib.rs}; tests/{parse.rs,roundtrip.rs,fixtures/mixed.toml}
crates/tune-engine/src/{compile.rs,engine.rs,mixer.rs,tracks.rs,live.rs,lib.rs}; tests/{baseline.rs,mix.rs,live.rs,live_no_alloc.rs,overrides.rs}; Cargo.toml (dev-dep)
crates/tune-studio/src/{path.rs,doc.rs,diff.rs,view.rs,core.rs,studio.rs}; tests/{doc.rs,diff_log.rs,core_edits.rs,studio.rs,fixtures/protocol.json}
crates/tune-cli/src/cmd/{params.rs,render.rs}; tests/{cli.rs,edit_cli.rs}
packages/ui/src/components/{TrackStrip.tsx,FxPanel.tsx}(+tests); src/lib/param.ts; src/index.ts; src/test-fixtures.ts
apps/desktop/src/{client/protocol.ts,client/fakeClient.ts,screens/MixPanel.tsx(+test),screens/StudioScreen.tsx,lib/describeChange.ts}
songs/first-acid/song.toml, CLAUDE.md, README.md
```

Test helpers each new `tune-dsp` test file uses (add only `sine`, `rms_db`, `gain_db`, `fnv64`) (put them in the existing `tests/common/mod.rs`): `sine(sr, hz, amp, secs) -> Vec<f64>`, the existing `noise(seed: u64, n: usize) -> Vec<f64>` (already in that file; reuse it, add no second one, and leave the parity tests untouched), `rms_db(x: &[f64]) -> f64`, `gain_db(fx: impl FnMut(f64) -> f64, sr, hz) -> f64` (steady-state RMS of a 1 s sine after 0.5 s settle, out minus in, in dB), `fnv64(bits: impl Iterator<Item = u64>) -> u64`. Import `db2amp` as `tune_dsp::db2amp` (re-exported in Task 1; the real path is `tune_dsp::acid303::math::db2amp`).

---

### Task 1: Biquad and the track EQ

**Files:**
- Create: `crates/tune-dsp/src/fx/biquad.rs`, `crates/tune-dsp/src/fx/eq.rs`, `crates/tune-dsp/tests/fx_eq.rs`
- Modify: `crates/tune-dsp/src/fx/mod.rs`, `crates/tune-dsp/src/lib.rs` (also `pub use acid303::math::db2amp;`, so every later task and test imports `tune_dsp::db2amp`; `acid303/` itself is untouched), `crates/tune-dsp/tests/common/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum BiquadKind { Lowpass, Highpass, Peak { gain_db: f64 }, HighShelf { gain_db: f64 } }
  /// Doc comment on `Biquad` (required): a new RBJ biquad beside `acid303::BiquadFilter`; `acid303/` is frozen and the EQ needs the cookbook peak and shelf forms the Open303 port lacks.
  #[derive(Clone, Debug)] pub struct Biquad { /* b0 b1 b2 a1 a2, TDF-II state s1 s2, all f64 */ }
  impl Biquad {
      pub fn new() -> Self;                                         // identity: b0 = 1, rest 0
      pub fn set(&mut self, kind: BiquadKind, hz: f64, q: f64, sample_rate: f64);  // RBJ cookbook; hz clamped to 0.49·sr
      pub fn process(&mut self, x: f64) -> f64;
      pub fn reset(&mut self);
  }
  pub const EQ_PARAMS: &[ParamSpec];
  pub struct Equalizer;   // Clone, Debug; not `Eq`, which would shadow `std::cmp::Eq`
  impl Equalizer {
      pub fn new(sample_rate: f64) -> Self;                         // defaults: every stage bypassed
      pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError>;
      pub fn set_param_at(&mut self, index: usize, plain: f64);     // clamped; NaN = min (as Distortion)
      pub fn process(&mut self, x: f64) -> f64;
      pub fn reset(&mut self);
  }
  ```
- `EQ_PARAMS` (id, label, unit, min, max, curve, default, help):
  | id | label | unit | range | curve | default | help |
  | --- | --- | --- | --- | --- | --- | --- |
  | `eq.low_cut` | Low cut | Hz | 20–1000 | Exp | 20 | "Removes everything below this (24 dB/oct). 20 Hz is off. On the 303 try 85 Hz so the kick owns the sub." |
  | `eq.mid_freq` | Mid freq | Hz | 100–8000 | Exp | 1000 | "Centre of the mid band." |
  | `eq.mid_gain` | Mid | dB | −18–18 | Lin | 0 | "Boost or cut around Mid freq. Cut 200-300 Hz to clear mud; boost near 1 kHz to bring the squelch forward. 0 is off." |
  | `eq.mid_q` | Mid Q | (empty) | 0.3–8 | Exp | 1.0 | "Width of the mid band. Higher is narrower." |
  | `eq.high_gain` | High | dB | −18–18 | Lin | 0 | "Shelf above 8 kHz: air and sizzle up, or harshness down. 0 is off." |
  | `eq.high_cut` | High cut | Hz | 1000–20000 | Exp | 20000 | "Removes everything above this (12 dB/oct). 20 kHz is off. Tames distortion fizz." |
- Signal order: low cut (two highpass biquads, Q 0.541196 then 1.306563: 4th-order Butterworth) → mid (`Peak`, Q = `eq.mid_q`) → high shelf (`HighShelf` at 8000 Hz, shelf slope S = 1, i.e. Q 0.7071) → high cut (`Lowpass`, Q 0.7071). A stage at its off value (low cut 20, gain 0, high cut 20000) is skipped entirely; a stage going from skipped to active starts from reset state. Coefficients are recomputed only when the stage's inputs change (lanes set params every sample).

- [ ] **Step 1: Write the failing tests** in `tests/fx_eq.rs` (48 kHz). `defaults_pass_bitwise`: 1 s seeded noise through `Equalizer::new` equals the input bit for bit. `low_cut_is_24_db_per_octave`: low cut 100 → gain at 25 Hz ≤ −40 dB, at 100 Hz −3.0 ± 0.5 dB, at 400 Hz within ±0.3 dB. `mid_band`: 1000 Hz / +6 dB / Q 1 → 1 kHz +6.0 ± 0.1, 100 Hz and 10 kHz within ±0.3. `high_shelf`: +6 → 16 kHz +6.0 ± 0.5, 200 Hz within ±0.1. `high_cut_is_12_db_per_octave`: 2000 → 2 kHz −3.0 ± 0.5, 8 kHz ≤ −20. `swept_params_stay_finite`: each param swept min→max→min across 2 s of noise via `set_param_at` every sample → every output finite and `|y| < 8`. `rejects_out_of_range`: `set_param("eq.mid_gain", 30.0)` is `Err(ParamError::OutOfRange { .. })`, `set_param("eq.nope", 0.0)` is `UnknownId`.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test fx_eq` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** `biquad.rs` (RBJ Audio EQ Cookbook formulas, normalised by a0) and `eq.rs`; export `Biquad, BiquadKind, Equalizer, EQ_PARAMS` from `fx` and the crate root.
- [ ] **Step 4: Run** `cargo test -p tune-dsp` — Expected: PASS, parity tests untouched.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(dsp): add an RBJ biquad and the track EQ`.

### Task 2: Tempo-synced delay

**Files:**
- Create: `crates/tune-dsp/src/fx/delay.rs`, `crates/tune-dsp/tests/fx_delay.rs`
- Modify: `crates/tune-dsp/src/fx/mod.rs`, `crates/tune-dsp/src/lib.rs`

**Interfaces:**
- Consumes: `Biquad`, `BiquadKind` (Task 1).
- Produces:
  ```rust
  pub const DELAY_PARAMS: &[ParamSpec];
  pub const DELAY_MAX_STEPS: f64 = 8.0;
  pub struct Delay;   // Clone, Debug
  impl Delay {
      /// Allocates two lines of ceil(DELAY_MAX_STEPS · samples_per_step) + 2 frames: build off the audio thread.
      pub fn new(sample_rate: f64, samples_per_step: f64) -> Self;
      pub fn samples_per_step(&self) -> f64;
      pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError>;
      pub fn set_param_at(&mut self, index: usize, plain: f64);
      pub fn process(&mut self, x: f64) -> (f64, f64);   // mono send in, stereo return out
      pub fn reset(&mut self);
  }
  ```
- `DELAY_PARAMS`: `delay.time` "Time", unit `16ths`, 1–8 Lin, default 3 ("Echo spacing in 16th notes. 3 is the dotted eighth that makes acid lines roll; 2 is an eighth, 4 a beat."); `delay.feedback` "Feedback" % 0–90 Lin default 35 ("How many repeats. Above 70 % the echoes pile up."); `delay.low_cut` "Low cut" Hz 20–2000 Exp default 250 ("Keeps the repeats out of the kick's low end. 20 Hz is off."); `delay.high_cut` "High cut" Hz 1000–20000 Exp default 6000 ("Darkens each repeat, so echoes sit behind the dry line. 20 kHz is off."); `delay.width` "Width" % 0–100 Lin default 100 ("0 echoes in the middle; 100 bounces left and right (ping-pong).").
- Per frame, in this order: `T = clamp(time · samples_per_step, 1, capacity − 2)`; read `dl`, `dr` at `T` frames behind the write index by linear interpolation (fraction 0 reads the sample exactly); with `w = width/100`, `a = 1 − w`, `fb = feedback/100`: `inL = x + fb·(a·dl + w·dr)`, `inR = a·x + fb·(a·dr + w·dl)`; write `fL(inL)`, `fR(inR)` where `f` = highpass(`low_cut`, Q 0.7071) then lowpass(`high_cut`, Q 0.7071), each skipped at its off value; advance; return `(dl, dr)`.

- [ ] **Step 1: Write the failing tests** in `tests/fx_delay.rs` (48 kHz). `echo_on_the_dotted_eighth`: `samples_per_step` 6000 (120 BPM), filters off, width 100, feedback 50, a unit impulse → `L[18000] == 1.0`, `R[36000] == 0.5`, `L[54000] == 0.25`, and every other sample of both channels in `0..60000` is exactly `0.0`. `width_zero_is_mono`: 2 s of noise → L and R bitwise equal. `fractional_time_interpolates`: 145 BPM (`samples_per_step = 48000·15/145`), filters off, feedback 0 → `L[14896]` = `1 − frac` and `L[14897]` = `frac` (±1e-12, `frac = fract(3·samples_per_step)`), every other sample in `0..20000` is `0.0`. `filters_shape_the_repeats`: defaults → the first echo of a 50 Hz burst is ≥ 12 dB below the burst, of a 1 kHz burst within 1 dB. `max_feedback_is_stable`: feedback 90, defaults, 10 s noise then 20 s zeros → all finite, `|y| < 10`, last-second RMS < 1 % of the RMS of the first second after the input stops. `silence_stays_exactly_zero`. `buffer_holds_eight_steps_at_40_bpm`: `new(48000.0, 18000.0)`, time 8, filters off → the impulse comes back at sample 144000. `time_automation_never_reads_outside`: sweep `delay.time` 1→8→1 every sample for 10 s at 40 BPM and 384 kHz → no panic, all finite.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test fx_delay` — Expected: FAIL.
- [ ] **Step 3: Implement** `delay.rs`.
- [ ] **Step 4: Run** `cargo test -p tune-dsp --test fx_delay` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(dsp): add a tempo-synced ping-pong delay`.

### Task 3: Short reverb

**Files:**
- Create: `crates/tune-dsp/src/fx/reverb.rs`, `crates/tune-dsp/tests/fx_reverb.rs`
- Modify: `crates/tune-dsp/src/fx/mod.rs`, `crates/tune-dsp/src/lib.rs`

**Interfaces:**
- Consumes: `Biquad` (Task 1).
- Produces:
  ```rust
  pub const REVERB_PARAMS: &[ParamSpec];
  pub struct Reverb;   // Clone, Debug
  impl Reverb {
      pub fn new(sample_rate: f64) -> Self;     // allocates every line, pre-delay sized for 100 ms
      pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError>;
      pub fn set_param_at(&mut self, index: usize, plain: f64);
      pub fn process(&mut self, x: f64) -> (f64, f64);
      pub fn reset(&mut self);
  }
  ```
- `REVERB_PARAMS`: `reverb.size` "Size" % 0–100 Lin default 25 ("Room size: how long the tail rings. Keep it small so the groove stays tight."); `reverb.damp` "Damp" % 0–100 Lin default 50 ("Darkens the tail, like a room with soft walls."); `reverb.pre_delay` "Pre-delay" ms 0–100 Lin default 0 ("Gap before the tail starts, so the dry hit stays clear."); `reverb.low_cut` "Low cut" Hz 20–2000 Exp default 400 ("Keeps the tail out of the low end. 20 Hz is off."); `reverb.width` "Width" % 0–100 Lin default 100 ("0 is a mono tail; 100 spreads it wide.").
- Algorithm: the Freeverb network (public domain), written from its published description. Input: pre-delay of `round(pre_delay · sr / 1000)` frames → highpass at `low_cut` (Q 0.7071, skipped at 20) → × 0.015. Per channel 8 parallel combs then 4 series allpasses. Comb lengths at 44.1 kHz `[1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617]`, allpasses `[556, 441, 341, 225]`, right channel each + 23; every length is `round(len · sr / 44100)`. Comb: `out = buf[i]; filt = out·(1 − d) + filt·d; buf[i] = in + filt·g` with `g = 0.7 + 0.28·size/100`, `d = 0.4·damp/100`. Allpass: `b = buf[i]; out = −in + b; buf[i] = in + b·0.5`. Output: `w1 = width/200 + 0.5`, `w2 = (1 − width/100)/2`; `L = oL·w1 + oR·w2`, `R = oR·w1 + oL·w2`.

- [ ] **Step 1: Write the failing tests** in `tests/fx_reverb.rs` (48 kHz). `onset_is_pre_delay_plus_shortest_comb`: low cut off, impulse → the first nonzero L sample is at `1215` with pre-delay 0 and at `2400 + 1215` with 50 ms. `impulse_tail_decays`: defaults → energy in `[1.5 s, 2.0 s)` at least 60 dB below energy in `[0, 0.1 s)`. `size_lengthens_the_tail`: the time for the 10 ms-RMS envelope of the impulse response to fall 40 dB below its peak is ≥ 1.5× longer at size 80 than at size 20. `damp_darkens`: energy above 6 kHz (a `Biquad` highpass at 6 kHz on the output) of the 0.2–1.0 s tail is ≥ 6 dB lower at damp 90 than damp 0. `width_zero_is_mono`: L and R bitwise equal on noise. `wide_is_decorrelated`: width 100, 4 s noise → `|corr(L, R)| < 0.5`. `low_cut_keeps_sub_out`: defaults → output RMS for a 50 Hz sine send is ≥ 20 dB below that for a 2 kHz sine send of the same level. `deterministic_and_silent`: zeros give exactly zeros; two instances give bitwise-equal output on the same noise.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test fx_reverb` — Expected: FAIL.
- [ ] **Step 3: Implement** `reverb.rs`. Nothing is copied, so `THIRD_PARTY.md` does not change; the module doc comment says "Freeverb topology (public domain), implemented from its published description".
- [ ] **Step 4: Run** `cargo test -p tune-dsp --test fx_reverb` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(dsp): add a short Freeverb-style reverb`.

### Task 4: Kick ducking and the strip params

**Files:**
- Create: `crates/tune-dsp/src/fx/duck.rs`, `crates/tune-dsp/src/fx/strip.rs`, `crates/tune-dsp/tests/fx_duck.rs`
- Modify: `crates/tune-dsp/src/fx/mod.rs`, `crates/tune-dsp/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const DUCK_ATTACK_MS: f64 = 2.0;
  #[derive(Clone, Debug)] pub struct Ducker;
  impl Ducker {
      pub fn new(sample_rate: f64) -> Self;     // idle, depth 0, release 80 ms
      pub fn set_depth_db(&mut self, db: f64);  // 0 = off
      pub fn set_release_ms(&mut self, ms: f64);
      pub fn is_active(&self) -> bool;          // depth > 0
      pub fn trigger(&mut self);                // restarts the envelope from its current value
      pub fn next_gain(&mut self) -> f64;       // this frame's gain, then advance one frame
      pub fn reset(&mut self);
  }
  pub const STRIP_PARAMS: &[ParamSpec];
  pub const STRIP_SEND_DELAY: usize = 0; pub const STRIP_SEND_REVERB: usize = 1;
  pub const STRIP_DUCK_DEPTH: usize = 2; pub const STRIP_DUCK_RELEASE: usize = 3;
  pub const SEND_OFF_DB: f64 = -60.0;
  pub fn send_gain(db: f64) -> f64;             // 0.0 at or below SEND_OFF_DB, else db2amp(db)
  ```
- `STRIP_PARAMS`: `send.delay` "Delay" dB −60–0 Lin default −60 ("How much of this track goes to the shared dotted-eighth delay. −60 is off."); `send.reverb` "Reverb" dB −60–0 Lin default −60 ("How much goes to the shared reverb. −60 is off."); `duck.depth` "Duck" dB 0–24 Lin default 0 ("How far this track dips on every kick, so the kick punches through. 0 is off."); `duck.release` "Duck release" ms 20–400 Exp default 80 ("How fast the track comes back after the dip. 50-100 ms suits a 303.").
- Envelope `e`: idle 0. `trigger` stores `e0 = e` and restarts time `t = 0`. For `t < A` (A = 2 ms in frames): `e = e0 + (1 − e0)·t/A`; for `A ≤ t < A + R`: `e = 0.5·(1 + cos(π·(t − A)/R))`; after: `e = 0` (idle). `next_gain = 1 − (1 − db2amp(−depth))·e`, which is exactly `1.0` while idle or at depth 0.

- [ ] **Step 1: Write the failing tests** in `tests/fx_duck.rs` (48 kHz, depth 6, release 100 ms, `k = db2amp(-6.0)`). `idle_gain_is_exactly_one`: 10 000 frames of `next_gain() == 1.0`. `attack_ramps_linearly`: after `trigger`, frame 48 is `1 − (1 − k)·0.5` (±1e-12), frame 96 is `k` (±1e-12). `release_is_a_raised_cosine`: frame 96 + 2400 is `1 − (1 − k)·0.5` (±1e-12) and frame 96 + 4800 is exactly `1.0`. `retrigger_is_continuous`: trigger, 50 ms later trigger again → no step between consecutive frames larger than 0.05. `depth_zero_never_dips`: depth 0 and a trigger → every frame `1.0`. `send_gain_off_at_floor`: `send_gain(-60.0) == 0.0`, `send_gain(-80.0) == 0.0`, `send_gain(-6.0) == db2amp(-6.0)`, `send_gain(0.0) == 1.0`.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test fx_duck` — Expected: FAIL.
- [ ] **Step 3: Implement** `duck.rs` and `strip.rs`.
- [ ] **Step 4: Run** `cargo test -p tune-dsp --test fx_duck` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(dsp): add grid-locked kick ducking and the strip params`.

### Task 5: True-peak master limiter

**Files:**
- Create: `crates/tune-dsp/src/fx/limiter.rs`, `crates/tune-dsp/tests/fx_limiter.rs`
- Modify: `crates/tune-dsp/src/fx/mod.rs`, `crates/tune-dsp/src/lib.rs`, `crates/tune-dsp/Cargo.toml` (dev-dep `tune-analysis = { path = "../tune-analysis" }`)

**Interfaces:**
- Produces:
  ```rust
  pub const MASTER_PARAMS: &[ParamSpec];
  pub const MASTER_GAIN: usize = 0; pub const MASTER_CEILING: usize = 1; pub const MASTER_RELEASE: usize = 2;
  pub const LIMITER_LOOKAHEAD_MS: f64 = 1.5;
  pub const DETECTOR_DELAY: usize = 6;
  #[derive(Clone, Debug)] pub struct Limiter;
  impl Limiter {
      pub fn new(sample_rate: f64) -> Self;     // ceiling −1.5 dB, release 100 ms; allocates its rings
      pub fn latency(&self) -> usize;           // W + DETECTOR_DELAY, W = max(1, round(1.5 ms · sr)); 78 at 48 kHz
      pub fn set_ceiling_db(&mut self, db: f64);
      pub fn set_release_ms(&mut self, ms: f64);
      /// One stereo frame in; the frame from latency() frames ago out, with the gain applied to it.
      pub fn process(&mut self, l: f64, r: f64) -> (f64, f64, f64);
      pub fn reset(&mut self);
  }
  ```
- `MASTER_PARAMS`: `master.gain` "Master gain" dB −12–24 Lin default 0 ("Drives the whole mix into the master stage. With the limiter, this is the loudness knob."); `master.ceiling` "Ceiling" dB −6 to −0.1 Lin default −1.5 ("The limiter never lets the true peak above this."); `master.release` "Release" ms 10–500 Exp default 100 ("How fast the limiter lets go. Short is louder but pumps; long is smoother.").
- Algorithm (pin it; the tests do not determine it):
  1. Detector: per channel a 4× interpolator. For the new frame `n`, the points at fractional time `t = n − 6 + j/4`, `j = 1, 2, 3`, are `Σ_{i=0}^{11} c_j[i]·x[n−i]` with `d = 6 − i − j/4`, `c_j[i] = sinc(d)·(0.42 + 0.5·cos(π·d/6) + 0.08·cos(2π·d/6))`, each phase normalised to sum 1 (computed in `new`). The peak estimate for frame `n − 6` is the max of `|x[n−6]|` and the three points, over both channels.
  2. Target gain `g_t = c / p` when `p > c` (c = `db2amp(ceiling)`), else exactly `1.0`. `new` and `reset` fill the gain rings with `1.0` and the audio rings with `0.0`.
  3. `held` = minimum of `g_t` over the last `W + 1` estimates; `avg` = the mean of the last `W` `held` values, summed directly every frame (no running sum: it drifts and would break the exact 1.0).
  4. Release: `g = avg` if `avg < g_prev`, else `g = g_prev + α·(avg − g_prev)` with `α = 1 − exp(−1 / (release_s · sr))`; snap `g = 1.0` when `1 − g < 1e-9`.
  5. Output = the input frame delayed by `latency()` × `g`, then clamped to `±c`.
- [ ] **Step 1: Write the failing tests** in `tests/fx_limiter.rs` (48 kHz; true peak via `tune_analysis::{Audio, measure_loudness}` on the f32 output). `latency_is_lookahead_plus_detector`: 78 at 48 kHz, 72 at 44.1 kHz (66 + 6). `transparent_below_ceiling`: a −12 dBFS 1 kHz sine → `out[n + 78]` equals `in[n]` bit for bit and the gain is `1.0` on every frame. `sample_peak_never_exceeds_ceiling`: +12 dB noise and a +12 dB 1 kHz sine → `max |out| <= db2amp(-1.5)`. `true_peak_stays_under_ceiling`: (a) a 12 kHz sine at phase π/4, amplitude 2.0, 5 s; (b) seeded noise × 4; (c) 50 Hz bursts (4 per second, 80 ms each, amplitude 3) plus a 3 kHz saw at amplitude 1 → true peak ≤ −1.2 dBTP in each case. `releases_back_to_unity`: 0.1 s at +6 dBFS then a −20 dBFS sine → the gain is exactly `1.0` again within 2 s. `extreme_gain_stays_finite`: input × `db2amp(24)` on noise plus a DC offset of 0.5 → all finite, sample peak ≤ ceiling. `deterministic`: two instances bitwise equal.
- [ ] **Step 2: Run** `cargo test -p tune-dsp --test fx_limiter` — Expected: FAIL.
- [ ] **Step 3: Implement** `limiter.rs`.
- [ ] **Step 4: Run** `cargo test -p tune-dsp` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(dsp): add a lookahead true-peak limiter`.

### Task 6: The 909 ride

**Files:**
- Modify: `crates/tune-dsp/src/drums/{hat.rs,mod.rs}`, `crates/tune-dsp/src/lib.rs`, `crates/tune-song/src/model.rs`, `crates/tune-engine/src/tracks.rs`
- Test: `crates/tune-dsp/tests/drums.rs`, `crates/tune-song/tests/parse.rs`

**Interfaces:**
- Produces: `HatKind::Ride`; `pub const RIDE_PARAMS: &[ParamSpec]` (exported like `HAT_OPEN_PARAMS`); `InstrumentKind::Ride` with name `"ride"`, appended last to `InstrumentKind::ALL` (6 kinds), `params()` → `RIDE_PARAMS`, `is_drum()` true; `make_instrument(InstrumentKind::Ride, sr)` → `Hat::new(HatKind::Ride, sr)`.
- `RIDE_PARAMS`: `ride.tone` "Tone" Hz 2000–10000 Exp default 4500 ("Brightness of the ride. Lower is darker and washier."); `ride.decay` "Decay" ms 300–3000 Exp default 1200 ("How long the ride rings. A long ride glues the 16ths together."); `ride.metal` "Metal" % 0–100 Lin default 75 ("Blend of metallic ring and plain noise.").
- The ride uses the hat engine with its own bank `RIDE_FREQS = [338.7, 502.3, 609.8, 862.5, 891.0, 1320.0]` and `RIDE_MAKEUP` (measured: the default-parameter peak over seeds 1..=50 lies in 0.6..=1.0). Closed and open hats keep `METAL_FREQS` and their makeups, so their output is bitwise unchanged.

- [ ] **Step 1: Pin the hats first.** Add `hats_unchanged_by_ride` to `tests/drums.rs`: FNV-1a over the `f64` bits of 1 s of a default closed hit and a default open hit (seed 1, 48 kHz, one plain and one accented each) equals constants recorded by running it on the unchanged code. Commit alone: `test(dsp): pin the hat output before adding the ride`.
- [ ] **Step 2: Write the failing tests.** `tests/drums.rs`: `ride_rings_longer_than_open_hat` (RMS over 600–900 ms after a default hit: ride > open hat); `ride_default_peak_in_range` (seeds 1..=50: every peak in 0.6..=1.0); `ride_accent_is_louder`; `ride_is_deterministic_per_seed`. `crates/tune-song/tests/parse.rs`: `ride_track_parses` (a `ride` track with a drum pattern `x.x.x.x.x.x.x.x.` and `"ride.decay" = 2000` validates; `"ride.nope"` is an unknown-parameter error listing `ride.tone`).
- [ ] **Step 3: Run** `cargo test -p tune-dsp --test drums && cargo test -p tune-song` — Expected: the new tests FAIL, `hats_unchanged_by_ride` PASSES.
- [ ] **Step 4: Implement** the ride. Update any existing test that asserts the full list of instrument names in an error message to include `ride`.
- [ ] **Step 5: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 6: Run** `task verify`, then commit: `feat(drums): add the 909-style ride`.

### Task 7: Song model — `eq`, strip params and `[mix]`

**Files:**
- Modify: `crates/tune-song/src/{model.rs,raw.rs,validate.rs,write.rs,lib.rs}`; match arms only: `crates/tune-engine/src/{compile.rs,engine.rs,tracks.rs}`, `crates/tune-studio/src/{doc.rs,diff.rs,view.rs}`
- Create: `crates/tune-song/tests/fixtures/mixed.toml`
- Test: `crates/tune-song/tests/{parse.rs,roundtrip.rs}`
- Mechanical test fixes (compile only, no assertion changes): `crates/tune-studio/tests/doc.rs` (lines ~143, 146, 305), `crates/tune-song/tests/parse.rs` (~69), `crates/tune-engine/tests/live.rs` (~491), `crates/tune-engine/tests/live_no_alloc.rs` (~108)

**Interfaces:**
- Consumes: `EQ_PARAMS`, `DELAY_PARAMS`, `REVERB_PARAMS`, `MASTER_PARAMS`, `STRIP_PARAMS`, `STRIP_DUCK_DEPTH` (Tasks 1–5).
- Produces:
  ```rust
  pub struct Song { /* …existing… */ pub mix: Mix }
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct Mix {
      pub master: MasterMode,
      /// Explicit `duck_source`; `None` = the first `kick` track (see `Song::duck_source`).
      pub duck_source: Option<String>,
      /// Explicit mix params in plain units, keyed by full id (`delay.feedback`, `master.gain`).
      pub params: IndexMap<String, f64>,
  }
  impl Mix { pub fn is_default(&self) -> bool; }
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
  pub enum MasterMode { #[default] Clip, Limit }
  impl MasterMode { pub const ALL: [Self; 2]; pub fn name(self) -> &'static str; pub fn from_name(s: &str) -> Option<Self>; }  // "clip", "limit"
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub enum MixUnit { Delay, Reverb, Master }
  impl MixUnit { pub const ALL: [Self; 3]; pub fn name(self) -> &'static str; pub fn params(self) -> &'static [ParamSpec]; }
  pub const MIX_TRACK_ID: &str = "mix";
  pub fn find_mix_param(id: &str) -> Option<(MixUnit, usize, &'static ParamSpec)>;
  impl Song {
      pub fn duck_source(&self) -> Option<&str>;
      /// Delay/Reverb: some track sets `send.<unit>` explicitly or a lane drives it. Master: always true.
      pub fn bus_active(&self, unit: MixUnit) -> bool;
  }
  // Track.params now holds instrument AND strip params, keyed by full id.
  pub enum FxSpec { Distortion { mode, params }, Eq { params: IndexMap<String, f64> } }   // TYPE_NAMES = ["distortion", "eq"]
  // Lane.track may be "mix"; its param is then a mix param id.
  ```
- File shape: root keys add `mix`. `[mix]` keys: `master` (string), `duck_source` (string), `params` (inline table of numbers). An `eq` fx entry takes only `type` and `params`.
- Validation (each at the offending span; messages exact):
  - `master = "loud"` → ``unknown master mode `loud` (expected clip or limit)``.
  - unknown mix param → ``unknown parameter `<id>` for mix (known: delay.time, …, master.release)``; out of range → the existing `ParamSpec::check` message.
  - `duck_source` not a track → ``unknown track `<id>` ``; not a drum track → ``duck_source `<id>` must be a drum track (ducking follows its hits)``.
  - a track's `params` accept its instrument's ids and `STRIP_PARAMS` ids; unknown ids list both tables in `known:`.
  - the duck source with an explicit `duck.depth` > 0 → ``track `<id>` is the duck source and cannot duck itself``.
  - any explicit `duck.depth` > 0 (or a lane on `duck.depth`) when `Song::duck_source()` is `None` → `` `duck.depth` needs a kick to follow: add a kick track or set duck_source in [mix]``.
  - `eq` with `mode` → `eq has no mode`; a second `eq` on a track → the existing "may have only one" rule.
  - lanes: `mix.<id>` resolves against the mix tables (`unknown parameter … for mix`); `<track>.<id>` against instrument, strip, then fx tables.
- Canonical write (`to_toml`): `[mix]` right after the `[song]` block, only when `!mix.is_default()`, with `master = "<name>"`, then `duck_source` when set, then `params` when non-empty. `eq` fx inline: `{ type = "eq", params = { … } }` (no `params` key when empty). Strip params stay in the track's `params` table in insertion order.
- `mixed.toml` (shared by later tasks; write exactly this):
  ```toml
  # Shared fixture: mini plus the Plan 5 mixer (eq, sends, ducking, buses, limiter, ride).
  [song]
  name = "mixed"
  bpm = 120
  seed = 7

  [mix]
  master = "limit"
  params = { "delay.feedback" = 40, "reverb.size" = 30, "master.gain" = 6 }

  [tracks.kick]
  instrument = "kick"
  gain_db = -3

  [tracks.acid]
  instrument = "acid303"
  params = { "acid303.cutoff" = 500, "acid303.resonance" = 70, "send.delay" = -12, "send.reverb" = -18, "duck.depth" = 6 }
  fx = [{ type = "distortion", mode = "soft", params = { "dist.drive" = 12 } }, { type = "eq", params = { "eq.low_cut" = 85 } }]

  [tracks.hat]
  instrument = "hat_closed"
  gain_db = -10
  pan = 0.3
  params = { "send.reverb" = -20 }

  [tracks.ride]
  instrument = "ride"
  gain_db = -14

  [patterns.k4]
  track = "kick"
  steps = "x...x...x...X..."

  [patterns.a1]
  track = "acid"
  steps = "C2 C2! C3~ C2 . D#2 C2~ G2!"

  [patterns.h]
  track = "hat"
  steps = "..x...x...x...x."

  [patterns.r]
  track = "ride"
  steps = "x.x.x.x.x.x.x.x."

  [[sections]]
  name = "a"
  bars = 2
  play = { kick = "k4", hat = "h", ride = "r" }

  [[sections]]
  name = "b"
  bars = 2
  play = { kick = "k4", acid = "a1", hat = "h" }

  [[sections.automation]]
  param = "acid.acid303.cutoff"
  points = [[0, 0.1], [2, 0.8]]

  [[sections.automation]]
  param = "acid.send.delay"
  points = [[0, 0.5], [2, 1.0]]

  [[sections.automation]]
  param = "mix.reverb.size"
  points = [[0, 0.2], [2, 0.6]]
  ```

- [ ] **Step 1: Write the failing tests.** `parse.rs`: `mixed_fixture_parses` (`mix.master == Limit`, `duck_source() == Some("kick")`, acid `params["send.delay"] == -12.0`, acid fx `[Distortion, Eq]`, section b lanes `("acid", "send.delay")` and `("mix", "reverb.size")`, `bus_active(Delay)` and `bus_active(Reverb)` true); `mini_has_default_mix` (`mix == Mix::default()`, both buses inactive); `lane_only_send_is_active` (no static send, a lane on `acid.send.delay` → `bus_active(Delay)`); `mix_errors_have_positions` (three inputs, because `duck_source` is one TOML key and a duplicate stops parsing, and a self-duck error needs the kick to be the source: (a) the main file with no `duck_source`: an unknown `[mix]` key, `master = "loud"`, `"delay.fedback"`, `"delay.feedback" = 95`, `"duck.depth" = 6` on the kick (the default source), `mode` on an eq, `"send.chorus"`, a lane `mix.delay.fedback`; (b) `duck_source = "kik"`; (c) `duck_source = "acid"` → each checked for its messages with expected line:col, in position order); `duck_needs_a_source` (no kick track, acid `"duck.depth" = 6` → that message at the value); `explicit_duck_source_wins` (`duck_source = "hat"` with two kicks → `Some("hat")`). `roundtrip.rs`: `mixed_fixture_round_trips` (`parse(to_toml(s)) == s`, and `to_toml` is stable across two rounds); `no_mix_writes_no_mix_table` (mini's `to_toml` contains no `[mix]`).
- [ ] **Step 2: Run** `cargo test -p tune-song` — Expected: FAIL.
- [ ] **Step 3: Implement** the model, raw reading (`RawMix`, `eq` entries), validation and writer. Generalise `param_values` to take a list of tables.
- [ ] **Step 4: Keep the workspace compiling** with the smallest real arms (no `todo!()`): `impl Effect for Equalizer` and `make_effect` builds an `Equalizer` for `FxSpec::Eq` (`tracks.rs`); `same_fx_chain` treats two `FxSpec::Eq` as alike and compile fills `Eq` params (`engine.rs`, `compile.rs`); compile's `fill` skips ids that belong to the strip table only and keeps its panic for an id in no known table (or use a `debug_assert`), so strip ids in `params` are ignored until Task 8 (compiling strip or mix lanes is Task 8's; no test reaches it before then); `effective_value`, `is_known_param`, `param_ids` and `fx_view` read `FxSpec::Eq` params (`doc.rs`, `diff.rs`, `view.rs`, with `mode: ""`, `modes: []`). Convert the six irrefutable `let FxSpec::Distortion { .. } = …` patterns listed under Files to `let … else { panic!("expected distortion") }`; without this `cargo test --workspace` does not compile once `FxSpec::Eq` exists. Run `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(song): add eq, strip params and the [mix] table`.

### Task 8: Engine — strip, buses and master stage (offline)

**Files:**
- Modify: `crates/tune-engine/src/{compile.rs,engine.rs,mixer.rs,tracks.rs,offline.rs,lib.rs}`, `crates/tune-engine/Cargo.toml` (dev-dep `tune-analysis`)
- Create: `crates/tune-engine/tests/baseline.rs`, `crates/tune-engine/tests/mix.rs`
- Test: `crates/tune-engine/tests/engine.rs`

**Interfaces:**
- Consumes: Tasks 1–7.
- Produces:
  ```rust
  pub const MIX_TRACK: usize = usize::MAX;   // the `track` of a lane or override aimed at the mix
  pub enum ParamTarget { Instrument(usize), Fx { slot: usize, index: usize }, Strip(usize), Mix { unit: MixUnit, index: usize } }
  pub struct CompiledTrack { /* … */ pub strip: Vec<f64> }   // STRIP_PARAMS, defaults filled in
  #[derive(Clone, Debug)] pub struct CompiledMix {
      pub master: MasterMode,
      pub duck_source: Option<usize>,     // track index of Song::duck_source()
      pub delay: Option<Vec<f64>>,        // full DELAY_PARAMS when Song::bus_active(Delay)
      pub reverb: Option<Vec<f64>>,       // full REVERB_PARAMS when Song::bus_active(Reverb)
      pub master_params: Vec<f64>,        // full MASTER_PARAMS
  }
  pub struct CompiledSong { /* … */ pub mix: CompiledMix }
  impl CompiledSong {
      /// `track_id` may be "mix": (track index or MIX_TRACK, target, spec).
      pub fn resolve_param(&self, track_id: &str, param_id: &str) -> Option<(usize, ParamTarget, &'static ParamSpec)>;
  }
  // mixer.rs
  pub struct Strip;       // send gains + Ducker, set by STRIP_PARAMS index; Strip::new(sample_rate), set_param_at, reset
  pub struct MixRuntime;  // Option<Delay>, Option<Reverb>, Option<Limiter> (Limit mode only), master gain, mix overrides
  impl MixRuntime { pub fn build(song: &CompiledSong) -> Self; }   // allocates
  pub struct Meter { /* … */ pub max_reduction_db: f64 }
  ```
- `CompiledTrack.params` and `.strip` partition the track's `params` map by table. `param_target(track, id)` resolves instrument, strip, then fx tables. Mix lanes compile to `CompiledLane { track: MIX_TRACK, target: ParamTarget::Mix { .. } }`.
- Per frame, after step handling (fixed order, extend `Engine::process`'s doc comment):
  - At a step boundary, after every track's `on_step`: if the duck source's cell at this step is a `Hit`, call `trigger()` on every track's ducker.
  - For each track in song order: `y` = instrument → fx chain; if the strip's ducker is active, `y *= next_gain()`; `l, r = y·gain·pan`; `stem(t, l, r)`; if audible (not muted, solo rules): `ml += l`, `mr += r`, `delay_in += y·gain·send_delay`, `reverb_in += y·gain·send_reverb` (each skipped when its bus is `None`).
  - Delay bus (when built): `(dl, dr) = delay.process(delay_in)`; `ml += dl`, `mr += dr`. Then the reverb bus the same way.
  - Master: `ml *= g`, `mr *= g` (`g = db2amp(master.gain)`). Clip mode: `Meter::clip_frame` as today. Limit mode: `pre_clip_peak` records the frame before the limiter, `(l, r, gain) = limiter.process(ml, mr)`, `knee_samples` counts frames with `gain < 1.0`, `max_reduction_db = max(…, −20·log10(gain))`, `peak` records the output.
  - `seek` resets the mix runtime too; `apply_static` (section change) re-applies strip and mix statics; lanes and overrides on `Strip`/`Mix` targets route to the strip or the mix runtime.

- [ ] **Step 1: Pin today's output before touching the engine.** `tests/baseline.rs`: `mini_renders_as_before` renders `mini.toml` (all bars, stems on) and asserts FNV-1a over the `f32` bits of the mix and of each stem, and the bits of the three meter fields, equal constants recorded from the `feat/ears` head, before Tasks 6 and 7 touched the engine: create a detached worktree with `git worktree add --detach <scratchpad>/ears-baseline feat/ears` (scratchpad directory, never inside the repo; the main checkout and branch stay as they are), copy `tests/baseline.rs` and `mini.toml` into it, run the test there to read the constants, paste them into this repo's `tests/baseline.rs`, then `git worktree remove` it. Do not record them from the current branch, which already carries Task 6 and 7 engine edits. Commit alone: `test(engine): pin the mini render before the mixer`.
- [ ] **Step 2: Write the failing tests** in `tests/mix.rs` (inline variants of `mixed.toml`; "clip variant" = `master = "clip"`). Acid plays only in section b, which carries the `acid.send.delay` lane, and lanes overwrite static params every sample (`apply_lanes` after `apply_static`); so every test that varies a static `send.delay` must drop that lane from its variant, as stated per test below. `compiles_mix_and_strip`: on `mixed.toml`, `mix.duck_source == Some(0)`, `delay`/`reverb` are `Some` with `delay[1] == 40.0`, acid `strip == [-12, -18, 6, 80]`; section b lanes include `(1, Strip(STRIP_SEND_DELAY))` and `(MIX_TRACK, Mix { unit: Reverb, index: 0 })`; `resolve_param("mix", "master.gain") == Some((MIX_TRACK, Mix { unit: Master, index: 0 }, _))`. `buses_inactive_without_sends`: mini and a song with `[mix] params = { "delay.feedback" = 50 }` and no sends → both `None`. `lane_only_send_activates_bus`: clip variant without the static acid send, keeping the lane → `delay` is `Some` and the mix differs from the same song with the lane removed. `send_reaches_delay_bus`: clip variant without the `acid.send.delay` lane (a lane overwrites the static send every sample) with only acid (sends off vs `send.delay = 0`, `delay.feedback = 0`, delay filters off) → the two mixes are bitwise equal before frame `bar_start(2) + 18000 − 1` and differ after. `ducking_follows_kick_hits`: clip variant, acid stem with `duck.depth = 12` vs 0 → at kick hit `h` in section b, `stem12[h + 96] / stem0[h + 96]` is `db2amp(-12)` within 1e-5 (where `|stem0| > 1e-3`), and at `h + 96 + 3840` (80 ms) the two stems are bitwise equal. `muted_source_still_ducks`: the same with the kick muted → identical acid stems. `mute_stops_sends`: clip variant without the `acid.send.delay` lane, acid muted with `send.delay = 0` vs `-60` → bitwise-equal mixes. `solo_keeps_returns`: clip variant without the `acid.send.delay` lane, acid soloed with `send.delay = 0` vs `-60` → the mixes differ. `master_gain_drives_clipper`: clip variant `master.gain = 6` vs 0 → `pre_clip_peak` ratio `db2amp(6)` ± 1e-9. `limit_mode_meets_ceiling`: `mixed.toml` → `peak <= db2amp(-1.5)`, `max_reduction_db > 0`, and `tune_analysis` true peak of the mix ≤ −1.2 dBTP. `eq_in_chain_runs`: acid stem with and without the eq differ. `ride_track_plays`: section a's ride stem is non-silent. In `tests/engine.rs`, existing tests keep passing unchanged.
- [ ] **Step 3: Run** `cargo test -p tune-engine` — Expected: `tests/mix.rs` FAILS, `mini_renders_as_before` PASSES.
- [ ] **Step 4: Implement** compile (strip partition, `CompiledMix`, mix lanes), `Strip`, `MixRuntime`, the frame order and the meter field. `Engine::new`/`from_parts` build the `MixRuntime` (both run off the audio thread). Until Task 9 adds the swap, a hot reload can leave the runtime out of step with the `CompiledMix` (a bus added or removed): the frame code treats a missing runtime bus as off and never indexes or unwraps it.
- [ ] **Step 5: Run** `cargo test -p tune-engine` — Expected: PASS, including `mini_renders_as_before` with the constants of Step 1 unchanged.
- [ ] **Step 6: Run** `task verify`, then commit: `feat(engine): add track sends, ducking, the delay and reverb buses and the limiter`.

### Task 9: Engine — live swaps and knob overrides with the mix

**Files:**
- Modify: `crates/tune-engine/src/{engine.rs,live.rs}`
- Test: `crates/tune-engine/tests/{live.rs,live_no_alloc.rs,overrides.rs}`

**Interfaces:**
- Consumes: `MixRuntime`, `MIX_TRACK`, `ParamTarget::{Strip, Mix}` (Task 8).
- Produces: `SongUpdate` carries a fresh `MixRuntime` (built in `SongUpdate::new`); `Engine::mix_swaps_kept(&self) -> u64` (swaps that kept the running mix runtime). `Engine::set_override` / `LiveController::set_override` accept `Strip` targets on a track and `Mix` targets with `track = MIX_TRACK` (no new command).
- A swap keeps the running `MixRuntime` (tails and mix overrides intact) when the new song's mix is built alike: same master mode, same delay and reverb presence, and bit-equal `clock.samples_per_step`; otherwise the fresh one takes over and the old goes back in the retired update. Strip state and overrides travel with the kept track runtime. `clear_overrides` clears strip and mix overrides too. Mix overrides count toward `overrides_active` (`clear_overrides` early-returns at 0, so they must): `apply_swap` recounts `tracks.map(overrides_set)` plus the kept `MixRuntime`'s overrides (0 when it is replaced).

- [ ] **Step 1: Write the failing tests.** `live.rs`: `mix_live_matches_offline` (`mixed.toml`, block sizes 1, 61, 512, 4096 → bitwise equal to `render`); `swap_keeps_mix_when_alike` (swap to `mixed` with `delay.feedback = 45` → `mix_swaps_kept() == 1`; then to one with `master = "clip"` at bpm 120 → still 1; then to one with `bpm = 121` → still 1, so the master-mode rule is tested on its own before the bpm rule; playback continues with no panic). `overrides.rs`: `mix_override_and_clear` (clip variant of `mixed.toml` with `"master.gain"` removed from its `[mix] params` (the fixture's static 6 dB would make the ratio db2amp(6), not db2amp(12)); the master stage holds no state: `set_override(MIX_TRACK, Mix { unit: Master, index: MASTER_GAIN }, Some(12.0))` makes the next bar's `pre_clip_peak` ≥ 3.9× that of an engine without the override; after `None`, every following frame is bitwise equal to that engine's); `strip_override_on_send` (acid `Strip(STRIP_SEND_DELAY)` to 0 dB changes the mix; cleared → static again). `live_no_alloc.rs`: `mixed_song_runs_without_alloc` (play `mixed.toml`, mix and strip overrides, a kept swap) and `bpm_swap_rebuilds_mix_without_alloc` (a swap to `bpm = 40` with `delay.time = 8` at 384 kHz) → zero allocations and frees inside `process`.
- [ ] **Step 2: Run** `cargo test -p tune-engine` — Expected: FAIL.
- [ ] **Step 3: Implement** the update's mix runtime, the keep-or-replace rule in `apply_swap` (moves only), and the override routing.
- [ ] **Step 4: Run** `cargo test -p tune-engine` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(engine): keep bus tails across hot reloads and override mix knobs live`.

### Task 10: Studio — mix paths, edits, diff, views and knob drags

**Files:**
- Modify: `crates/tune-studio/src/{path.rs,doc.rs,diff.rs,view.rs,core.rs,studio.rs,lib.rs}`, `crates/tune-studio/tests/fixtures/protocol.json` (regenerated), `apps/desktop/src/client/protocol.ts`
- Test: `crates/tune-studio/tests/{doc.rs,diff_log.rs,core_edits.rs,studio.rs,protocol.rs}`

**Interfaces:**
- Consumes: `Mix`, `MasterMode`, `MixUnit`, `find_mix_param`, `Song::{duck_source, bus_active}` (Task 7); `CompiledSong::resolve_param` (Task 8).
- Produces:
  ```rust
  pub enum SongPath { /* … */
      MixParam { param: String },   // `mix.<param id>`, e.g. `mix.delay.feedback`
      MixMaster,                    // `mix.master`: "clip" | "limit"
      MixDuckSource,                // `mix.duck_source`: track id, or null for the default
  }
  impl SongPath { /// `tracks.<t>.<param>`, or `mix.<param>` when `track == "mix"`.
      pub fn param(track: &str, param: &str) -> SongPath; }
  pub struct TrackView { /* … */ pub strip: Vec<ParamView> }   // STRIP_PARAMS with values
  #[derive(Clone, Debug, PartialEq, Serialize)]
  pub struct MixView {
      pub master: String, pub masters: Vec<String>,           // "clip" | "limit"; both, in that order
      pub duck_source: Option<String>,                        // effective (Song::duck_source)
      pub delay: Vec<ParamView>, pub reverb: Vec<ParamView>, pub master_params: Vec<ParamView>,
      pub delay_active: bool, pub reverb_active: bool,
  }
  pub struct SongView { /* … */ pub mix: MixView }
  // FxView for eq: kind "eq", mode "", modes [].
  ```
- Grammar: `mix.master` and `mix.duck_source` are fixed; `mix.<prefix>.<name>` is `MixParam`. The `FromStr` error's example list adds `mix.<param id>`. Strip ids need no new grammar (`tracks.acid.send.delay` is already a `TrackParam`).
- `SongDoc::set`: a strip param writes into the track's `params` table (created inline when absent, as instrument params are). `MixParam` writes into `[mix] params`; `MixMaster` sets `master`; `MixDuckSource` sets `duck_source`, and `null` removes the key. When `[mix]` is absent it is created as a table placed directly after the `[song]` table (reuse `normalize_positions` and shift later positions, as `duplicate_section` does). Unknown mix ids → `EditError::UnknownParam { track: "mix", instrument: "mix", param }` (its text is ``"`<id>` is not a parameter of track `mix` (mix); see `tune params --json`"``; do not change the error type).
- `effective_value`: `MixParam` → explicit or default (`None` for an unknown id); `MixMaster` → name; `MixDuckSource` → `Song::duck_source()` or `null`. Track params read instrument, strip and fx (`FxSpec::Eq` included).
- `diff_songs`: after `song.bpm`, `mix.master`, `mix.duck_source`, then every mix param id in `MixUnit::ALL` table order; track param ids include `STRIP_PARAMS`.
- `apply_command(SetParam)` and the studio's drag/release build the path with `SongPath::param`; the studio's `param_target` uses `CompiledSong::resolve_param`; `is_automated("mix", …)` already works because mix lanes carry `track == "mix"`.

- [ ] **Step 1: Write the failing tests.** `doc.rs`: `mix_paths_round_trip` (`mix.master`, `mix.duck_source`, `mix.delay.feedback`, `tracks.acid.send.delay` parse and print back; `mix`, `mix.`, `mix.delay`, `mix.delay.` do not parse); `set_mix_param_creates_mix_table` (on `commented.toml`: `mix.master.gain = 6` → the text gains `[mix]` with `params = { "master.gain" = 6 }` directly after the `[song]` table, every other line unchanged byte for byte, and the song parses); `set_strip_param_on_track_without_params` (a track with no `params` key gets `params = { "send.delay" = -14 }`); `set_eq_param` (`tracks.acid.eq.low_cut = 120` on `mixed.toml` edits the eq entry); `set_master_mode_bad_value` (`mix.master = "loud"` → `EditError::Invalid` whose text contains `unknown master mode`); `clear_duck_source` (`mix.duck_source = null` removes the key; `get` then returns `"kick"`). `diff_log.rs`: `diff_reports_mix_changes` (paths `mix.master`, `mix.delay.feedback`, `tracks.acid.send.delay`, in that order). `core_edits.rs`: `set_param_on_mix_commits` (`SetParam { track: "mix", param: "reverb.size", value: 42.0, commit: true }` writes `mix.reverb.size`); `mix_lane_is_automated` (`is_automated("mix", "reverb.size", 2)` on `mixed.toml`). `studio.rs`: `drag_mix_knob_overrides_live` (`NullOut`: a drag on `mix`/`master.gain` → `overrides_active() == 1`; release → written, override cleared). `protocol.rs`: regenerate with `UPDATE_GOLDEN=1` after the views change and review the diff (only `mix`, `strip` and eq fx entries may appear).
- [ ] **Step 2: Run** `cargo test -p tune-studio` — Expected: FAIL.
- [ ] **Step 3: Implement** the path variants, `SongDoc` edits, diff, views and the `SetParam`/drag routing.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Run** `task verify` (the frontend's `protocol.test.ts` reads the regenerated golden; update only the `apps/desktop/src/client/protocol.ts` types `TrackView.strip`, `MixView`, `SongView.mix` in this commit so the typecheck stays green; `test-fixtures.ts` holds only `ParamSpecView` constants (Task 12 adds the strip/FX ones) and `fakeClient.ts` casts the golden `protocol.json`, so it updates with the regeneration and needs no edit), then commit: `feat(studio): edit, diff and show the mix and strip params`.

### Task 11: CLI — params, render report, set

**Files:**
- Modify: `crates/tune-cli/src/cmd/{params.rs,render.rs}`
- Test: `crates/tune-cli/tests/{song_cli.rs,edit_cli.rs}` (the song `params`/`render` tests and their helpers `mini()`, `json_of` live in `song_cli.rs`, not `cli.rs`)

**Interfaces:**
- Consumes: `STRIP_PARAMS`, `EQ_PARAMS`, `MixUnit` tables, `Meter::max_reduction_db`, `CompiledMix::master`.
- `tune params --json` adds `"strip": [spec…]` and `"mix": {"delay": […], "reverb": […], "master": […]}`; `effects` gains `eq`; `instruments` gains `ride`. Text adds headings `strip` and `mix delay|reverb|master`.
- `tune render --json` adds `"master": "clip" | "limit"` and `"max_reduction_db"` (2 decimals). Limit mode text on stderr: `note: limiter reduced peaks by up to X.X dB` when > 0, `warning: limiter reduced peaks by X.X dB: lower mix.master.gain` when > 9.0. Clip-mode messages unchanged.
- `tune set` needs no code: `parse_assignment` already parses through `SongPath`.

- [ ] **Step 1: Write the failing tests.** `song_cli.rs`: `params_lists_mix_and_strip` (ids include `send.delay`, `duck.depth`, `delay.time`, `reverb.size`, `master.ceiling`, `eq.low_cut`, `ride.decay`); `render_reports_limiter` (`mixed.toml` → `master == "limit"`, `max_reduction_db >= 0`, `peak_dbfs <= -1.5 + 1e-9`: the limiter clamps to db2amp(-1.5), whose 20·log10 is -1.4999999999999996); `render_mini_reports_clip` (`master == "clip"`, `max_reduction_db == 0`, every pre-existing key present). `edit_cli.rs`: `set_mix_paths` (a temp copy of `mixed.toml`: `tune set <copy> mix.delay.feedback=50 tracks.acid.send.delay=-10 mix.master=clip --json` → three changes, the file validates, `tune changes` lists them as `claude`); `set_mix_errors` (`mix.delay.fedback=1` → exit 2, message contains ``is not a parameter of track `mix` `` (the text of the existing `set_unknown_path` test); `mix.master=loud` → contains `unknown master mode`).
- [ ] **Step 2: Run** `cargo test -p tune-cli` — Expected: FAIL.
- [ ] **Step 3: Implement** both commands' additions.
- [ ] **Step 4: Run** `cargo test -p tune-cli` — Expected: PASS.
- [ ] **Step 5: Run** `task verify`, then commit: `feat(cli): list mix params and report the limiter`.

### Task 12: Studio UI — send knobs and the FX/master panel

**Files:**
- Modify: `packages/ui/src/components/TrackStrip.tsx` (+ `TrackStrip.test.tsx`), `packages/ui/src/lib/param.ts` (+ test), `packages/ui/src/index.ts`, `packages/ui/src/test-fixtures.ts` (add the strip/FX `ParamSpecView` fixtures), `apps/desktop/src/screens/{StudioScreen.tsx,Panel303.tsx}`, `apps/desktop/src/lib/describeChange.ts`
- Create: `packages/ui/src/components/FxPanel.tsx` (+ `FxPanel.test.tsx`), `apps/desktop/src/screens/MixPanel.tsx` (+ `MixPanel.test.tsx`), `apps/desktop/src/screens/useParamDrag.ts` (shared hook)

**Interfaces:**
- Consumes: the wire types of Task 10 (`TrackView.strip`, `SongView.mix`), `set_param` with `track: "mix"`.
- Produces:
  ```ts
  export type TrackStripProps = {
    track: { id: string; gain_db: number; pan: number; mute: boolean; solo: boolean };
    onMix(patch: { gain_db?: number; pan?: number; mute?: boolean; solo?: boolean }): void;
    /** STRIP params; the strip shows knobs for send.delay, send.reverb and duck.depth. Omit to hide them. */
    strip?: ParamSpecView[];
    /** Live while dragging (commit false), written on release (commit true). */
    onParam?(id: string, value: number, commit: boolean): void;
    highlight?(id: string): Highlight;
  };
  export type FxPanelProps = {
    title: string;
    params: ParamSpecView[];
    /** Dims the knobs and shows this line, e.g. "No track sends here yet". */
    inactiveNote?: string | null;
    onParam(id: string, value: number, commit: boolean): void;
    highlight?(id: string): Highlight;
    automated?(id: string): boolean;
    onHelp?(spec: ParamSpecView | null): void;
  };
  export function FxPanel(props: FxPanelProps): JSX.Element;
  ```
- `formatValue` gains unit `16ths`: whole values print `3/16`, others two decimals plus `/16`.
- `MixPanel({ song, highlights, automatedIn, send })`: three `FxPanel`s (Delay with `inactiveNote` when `!delay_active`, Reverb likewise, Master) and two read-only lines: `Master: limiter` or `Master: soft clipper (set mix.master = "limit" in the song for the limiter)`, and `Ducking follows: <id>` or `Ducking: no kick to follow`. Do not copy `Panel303`'s logic: extract its live-value map, `settled`/`SETTLE_MS` handling and `frameThrottle` wiring into a shared hook `useParamDrag(track, send)` (`useParamDrag.ts`), and use it in both `Panel303` and `MixPanel`. Drags go through that hook with `{ cmd: "set_param", args: { track: "mix", param, value, commit } }`; highlights read `mix.<param id>`. `StudioScreen` passes `strip` and `onParam` (track id) to each `MixerStrip` (highlights `tracks.<t>.<param id>`) and renders `MixPanel` under the strips in the Mixer aside. `describeChange` names mix paths ("delay feedback", "master gain"), and its `findParam` also searches `t.strip` (today only `t.params` and `t.fx`), so `tracks.<t>.send.delay` changes get a label and unit. A send knob dragged on a track with no bus built is silent until release (accepted for v1, spec §6.3): the FxPanel `inactiveNote` says "sends start after the first release".

- [ ] **Step 1: Write the failing tests.** `TrackStrip.test.tsx`: `renders send knobs when strip given` (knobs labelled Delay, Reverb, Duck), `send knob release calls onParam(id, v, true)`, `no strip, no send knobs`. `FxPanel.test.tsx`: `one knob per param`, `inactive note dims the panel`, `keyboard step commits`. `param.test.ts`: `formats 16ths` (`3` → `3/16`, `2.5` → `2.50/16`). `MixPanel.test.tsx` (fake client): `drag sends set_param on mix with commit false`, `release commits`, `claude change on mix.delay.feedback rings its knob`, `shows master mode and duck source`.
- [ ] **Step 2: Run** `pnpm -r test` — Expected: FAIL.
- [ ] **Step 3: Implement** the components, wiring and fixtures.
- [ ] **Step 4: Run** `pnpm -r test && pnpm -r typecheck` — Expected: PASS.
- [ ] **Step 5: Look at it.** `task ui:build`, copy `mixed.toml` to `scratch/mixed/song.toml`, run `tune serve scratch/mixed/song.toml --device null --port 4580` in the background, open `http://127.0.0.1:4580/` with the `agent-browser` skill, drag a send knob and a delay knob, `screenshot scratch/ui-mixer.png`, read the PNG — Expected: send knobs on every strip, the FX/master panel readable at 1280 px wide, the dragged values written (`tune changes` shows them as `human`). Stop the server. Nothing under `scratch/` is committed.
- [ ] **Step 6: Run** `task verify`, then commit: `feat(ui): add send knobs and the FX/master panel`.

### Task 13: Docs — mixing with tune

**Files:**
- Modify: `CLAUDE.md`, `README.md`

**Interfaces:**
- Consumes: the file shape, paths and CLI output of Tasks 7–11.
- `CLAUDE.md`: in Layout, `fx/*` lists eq, delay, reverb, duck, limiter. Add "### Mixing" under "How Claude uses tune": the signal flow in one line (instrument → fx (distortion, eq) → duck → gain/pan → sends → delay and reverb buses → master gain → clip or limit); the `[mix]` table with an example; strip params live in a track's `params`; `eq` as an fx entry; lanes on `<track>.send.delay` (delay throws) and `mix.<param id>`; buses run only when a track sends to them; `duck_source` defaults to the first kick; `master = "limit"` for loud masters (the soft clipper stays the default); limit-mode renders start 1.6 ms late (lookahead); a send knob dragged on a track with no static send and no lane is silent until release (sends start after the first release); stems carry dry tracks only. Extend the `tune set` path table with `tracks.<t>.send.delay|send.reverb|duck.depth|duck.release`, `tracks.<t>.eq.<name>`, `mix.<param id>`, `mix.master` (`clip`/`limit`), `mix.duck_source` (track id; empty = first kick). In the render step, describe `master` and `max_reduction_db` and the limiter note/warning. In "Listening", add: to hit −8..−6 LUFS raise `mix.master.gain` in limit mode and keep `max_reduction_db` under about 9.
- `README.md`: a short "Mixing" section with one `[mix]` example and `tune set` examples.

- [ ] **Step 1: Write** the docs.
- [ ] **Step 2: Run** every command quoted in the new sections against a temp copy of `mixed.toml` and check each flag and JSON key named — Expected: every command exits 0 (or 2 where the docs say it fails) and every key exists.
- [ ] **Step 3: Run** `task verify`, then commit: `docs: document the mixer, sends, ducking and the limiter`.

### Task 14: Mix `first-acid` and listen

**Files:**
- Modify: `songs/first-acid/song.toml`
- Create (gitignored, not committed): `scratch/mix/*`, `scratch/mix-first-acid.md`

**Interfaces:**
- Consumes: everything above; Plan 4's ears; the controller supplies `tune.local.toml` with `[refs] this-is-acid = "<path>"` when available.
- Starting point (Plan 4 findings: −16.0 LUFS integrated, TP −2.4 dBTP, `low_mid` −17.5 dB re total, build only +0.2 LU over intro_acid, drop +6.9 LU over breakdown). Starting values, all via `tune set` (comments added by hand afterwards):
  - `mix.master = limit`, `mix.master.gain = 8`, `mix.master.ceiling = -1.5`, `mix.master.release = 100`, `mix.delay.time = 3`, `mix.delay.feedback = 35`, `mix.delay.low_cut = 300`, `mix.delay.high_cut = 5000`, `mix.reverb.size = 25`, `mix.reverb.damp = 60`, `mix.reverb.low_cut = 400`.
  - `acid`: `gain_db = 1` (from −2: the 303 louder against the kick), `send.delay = -16`, `send.reverb = -22`, `duck.depth = 4`, `duck.release = 90`; fx gains `{ type = "eq", params = { "eq.low_cut" = 85, "eq.mid_freq" = 300, "eq.mid_gain" = -2, "eq.high_gain" = 2 } }` after the distortion (hand edit: `tune set` cannot add an fx entry).
  - `clap`: `send.reverb = -14`; `hat_open`: `send.reverb = -24`; `hat_closed`: `send.reverb = -28`. Kick: no sends.
  - Breakdown: lane `acid.send.delay` `[[0, 0.75], [16, 0.85]]` (−15 → −9 dB: delay throws while the kick is out). Build: lane `acid.send.delay` `[[0, 0.6], [32, 0.75]]` (−24 → −15 dB) for rising energy.

- [ ] **Step 1: Baseline.** `cargo build --release -p tune-cli`; `target/release/tune analyze songs/first-acid/song.toml --sections --png scratch/mix/before.png --json > scratch/mix/before.json`. Expected: the Plan 4 numbers within ±0.1 (the song is unchanged so far).
- [ ] **Step 2: Apply the starting mix** (above), then `tune validate songs/first-acid/song.toml --json` and `tune render songs/first-acid/song.toml --json --out scratch/mix/first-acid.wav`. Expected: valid; `master == "limit"`.
- [ ] **Step 3: Iterate with the ears** (`tune analyze … --sections --png scratch/mix/after-N.png --json`, read each PNG and JSON). Adjust `mix.master.gain` first, then track gains and sends, until: integrated in −8..−6 LUFS; `true_peak_dbtp <= -1.0`; `max_reduction_db <= 9`; `bands.low_mid` above −17.5 dB (the 303 more present); drop ≥ +3 LU over breakdown; build ≥ +1 LU over intro_acid; no new `nothing changes here` note. If a target cannot be met by gains, sends, lanes or `mix.*` alone, stop and record why instead of changing tools.
- [ ] **Step 4: Compare.** If `this-is-acid` resolves: `tune compare songs/first-acid/song.toml --bars 81..96 --ref this-is-acid --ref-seconds A..B --png scratch/mix/drop-vs-ref.png --json` with the reference drop from Plan 4's session; read the PNG and verdicts. Otherwise skip and say so.
- [ ] **Step 5: Document in the song.** Replace the "Levels" note at the end of `song.toml` with the measured numbers (integrated LUFS, true peak, max reduction, per-section LUFS of build/breakdown/drop) and add short producer-facing comments at `[mix]`, the sends, the duck and the eq explaining what each does musically (the file's existing comment style).
- [ ] **Step 6: Verify and commit.** `tune validate`, `cargo test -p tune-cli --test songs`, `task verify`; `git status --short` shows only `songs/first-acid/song.toml` (nothing under `scratch/`). Commit: `feat(songs): mix first-acid with sends, ducking, eq and the limiter`.
- [ ] **Step 7: Write `scratch/mix-first-acid.md`** (not committed): commands run, before/after table (integrated, TP, max reduction, bands, `low_end.ratio_db`, per-section LUFS and `delta_lu`), the compare verdicts, what the PNGs show, and each change made with its reason.
- [ ] **Step 8: Session gate (producer).** The producer listens to the render (outside this session; never played by Claude) next to the reference and says whether the 303 now sits right against the kick, whether the delay and reverb are too much, and whether the limiter pumps. Record the verdict as input for Plan 6.
