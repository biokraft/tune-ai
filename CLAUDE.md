# tune-ai

Rust music workstation: TOML song files, 909-style drums, a faithful Rust port
of the Open303 TB-303 emulation, and a sample-accurate engine (offline render
and lock-free live playback) driven through the `tune` CLI.

## Layout

- `crates/tune-dsp` - DSP library (`param`, `acid303/*`, `drums/*`, `fx/*`). No Tauri, UI,
  MCP, `cc` or `open303-ref` dependencies (`open303-ref` only as a dev-dependency).
- `crates/tune-song` - song file model: parse, validate (positional errors),
  canonical write, file watcher. Headless.
- `crates/tune-engine` - compiles a song and renders it: offline (`render`) or
  live (`live`, lock-free swap for hot reload). Headless.
- `crates/tune-studio` - the studio core: in-place song edits by path
  (`SongPath`, comments kept), the edit log, undo, external-change merge, and
  the `Studio` actor the app talks to. Headless.
- `crates/tune-cli` - the `tune` binary: `validate`, `info`, `params`,
  `render`, `play`, `render-303`, `set`, `changes`, `serve`. `ctrlc`, `tokio`
  and `axum` live only here (and in the Tauri host).
- `crates/tune-audio` - audio outputs for the engine's `AudioOut`: `CpalOut`
  (default device) and `NullOut`. `cpal` lives only here.
- `apps/desktop` - the studio app (React + Vite in `src/`, Tauri host in
  `src-tauri/`). `tune serve` serves its built `dist/` to a browser.
- `packages/ui` - `@tune/ui`: the studio's components (Knob, sequencers,
  arrangement, mixer, transport) and theme tokens.
- `songs/<name>/song.toml` - committed songs. Renders go to `scratch/` or
  `songs/*/renders/` (gitignored).
- `crates/open303-ref` - dev-only C++ reference build used for parity tests.
- `third_party/open303/` - vendored Open303 sources, unmodified.
- `scripts/` - privacy check and its tests. `.githooks/` - pre-commit hook.
- `docs/` - design spec, plans, research.

## How Claude uses tune

Songs are TOML files, `songs/<name>/song.toml`. Loop:

1. Edit the song file.
2. `tune validate <song> --json`. Errors carry `line` and `column`; fix them all.
3. `tune info <song> --json` for the section, bar and time map.
4. `tune render <song> --bars A..B --json --out scratch/x.wav` for the passage
   in question. Read `peak_dbfs` and `pre_clip_peak_dbfs`. The soft
   clipper starts shaping at -6 dBFS; a pre-clip peak above -1 dBFS means it
   is limiting hard, so lower gains. Text mode prints `warning:` only in that
   case and `note:` when only the -6 dBFS knee is crossed (both on stderr).

Notation:

- Acid pattern: one token per 16th. `C2` note, `.` rest, `!` accent,
  `~` slide into the next step (same pitch ties), e.g. `C2 C2! C3~ . D#2`.
- Drum pattern: `x` hit, `X` accent, `.` rest, e.g. `x...x...x...X...`.
- A section plays patterns per track (`play = { kick = "k4" }`); bars are
  1-based and ranges inclusive in CLI flags.
- Automation: `[[sections.automation]]` with `param = "<track>.<param id>"` and
  `points = [[bar_offset, normalized_0_to_1], ...]`; offset 0 is the section
  start.
- `tune params --json` is the source of param ids, ranges and units.

Every command takes `--json`; exit code 2 means failure. `tune play` is for the
producer: it plays through the speakers and reloads the file when it changes.
Claude may run `tune play <song> --device null --json` to check a reload (no
sound), never the default device. Never commit renders or any audio.

### Working with the studio open

The producer may have the studio (app or browser) open on the same song.

- Start each turn with `tune changes <song> --since <until> --json`, passing
  the `until` from the last call (omit `--since` the first time). It lists the
  producer's knob moves and step edits (`human`), Claude's (`claude`) and
  anything else (`external`), oldest first. `--source human` narrows it.
- Edit with `tune set <song> path=value... --json` while the app is open: the
  edits are all-or-nothing, keep the file's comments, and are tagged `claude`,
  so the app rings them in cyan with an Undo. Plain file edits still work and
  show up as `external`. The value is TOML (`820`, `true`, `"hard"`); a bare
  word is a string, and an empty value clears (`sections[1].play.acid=`).

  | Path | Value |
  | --- | --- |
  | `song.bpm` | number |
  | `tracks.<t>.gain_db` / `pan` / `mute` / `solo` | dB / -1 to 1 / bool / bool |
  | `tracks.<t>.<param id>` | plain value in the param's unit, e.g. `tracks.acid.acid303.cutoff=820`, `tracks.acid.dist.drive=18` |
  | `tracks.<t>.dist.mode` | `soft`, `hard` or `overdrive` |
  | `patterns.<p>.steps` | the pattern in notation, quoted |
  | `sections[<i>].bars` / `name` | integer / string; `<i>` is 0-based |
  | `sections[<i>].play.<t>` | pattern id, or empty to stop that track there |
  | `tracks.<t>`, `sections`, `sections[<i>].lanes` | read-only (appear in `tune changes`) |

- To see the UI: `tune serve <song> --device null --port 4580` (in the
  background; `task ui:build` first), then drive Chrome with the
  `agent-browser` skill: start the agent Chrome, pick a session
  (`session id --prefix <task>`), `open http://127.0.0.1:4580/`, `snapshot`,
  interact, and `screenshot scratch/<name>.png`. Read the PNG to see the UI.
  Ctrl-C or `kill` stops the server and writes pending edits (it prints
  `stopped`). Never commit screenshots.
- `edits.jsonl` sits next to the song: local and gitignored, never committed.

## Rules

- Run `task verify` before every commit. Run `task setup` once per clone to
  enable the pre-commit privacy hook.
- Atomic commits, one concern each, conventional-commit messages.
- Privacy: never commit audio files, absolute home paths, email addresses, real
  names or other machine-specific data. `scripts/check-privacy.sh` enforces it.
  Use generic placeholders such as `~/Downloads/...` in docs.
- Never commit audio of any kind (renders, samples, reference tracks).
- Parity: the Rust port must match the C++ reference (compiled with
  `-O2 -std=c++14 -ffp-contract=off`). Module-level tolerance is 1e-12 max abs
  diff, full-voice 1e-9. If a test fails, find the cause (a missed `float` cast,
  operation order, a default); never loosen the tolerance. Mirror every C++
  `float` cast and member exactly (`as f32 as f64` where C++ narrows).
- Determinism: same inputs give bitwise-identical output; no unseeded
  randomness, no time-dependent behaviour.
- No GPL code (e.g. JC303) may be copied. Open303 is MIT and attributed in
  `THIRD_PARTY.md`.
