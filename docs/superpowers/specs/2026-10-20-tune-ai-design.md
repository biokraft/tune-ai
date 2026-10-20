# tune-ai — Design

Date: 2026-10-20
Status: draft, awaiting approval

## 1. Intent

tune-ai is a desktop music workstation built for exactly two users: a human
producer and Claude (running in Claude Code). Together they compose finished
electronic tracks — acid techno first, later hardtekk/tekkno and uptempo
hardcore.

The producer is a beginner at music production and an experienced software
developer. The project has two equal purposes:

1. **Make music.** Finished, club-playable tracks, not loops.
2. **Learn production.** The producer learns synthesis, arrangement and mixing
   by doing, with Claude explaining what each control does musically while
   they work.

The tools are built so that both partners can use them. The human works with
knobs, step grids, samples and an arrangement view. Claude works with text
files, a CLI and an MCP server. Both act on the same song, and each can see
what the other changed.

### Why it matters

Claude cannot hear. Every design choice that lets Claude still take part —
offline rendering, audio analysis, a text song format, a log of the human's
edits — exists to close that gap. The human's ears are the final judge of
groove and "does it slap"; the tools let Claude catch the objective problems
(clipping, mud, wrong tempo, sections that never change) and propose musical
changes in a language both understand.

### The heart: the 303

The producer's favourite sound is the wobbly, squelchy TB-303 acid line.
Everything else in the project supports it. The 303 voice is built first,
polished hardest, and its controls get the most prominent place in the UI.
Until the 303 squelches like the real thing, nothing else matters.

### A typical session (described by the producer, details are the designer's guess)

