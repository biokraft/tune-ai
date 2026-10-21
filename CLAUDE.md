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
- `crates/tune-cli` - the `tune` binary: `validate`, `info`, `params`,
  `render`, `play`, `render-303`. `cpal` and `ctrlc` live only here.
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
