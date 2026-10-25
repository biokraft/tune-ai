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
  `fft.rs`, `blend_osc.rs`, `teebee.rs`, `voice.rs`, `player.rs`.
- `crates/tune-dsp/src/acid303/pattern.rs` is original tune-ai step notation
  (`AcidPattern`, parse/format). It is not a port of `rosic_AcidPattern`.

## Ooura FFT package (via Open303)

- Project: General Purpose FFT (Fast Fourier/Cosine/Sine Transform) Package
  by Takuya Ooura (`fft4g.c`)
- URL: https://www.kurims.kyoto-u.ac.jp/~ooura/fft.html
- Vendored: `third_party/open303/fft4g.c`, as shipped inside Open303,
  unmodified.
- Use: `crates/tune-dsp/src/acid303/fft.rs` is a Rust port of Open303's
  `rosic_FourierTransformerRadix2` together with the routines of `fft4g.c`
  that its real-signal path calls (`rdft`, `makewt`, `makect`, `bitrv2`,
  `cftfsub`, `cftbsub`, `cft1st`, `cftmdl`, `rftfsub`, `rftbsub`).
- Terms: the package page permits use, copying and modification for any
  purpose and without fee, and asks that a modified version carry a
  reference to the original. This port is a modified version, so the
  original is credited here and in the `fft.rs` module header.

## Manrope and JetBrains Mono (typefaces)

- Projects: Manrope, by Mikhail Sharanda; JetBrains Mono, by JetBrains
- URLs: https://github.com/sharanda/manrope and
  https://github.com/JetBrains/JetBrainsMono
- License: SIL Open Font License 1.1
- Packaged for self-hosting by the `@fontsource/manrope` and
  `@fontsource/jetbrains-mono` npm packages (no network fetches at run time).
- Use: Manrope for the interface, JetBrains Mono for live numbers. The font
  files are bundled into app builds.
