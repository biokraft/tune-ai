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
with `afplay`. To tweak it, call the CLI directly. Knob values are normalized
0 to 1; `--sweep ID=FROM:TO` moves a knob across the render.

```sh
# more resonance, filter opening over the whole clip
cargo run --release -p tune-cli -- render-303 --demo \
  --set acid303.resonance=0.9 --sweep acid303.cutoff=0.2:0.9 \
  --out scratch/squelch.wav

# your own pattern: one token per 16th, "." rest, "!" accent, "~" slide
cargo run --release -p tune-cli -- render-303 \
  --pattern "C2 . C2! D#2~ . C2 Bb1 ." --bars 4 --out scratch/mine.wav

# hard distortion
cargo run --release -p tune-cli -- render-303 --demo --dist hard \
  --set dist.drive=0.7 --out scratch/hard.wav
```

Parameter ids: `acid303.{waveform,tuning,cutoff,resonance,env_mod,decay,accent,volume,slide_time}`
and `dist.{drive,mix,output}`.

To A/B against a reference track, keep the reference outside the repo and play
the two back to back:

```sh
afplay path/to/your/reference.mp3
afplay scratch/demo.wav
```

Audio never goes into git; the privacy check blocks audio files.