1. The producer describes what they want in the Claude Code chat ("rolling
   acid at 145, dark, a line that slowly tears open").
2. Claude writes a first draft into the song file: drums, 303 pattern,
   arrangement.
3. The desktop app hot-reloads and plays it. The producer twists knobs, picks
   and distorts samples, rearranges sections.
4. Claude reads the producer's edits, renders sections offline, analyses them,
   explains what changed, and proposes or makes further edits on request.
5. The session ends with the song file committed and a rendered WAV the
   producer can listen to anywhere.

Chat happens in the Claude Code terminal next to the app. The app does not
embed a chat in v1 (designer's call — avoids API keys and duplicate UI).

## 2. Goals

- **G1. First milestone track.** One finished ~4-minute acid techno track in
  the vein of Mark Blair – "This Is Acid, Man": hard driving kick, rave-style
  drums, a 303 line that starts closed and slowly opens, a breakdown, a drop,
  DJ-friendly 32-bar intro and outro, ~145 BPM. Rendered to WAV at a
  loudness and tonal balance comparable to the reference.
- **G2. A 303 that is the real thing.** A Rust port of Open303 with correct
  accent (including stacking accent sweep), slide (gate held, pitch glide,
  no envelope retrigger), and the classic controls: tune, cutoff, resonance,
  env mod, decay, accent. Followed by a distortion stage.
- **G3. One engine, two modes.** The same Rust engine plays in realtime and
  renders offline to WAV. Offline output must match realtime output sample
  for sample given the same song state (deterministic), so what the human
  hears is what Claude measures.
- **G4. Song as text.** The whole song — tempo, instruments, parameters,
  patterns, arrangement, automation — lives in a human- and LLM-editable text
  file that is diffable in git and hot-reloads in the running app.
- **G5. Claude can work alone in the loop.** Through a CLI and an MCP server,
  Claude can edit the song, render any section, analyse it, and compare it
  against a reference track without the GUI running.
- **G6. Claude sees what the human did.** Every human edit in the app
  (knob moves, step toggles, sample changes, rearrangements) is persisted and
  logged so Claude can read what changed and when.
- **G7. Hands-on desktop app.** Tauri app with transport, 303 panel, step
  sequencer, drum grid, arrangement view, sample browser, and mixer basics.
- **G8. Learning built in.** Each control has a short "what this does
  musically" explanation available in the UI. The research docs in
  `docs/research/` are the source for this content.
- **G9. Modular.** New instruments, effects and analysers are added as
  self-contained modules behind stable traits, without touching unrelated
  code.

## 3. Non-goals (for now)

- Live coding or performing on stage.
- DJ features (later, maybe).
- Hosting third-party plugins (VST3/AU/CLAP). Possible later module; the
  engine design should not rule it out.
- Physical MIDI controller support in v1. The producer has none yet. The
  parameter system must be designed so controller mapping can be added later
  without restructuring (every automatable parameter has a stable ID and a
  normalised range).
- Hardtekk, tekkno and uptempo-specific tooling (clipped tonal kicks, offbeat
  bass, layered uptempo kicks). Comes after G1.
- Multi-user, cloud sync, distribution to other people, Windows/Linux
  support.
- An in-app chat with Claude.

## 4. Anti-goals (designer's guesses, unconfirmed by the producer)

The project fails, even if it technically works, if:

- The 303 sounds like a generic synth with a low-pass filter. No wobble,
  no squelch.
- The producer spends sessions fighting setup, crashes or audio glitches
  instead of making music.
- Claude edits feel like a black box: the producer cannot see or understand
  what Claude changed.
- The human's tweaks get lost or overwritten (e.g. Claude rewriting a file
  the app just changed, or a hot-reload discarding an unsaved knob move).
- It only ever makes 8-bar loops and never a finished track.
- The producer learns nothing because Claude does everything.

## 5. Constraints

- **Platform:** macOS on Apple Silicon. Nothing else needs to work.
- **Stack (the producer's choices):** Rust for the engine and backend,
  mirroring the producer's sagepad repo: Tauri 2, React 18 + TypeScript +
  Vite, Tailwind + shadcn/Radix in a separate `packages/ui`, pnpm workspace,
  Cargo workspace, Taskfile, vitest, eslint/prettier, conventional commits.
  Custom controls that shadcn lacks (knobs, step grids, waveform and
  arrangement lanes) are built in canvas or SVG inside `packages/ui`.
- **303 source (the producer's choice):** port Open303's DSP
  (https://github.com/RobinSchmidt/Open303, MIT) to Rust. Keep the MIT notice
  and attribution in a `NOTICE`/`THIRD_PARTY` file. JC303
  (https://github.com/midilab/jc303) may be consulted as a reference for
  sequencer behaviour, but its GPL code must not be copied into this
  MIT-licensed repo.
- **License and visibility:** the repo stays MIT-licensed but becomes
  **private** (the producer's choice), and song files live in it (see §6.3a).
- **No copyrighted or personal data in git.** Reference tracks (the
  purchased "This Is Acid, Man" MP3 and any later ones) and sample libraries
  are never committed. They live outside the repo or in gitignored
  directories (`refs/`, `samples/user/`), configured by a local, gitignored
  settings file. No absolute home paths, emails, real names or
  machine-specific data in committed files. An automated privacy check
  (script wired into `task verify` and a pre-commit hook) blocks audio files
  and personal data patterns from being committed. The check uses generic
  patterns, never the sensitive literals themselves.
- **Git:** atomic commits, one concern per commit. GitHub actions only as the
  bot account per the producer's global rules.
- **Repo reset (the producer's choice):** tag the current `main` as
  `legacy-tidalcycles` and push the tag, then one normal commit on `main`
  removes the old TidalCycles content. No force push. LICENSE is kept.

## 6. Architecture (decided parts)

### 6.1 Workspace layout

A Cargo workspace plus a pnpm workspace, sagepad-style:

```
apps/desktop/          Tauri app (React frontend + src-tauri host)
packages/ui/           React components: shadcn wrappers + custom knobs,
                       step grid, arrangement lanes
crates/
  tune-core/           song model, parameter IDs and ranges, timing math
  tune-song/           text song format: parse, validate, serialise,
                       file watching / hot reload
  tune-dsp/            voices and effects behind common traits
                       (acid303, drum synths, sampler, distortion, EQ,
                       delay, reverb, ducking, clipper/limiter)
  tune-engine/         sequencer, arrangement playback, mixer;
                       realtime (cpal) and offline render paths
  tune-analysis/       loudness, spectrum, spectrogram images,
                       reference comparison
  tune-cli/            headless commands for Claude and scripts
  tune-mcp/            MCP server exposing the CLI capabilities as tools
```

Crate names and exact boundaries are the builder's to refine. The
non-negotiable boundary: DSP and engine crates have no dependency on Tauri,
the UI or the MCP server, so they run headless in tests, the CLI and the app
alike.

### 6.2 One engine, realtime and offline

- Realtime: `cpal` output stream driven by the engine.
- Offline: the same engine code run in a loop writing to WAV (`hound`),
  much faster than realtime.
- Determinism: given the same song file and start position, both paths
  produce identical samples. Any randomness (humanise, noise) is seeded from
  the song file.
- Render targets: full mix, per-track stems, any bar range.

### 6.3 Song file is the shared source of truth

- One text file per song (plus referenced sample paths). Format choice is the
  builder's (TOML, RON, KDL or a small custom notation), subject to these
  requirements:
  - Comfortable for an LLM to write and for a human to read in a diff.
  - Supports comments.
  - Compact pattern notation for 303 steps: per step a note (or rest/tie),
    accent flag, slide flag, octave shift. Sixteen steps must fit in a line or
    two, e.g. something in the spirit of `C2 . C3!a D#2~ ...`.
  - Drum patterns as compact step strings.
  - Arrangement as sections with bar counts that reference patterns.
  - Automation lanes (e.g. cutoff ramp over 32 bars) as breakpoints.
  - Stable parameter IDs shared with the engine and the UI.
- The running app watches the file and hot-reloads on change without
  stopping playback, keeping the playhead.
- Human edits in the app are written back to the same file. While a knob is
  being dragged, values go straight to the engine at audio rate; the file
  write happens on release (debounced), so the file never lags far behind
  what is heard.
- **Knob recording (the producer's choice).** Knob movements can be recorded
  into automation lanes, DAW-style: the producer arms recording, twists
  (e.g. cutoff over 32 bars), and the movement becomes part of the song.
  Recorded gestures are thinned to breakpoints before being written, so the
  song file stays readable and diffable. Without recording armed, a knob on
  an automated parameter overrides the lane only while held, then the lane
  resumes; a knob on an unautomated parameter sets its static value. Write
  modes beyond this (touch/latch variants) are the builder's call. The song
  format and parameter model must support recorded lanes from stage 4 on.
- Conflict rule: whoever wrote last wins, but no edit is ever silently lost.
  The app must detect an external change arriving while it has an unwritten
  local change and resolve it without discarding either side (exact strategy
  is the builder's, e.g. merge by parameter ID, since edits rarely touch the
  same parameter).

### 6.3a Where songs live

Songs live in this repo under `songs/<song-name>/` (the producer's choice,
with the repo made private). Each song folder holds the song file and its
renders directory (renders gitignored). Committed song files are the durable
history of how a track grew. Reference tracks and sample audio stay
gitignored regardless (copyright and size); songs refer to samples by paths
relative to a configured, gitignored sample root, never absolute paths.

### 6.4 Edit log

- Every human edit in the app is appended to a per-song edit log (e.g. JSONL
  next to the song file): timestamp, parameter or pattern ID, old value, new
  value, source (`human` / `claude`).
- Claude reads it through the CLI/MCP ("what did you change since I last
  looked?"). This is how Claude "sees" the knobs.
- The edit log is local session data and gitignored by default; the song
  file history in git is the durable record.

### 6.5 Claude's interface: CLI first, MCP on top

- `tune-cli` offers headless commands; `tune-mcp` exposes the same
  capabilities as MCP tools so Claude can call them directly from Claude
  Code. Initial capability set (names illustrative):
  - `render` — song or bar range → WAV (mix and/or stems)
  - `analyze` — WAV or section → loudness (integrated LUFS, true peak),
    band energy, spectral centroid over time, spectrogram PNG
  - `compare` — our render vs a reference track: loudness, tonal balance,
    low-end ratio, side by side spectrograms
  - `changes` — read the edit log since a marker
  - `validate` — check the song file parses and references resolve
  - `play` — ask the running app to play from a bar (optional when the app
    is running)
- Song edits are plain file edits by Claude; the MCP server need not wrap
  them, but may offer convenience tools (e.g. set a parameter by ID).
- When the app is running, the MCP server may query it for live state
  (playing or not, playhead position). The transport for that is the
  builder's choice (local socket, localhost HTTP). Everything except `play`
  and live state must work with the app closed.

### 6.6 Analysis ("Claude's ears")

- Decode MP3/WAV/FLAC references (e.g. `symphonia`).
- Loudness per EBU R128 / ITU-R BS.1770 (integrated LUFS, short-term,
  true peak).
- Spectrogram images per section, readable by Claude as PNG.
- Comparison report against a reference track as text Claude can read.
- Genre targets from the research: acid masters around −8 to −6 LUFS
  integrated, true peak ≤ −1 dBTP.

### 6.7 Sound palette for milestone G1

- **acid303** — Open303 port: oscillator (saw/square blend), 303 filter,
  filter envelope with accent path and stacking accent sweep, amp envelope,
  slide glide (Open303 default ~60 ms), Open303 default parameter values as
  starting point (cutoff 1000 Hz, env mod 25%, decay 1000 ms, accent decay
  200 ms, amp decay 1230 ms — see `docs/research/2026-10-20-genres.md`).
- **Distortion** after the 303 (several flavours: soft clip, hard clip,
  overdrive-style).
- **Drums** — synthesised 909-style kick, clap, open/closed hats, ride;
  plus a sampler that plays user WAV samples with pitch, start, decay,
  reverse and distortion so the producer can pick and mangle samples.
- **Effects** — EQ, dotted-eighth delay, short reverb, kick-triggered ducking
  (sidechain), master clipper and limiter.
- **Mixer** — per track gain, pan, mute/solo, sends to delay/reverb.

### 6.8 Desktop app (v1 screens)

- **Transport:** play/stop, BPM, loop range, position.
- **303 panel (centerpiece):** big knobs for tune, cutoff, resonance, env
  mod, decay, accent, distortion; waveform/filter visual feedback is nice to
  have. Each knob has a hover/side explanation.
- **303 step sequencer:** 16 steps (pattern length configurable), per step
  note, octave, accent, slide; classic 303 workflow feel.
- **Drum grid:** per-instrument step rows.
- **Arrangement view:** sections on a timeline, drag to reorder/resize,
  automation lanes (draw a cutoff ramp).
- **Sample browser:** list user samples from the configured folder, audition,
  assign to a sampler track.
- **Mixer:** compact strip per track.

Visual design direction is the builder's, within the sagepad component
conventions. It should feel like an instrument, not a form.

## 7. Delivery order

Build in this order so each stage is usable and the producer hears the 303
early:

1. **Repo reset + skeleton.** Workspaces, Taskfile, CI-style local checks,
   privacy check, gitignore for audio/refs/samples, license notice for
   Open303.
2. **acid303 voice, headless.** Port, unit tests, CLI renders a pattern to
   WAV that the producer can play with `afplay`. Verification below.
3. **Analysis + reference compare.** Claude's loop works end to end:
   edit → render → analyse → compare to "This Is Acid, Man".
4. **Song format + drums + effects + arrangement, headless.** A full track
   renders from a song file.
5. **Desktop app: realtime playback, hot reload, 303 panel and sequencer.**
6. **Edit log, write-back, MCP server.**
7. **Drum grid, arrangement view, sample browser, mixer.**
8. **Make the milestone track together (G1).**

Stages 2–4 alone already let Claude draft tracks; stage 5 is where the
producer gets hands on. Re-ordering within this is the builder's call, as long
as the 303 comes first and headless rendering exists before the GUI.

## 8. Verification

- **303 port correctness:** compare the Rust port's output against the
  original Open303 C++ on identical note/parameter sequences (e.g. compile
  the C++ in a dev-only test harness and compare buffers within a small
  tolerance), plus behaviour tests for accent stacking, slide without
  retrigger, and glide timing. Final check is the producer's ears.
- **Determinism:** realtime path (driven in a test without a device) and
  offline render produce identical buffers.
- **Hot reload:** editing the song file while playing changes the sound
  within a bar without dropouts or playhead reset.
- **No lost edits:** concurrent app edit + external file edit both survive.
- **Analysis sanity:** LUFS of known test signals matches reference values.
- **Privacy check** fails on a staged audio file or home path.

## 8a. Designer's calls (made without the producer; flag if wrong)

- **When is the 303 "done"?** Two layers. (1) A faithful port: buffer parity
  with Open303 C++ at the same settings. This parity test is permanent and
  guards the default voice. (2) Voicing: the producer's ear wins. Any tweak
  beyond Open303 (filter variants, oversampling, accent-sweep curve,
  Devil-Fish-style mods) is added as an extra parameter or mode whose default
  keeps parity, never by changing the faithful path. Stage 2 exits when the
  parity tests pass and the producer, A/B-ing a rendered pattern against the
  reference track, says it squelches.
- **Claude's edits are visible and undoable.** The app highlights what an
  external edit changed (parameters, steps, sections) and names the source.
  Undo/redo exists from the first app stage and covers the producer's own
  edits and incoming external edits alike. Edit log, write-back, hot reload
  and undo share one command model from the start, so undo is not retrofitted.
- **Writers of the song file.** The app, Claude, the producer in a text
  editor, and git (checkout, revert) may all write it. Claude's edits go
  through a CLI/MCP call that tags the source as `claude` (plain file edits
  remain possible and are logged as `external`). Any change the app did not
  make and that is untagged is `external`. A change that replaces most of the
  file (e.g. git checkout) is treated as a whole-song replace: the app
  reloads it, and unwritten local knob moves are offered for re-apply instead
  of merged silently.

## 9. Left to the builder

- Song file syntax (within §6.3 requirements).
- Exact crate boundaries and trait design (within §6.1's headless rule).
- DSP libraries beyond the 303 (e.g. `fundsp` vs hand-written), as long as
  determinism holds.
- App↔MCP live-state transport.
- Conflict-merge strategy details.
- Visual design.
- Test framework details beyond §8.

## 10. Research

- `docs/research/2026-10-20-music-code-tools.md` — music-as-code tools,
  code-to-DAW bridges, AI integrations, candidate stacks.
- `docs/research/2026-10-20-genres.md` — acid, hardtekk/tekkno, uptempo:
  BPM, arrangement, 303 mechanics, mixing, references, learning path.

## 11. Open decisions for the producer

None. Before any song is committed, the repo must be switched to private
(the producer does this; the bot account lacks admin rights).
