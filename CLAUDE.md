# tune-ai

Rust music workstation: TOML song files, 909-style drums, a faithful Rust port
of the Open303 TB-303 emulation, and a sample-accurate engine (offline render
and lock-free live playback) driven through the `tune` CLI.

## Layout

- `crates/tune-dsp` - DSP library (`param`, `acid303/*`, `drums/*`, `fx/*`: eq, delay, reverb, duck, limiter). No Tauri, UI,
  MCP, `cc` or `open303-ref` dependencies (`open303-ref` only as a dev-dependency).
- `crates/tune-song` - song file model: parse, validate (positional errors),
  canonical write, file watcher. Headless.
- `crates/tune-engine` - compiles a song and renders it: offline (`render`) or
  live (`live`, lock-free swap for hot reload). Headless.
- `crates/tune-studio` - the studio core: in-place song edits by path
  (`SongPath`, comments kept), the edit log, undo, external-change merge, and
  the `Studio` actor the app talks to. Headless.
- `crates/tune-cli` - the `tune` binary: `validate`, `info`, `params`,
  `render`, `play`, `render-303`, `set`, `changes`, `serve`, `analyze`,
  `compare`, `refs`. `ctrlc`, `tokio` and `axum` live only here (and in the
  Tauri host).
- `crates/tune-analysis` - Claude's ears: decode, loudness (EBU R128), spectrum,
  spectrogram PNG, comparison against a reference. Headless, no unsafe.
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
   in question. Read `master` (`clip` or `limit`), `peak_dbfs`,
   `pre_clip_peak_dbfs` and `max_reduction_db`. In clip mode (the default)
   the soft clipper starts shaping at -6 dBFS; a pre-clip peak above -1 dBFS
   means it is limiting hard, so lower gains. Text mode prints `warning:
   master clipper engaged on N samples` only in that case and `note: soft
   clipper shaped N samples` when only the -6 dBFS knee is crossed (stderr).
   That knee note/warning is clip mode only. In limit mode the text line ends
   "limiter reduced peaks by R dB" instead of the pre-clip peak,
   `max_reduction_db` is R (0 in clip mode), and stderr gets `note: limiter
   reduced peaks by up to R dB` for R > 0, or `warning: ... lower
   mix.master.gain` for R > 9.

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

### Mixing

Signal flow, fixed: instrument -> fx (distortion, eq) -> duck -> gain/pan ->
sends -> delay and reverb buses -> master gain -> clip or limit.

- Strip params live in the track's `params` table: `send.delay`, `send.reverb`
  (dB, -60 = off), `duck.depth` (dB), `duck.release` (ms). EQ is an fx entry:
  `fx = [{ type = "eq", params = { "eq.low_cut" = 85 } }]`
  (`eq.{low_cut,mid_freq,mid_gain,mid_q,high_gain,high_cut}`).
- `[mix]` holds the master mode, the duck source and the shared bus params
  (`delay.*`, `reverb.*`, `master.*`):

  ```toml
  [mix]
  master = "limit"            # "clip" (default, soft clipper) or "limit"
  duck_source = "kick"        # optional; default = the first kick track
  params = { "delay.feedback" = 40, "reverb.size" = 30, "master.gain" = 6 }
  ```

- Lanes: `<track>.send.delay` (delay throws), `<track>.duck.depth`,
  `mix.<param id>` (e.g. `param = "mix.reverb.size"`).
- A delay/reverb bus is built only if some track sets `send.delay` /
  `send.reverb` explicitly (even at -60) or a lane drives it. Ducking follows
  the source's hits even if it is muted; no hits, no ducking.
- `master = "limit"` is for loud masters; `master.ceiling` defaults to -1.5 dB.
  Its 84-frame lookahead (1.75 ms at 48 kHz) is not compensated.
- A studio send knob on a track with no static send and no lane is silent
  until released. Stems carry dry tracks only.
- A hot reload keeps the reverb tail and the limiter. Only a bpm change or a
  delay bus appearing or going restarts the delay (its echoes stop). A
  master-mode switch crossfades clipper and limiter over 10 ms, with no silent
  frames.

Every command takes `--json`; exit code 2 means failure. `tune play` is for the
producer: it plays through the speakers and reloads the file when it changes.
Claude may run `tune play <song> --device null --json` to check a reload (no
sound), never the default device. Never commit renders or any audio.

### Listening (Claude cannot hear)

`tune analyze` and `tune compare` turn a render into numbers and a picture.
They are hints about the mix, not verdicts on the music: the producer's ears
are the final judge.

1. `tune info <song> --json` for the section map (bars are 1-based).
2. `tune analyze <song> --sections --png --json` for the whole song, or
   `--bars A..B` for a passage. Put INPUT before `--png` (or write
   `--png=PATH`): a bare `--png` takes an optional value and would swallow a
   following path. Bare `--png` writes `scratch/<name>[-bars-A-B].png`.
