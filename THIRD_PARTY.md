# Third-party software

## Open303

- Project: Open303, a TB-303 emulation by Robin Schmidt
- URL: https://github.com/RobinSchmidt/Open303
- Commit: `313bf0d9ade7c1dcb6b3a74f5ea1780a29d70074`
- License: MIT, Copyright (c) 2009 Robin Schmidt. Full text in
  [`third_party/open303/License.txt`](third_party/open303/License.txt).
- Vendored: `Source/DSPCode/*` and `License.txt`, copied unmodified into
  `third_party/open303/`.

### Use in this repository

- `crates/open303-ref` compiles the vendored C++ as a test-only reference
  (not published, used by `tune-dsp` only as a dev-dependency).
- `crates/tune-dsp/src/acid303/*` are Rust ports of the Open303 modules and
  are checked sample-for-sample against the C++ reference. Ported files:
  `math.rs`, `leaky_integrator.rs`, `one_pole.rs`, `biquad.rs`,
  `elliptic_qb.rs`, `decay_env.rs`, `analog_env.rs`, `wavetable.rs`,
  `blend_osc.rs`, `teebee.rs`, `voice.rs`, `pattern.rs`, `player.rs`.
