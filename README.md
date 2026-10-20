# tune-ai

A Rust music workstation. The first milestone is a faithful port of the
Open303 TB-303 emulation, rendered to WAV through the `tune` command line tool.

## Prerequisites

- macOS on Apple Silicon
- Rust (stable, edition 2024) and cargo
- [go-task](https://taskfile.dev) (`task`)

## Usage

```sh
task setup    # install the git pre-commit hook (once per clone)
task verify   # format check, clippy, tests, privacy check
task demo     # render and play the demo 303 line
```

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