3. Read the PNG with the image tool (1600x856). Bright = loud. x is time, with
   1-based bar numbers in the ruler and white section lines. y is 30 Hz to
   16 kHz, log scale. The strip below shows each band's share of the total
   over time.
4. Read the JSON. Per-section `integrated_lufs` and `delta_lu` are the energy
   curve: a drop should be louder and brighter than its breakdown. A
   `nothing changes here` entry in a section's `notes` is a problem.
5. `tune compare <song> --bars <drop> --ref <name> [--ref-seconds A..B] --png --json`.
   Find the reference's drop first with `tune analyze --ref <name> --png`
   (never look up its path), then compare our drop with it. Give exactly one of `--ref <name>` or a
   reference path (`tune compare <song> <path>`). The compare PNG is 1600x1712:
   ours on top, the reference below, same frequency axis and colour scale.
6. For -8..-6 LUFS, set `master = "limit"` and raise `mix.master.gain`; keep
   `max_reduction_db` under about 9.
7. Change the song (`tune set`), re-render, repeat.

Input is a song file (rendered in memory) or audio (WAV, MP3, FLAC, AAC; at
most 2 channels). `--song <song> --from-bar N` maps an audio file onto a song's
bars; audio past the song's last bar is left out with an entry in `notes`
(`note:` on stderr in text mode), and a section cut by the file's end is
`partial`. Failures exit 2 with a message.

| Key | Meaning |
| --- | --- |
| `loudness.integrated_lufs` | whole-passage loudness; acid target -8 to -6 |
| `loudness.true_peak_dbtp` | at most -1; above risks distortion after encoding |
| `loudness.lra_lu` | loudness range; `null` on passages under about 3 s |
| `loudness.crest_db`, `plr_db` | peak vs RMS / vs loudness; lower = more squashed |
| `targets` | the acid targets and whether `integrated` / `true_peak` meet them |
| `bands.{sub,low,low_mid,mid,high,air}` | dB vs the total; independent of loudness |
| `low_end` | `kick_region_db` (20-90 Hz) vs `bass_region_db` (90-250 Hz), `ratio_db` = kick - bass |
| `stereo.low_correlation` | L/R correlation below 120 Hz; 0.9 or more = mono low end |
| `stereo.side_to_mid_db` | stereo width |
| `brightness.*` | whole-mix spectral centroid: `median_hz`, `p10_hz`, `p90_hz`, `profile_hz`; `range_oct` = log2(p90 / p10), how far the centroid moves. It tracks a filter sweep only where the 303 dominates the mix; under a loud kick it barely moves (0.17 oct across a 400-960 Hz cutoff ramp in the first session) |
| `series` | 1 s grid: `short_term_lufs`, `centroid_hz` |
| `sections[]` | `integrated_lufs`, `short_term_max_lufs`, `delta_lu`, `bands`, `centroid_median_hz`, `notes` |
| `deltas`, `verdicts` (compare) | ours minus reference, plus one plain-language line each |

A quantity that does not exist (silence, too short) is `null`; a mono
signal's `side_to_mid_db` floors at -200. In `compare`, a band delta is `null`
with a "not compared" verdict when one side lacks content there, and the width
delta is `null` when exactly one side is mono, with a verdict naming that side
(0 when both are). A side 60 dB or more below the mid counts as mono: a mono
master in a lossy stereo file keeps a faint side residue, so its
`side_to_mid_db` reads about -90, not -200. A side with no mid at all (left
and right in anti-phase, +200) is not compared either.

References live outside git. Put `[refs]` `<name> = "~/path/file.mp3"` in
`tune.local.toml` at the repo root (gitignored) or drop files into `refs/`
(name = file stem); `tune refs --json` checks that each resolves. It echoes
the configured paths and also the expanded `resolved` path and repo root, so
keep its output out of reports and anything committed. With `--ref <name>`,
`tune compare` shows only the configured path (`~/...` or `refs/<file>`) in
`source.path` and in error messages, never the expanded one; a reference given
as a path is echoed as typed, so prefer `--ref <name>`. Never read
or print `tune.local.toml` itself. Never commit references, PNGs or the local
config.

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
  | `tracks.<t>.send.delay` / `send.reverb` / `duck.depth` / `duck.release` | dB / dB / dB / ms |
  | `tracks.<t>.eq.<name>` | `low_cut`, `mid_freq`, `mid_gain`, `mid_q`, `high_gain`, `high_cut`; the track needs an `eq` fx entry |
  | `mix.<param id>` | `delay.*`, `reverb.*`, `master.gain` / `ceiling` / `release`, e.g. `mix.master.gain=6` |
  | `mix.master` | `clip` or `limit` |
  | `mix.duck_source` | track id; empty = first kick |
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
