# Contributing to TuneAI

Thanks for looking. The project is paused by its author, so replies may be
slow. Forks are welcome, and so are pull requests here.

## Before you start

- Read the [README](README.md) for the vision and
  [`docs/roadmap.md`](docs/roadmap.md) for the phases. Pick work that fits a
  phase, or open an issue to propose something.
- For anything larger than a bug fix, open an issue first and describe the
  approach. For new features, write a short spec and plan under `docs/superpowers/`
  (look at the existing ones for the format), or have your coding agent write
  them. [`AGENTS.md`](AGENTS.md) explains how to work with an agent here.

## Setup

```sh
task setup    # install the pre-commit hook (once per clone)
task verify   # must pass before every commit and pull request
```

You need Rust (stable, edition 2024), [go-task](https://taskfile.dev), and
Node.js with pnpm 9. See the README for platform notes.

## Rules that hold for every change

- **No audio in git.** No renders, samples or reference tracks. The pre-commit
  privacy check blocks them, along with home paths and email addresses.
- **Keep the 303 bit-faithful.** The Rust port must match the Open303 C++
  reference. If a parity test fails, find the cause. Never loosen a tolerance.
- **Deterministic output.** Same inputs, bitwise-identical audio. No unseeded
  randomness or time-dependent behaviour.
- **No GPL code.** Do not copy from GPL projects (for example JC303). Open303 is
  MIT and credited in [`THIRD_PARTY.md`](THIRD_PARTY.md). Add any new
  third-party code there with its license.
- **Humans and agents get the same powers.** A new UI action needs an agent
  equivalent in the action layer (`crates/tune-studio/src/action/`).

## Commits and pull requests

- Small, atomic commits, one concern each, conventional-commit messages
  (`feat(engine): ...`, `fix(ui): ...`, `docs: ...`).
- Add tests with the change. Rust crates have unit and integration tests; the
  frontend uses Vitest.
- For a UI change, say what you looked at and include a screenshot in the pull
  request description (not in the repo).
- Describe what changed and why, and how you verified it.

## License

By contributing you agree that your work is licensed under the [MIT License](LICENSE).
