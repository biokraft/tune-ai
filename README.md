# tune-ai

A Rust music workstation. The first milestone is a faithful port of the
Open303 TB-303 emulation, rendered to WAV through the `tune` command line tool.

## Prerequisites

- macOS on Apple Silicon
- Rust (stable, edition 2024) and cargo
- [go-task](https://taskfile.dev) (`task`)
- Node.js and pnpm 9 (for the studio app)

## Usage

```sh
task setup    # install the git pre-commit hook (once per clone)
task verify   # format check, clippy, tests, privacy check
task demo     # render and play the demo 303 line
```

## Songs

A song is one TOML file: tracks, patterns and an arrangement of sections with
parameter automation. See `crates/tune-song/tests/fixtures/mini.toml` for a
small one, and `songs/first-acid/song.toml` (`task first-acid`) for a full
4-minute acid track with notes on each musical choice.

```sh
cargo run --release -p tune-cli -- validate path/to/song.toml
cargo run --release -p tune-cli -- info path/to/song.toml
cargo run --release -p tune-cli -- render path/to/song.toml --bars 1..4 --out scratch/part.wav
cargo run --release -p tune-cli -- play path/to/song.toml --loop 1..4
```

`play` uses the default audio device and hot-reloads: save the file and the
change is heard at the next step without stopping. A file with errors prints
them and the previous version keeps playing. `--device null` runs without
sound. Every command takes `--json`.

## Mixing

A track can carry an EQ, send to a shared dotted-eighth delay and a short
reverb, and duck under the kick. The master is a soft clipper by default;
`master = "limit"` switches to a true-peak limiter for loud masters.

```toml
[mix]
master = "limit"
params = { "delay.feedback" = 40, "reverb.size" = 30, "master.gain" = 6 }

[tracks.acid]
instrument = "acid303"
params = { "send.delay" = -12, "send.reverb" = -18, "duck.depth" = 6 }
fx = [{ type = "eq", params = { "eq.low_cut" = 85 } }]
```

```sh
tune set songs/first-acid/song.toml tracks.acid.send.delay=-9 mix.master.gain=8
tune set songs/first-acid/song.toml mix.master=limit mix.duck_source=kick
tune render songs/first-acid/song.toml --bars 81..96 --out scratch/drop.wav
```

`tune render` reports `max_reduction_db`, how far the limiter pulled peaks
down; keep it under about 9 dB. `tune params` lists every id and range. The
limiter delays the output by 1.75 ms (not compensated), and a hot reload that
changes the tempo or master mode, or adds or removes a delay or reverb bus,
cuts ringing delay and reverb tails.

## Open the studio

The studio is the song on screen: play it, twist the 303's knobs, edit steps
and drum hits, and rearrange sections. Every edit is written back to
`song.toml` (comments kept) 150 ms after you stop moving, so Claude and the
studio always work on the same file. When Claude edits the song, what it
changed lights up in cyan, with an Undo.

```sh
task install                                  # once: JS dependencies
task studio SONG=songs/first-acid/song.toml   # the desktop window
```

Or in a browser: run `task serve SONG=songs/first-acid/song.toml` and open
http://127.0.0.1:4580/.

| Key | Does |
| --- | --- |
| Space | Play / stop |
| Cmd+Z / Shift+Cmd+Z | Undo / redo |
| L | Loop the selected section (again to clear) |
| Knob: drag up or down | Turn it (Shift: 10x finer) |
| Knob: arrows, Page Up/Down, Home/End | Nudge (Shift: finer), big step, min/max |
| 303 step: Up/Down | Note up or down a semitone (Shift: an octave) |
| 303 step: Left/Right | Previous / next step |
| 303 step: A, S, R | Toggle accent, slide, rest |
| Arrangement: Cmd+D | Duplicate the selected section |
| Arrangement: Delete | Delete the selected section |
| Arrangement: Alt+Left/Right | Move the selected section |

Each edit is logged to `edits.jsonl` next to the song (local, gitignored);
`tune changes <song>` lists it.

## Analyse a render

`tune analyze` measures loudness, frequency balance and brightness and draws a
spectrogram; `tune compare` sets a render against a reference track. Both take
a song file or an audio file (WAV, MP3, FLAC, AAC). PNGs go to `scratch/`.

```sh
tune analyze songs/first-acid/song.toml --sections --png
tune analyze songs/first-acid/song.toml --bars 33..64 --png
tune analyze ~/Music/reference.mp3 --png
tune compare songs/first-acid/song.toml ~/Music/reference.mp3 --bars 33..64 --ref-seconds 90..150 --png
```

Put INPUT before `--png` (or write `--png=PATH`). To refer to a reference by
name, add `[refs]` `name = "~/Music/reference.mp3"` to `tune.local.toml` in the
repo root (gitignored), or drop files into `refs/`, then
`tune compare songs/first-acid/song.toml --ref name` (or
`tune analyze --ref name --png`). `tune refs` lists what is
configured. The numbers are hints; trust your ears. Reference tracks, PNGs and
the local config never go into git.

## Hear the 303

```sh
task demo
```

This renders a demo acid line to `scratch/demo.wav` (gitignored) and plays it
with `afplay`. To tweak it, call the CLI directly. `--set ID=VALUE` takes the
plain value in the knob's own unit (table below). `--sweep ID=FROM:TO` moves a
knob across the render and takes normalized values from 0 to 1. A pattern
loops to fill `--bars`.

```sh
# more resonance (80 %), filter opening over the whole clip
cargo run --release -p tune-cli -- render-303 --demo \
  --set acid303.resonance=80 --sweep acid303.cutoff=0.2:0.9 \
  --out scratch/squelch.wav

# your own pattern: one token per 16th, "." rest, "!" accent, "~" slide
cargo run --release -p tune-cli -- render-303 \
  --pattern "C2 . C2! D#2~ . C2 Bb1 . C2 . C2! . D#2 . Bb1~ C2" --bars 4 \
  --out scratch/mine.wav

# hard distortion, 24 dB of drive
cargo run --release -p tune-cli -- render-303 --demo --dist hard \
  --set dist.drive=24 --out scratch/hard.wav
```

| Id | Range | Unit |
| --- | --- | --- |
| `acid303.waveform` | 0 to 1 | saw to square |
| `acid303.tuning` | 400 to 480 | Hz |
| `acid303.cutoff` | 314 to 2394 | Hz |
| `acid303.resonance` | 0 to 100 | % |
| `acid303.env_mod` | 0 to 100 | % |
| `acid303.decay` | 200 to 2000 | ms |
| `acid303.accent` | 0 to 100 | % |
| `acid303.volume` | -60 to 0 | dB |
| `acid303.slide_time` | 0 to 200 | ms |
| `dist.drive` | 0 to 48 | dB |
| `dist.mix` | 0 to 1 | dry to wet |
| `dist.output` | -24 to 6 | dB |

To A/B against a reference track, keep the reference outside the repo and play
the two back to back:

```sh
afplay path/to/your/reference.mp3
afplay scratch/demo.wav
```

Audio never goes into git; the privacy check blocks audio files.
