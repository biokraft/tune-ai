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
task demo     # render the demo track (added in a later task)
```
