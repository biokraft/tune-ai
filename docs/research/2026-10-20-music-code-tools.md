# Music-as-code tools for a human + Claude Code composing duo

Research date: 2026-10-20. Target: composing finished acid techno, hardtekk/tekkno and uptempo hardcore tracks (about 140 to 200+ BPM), with the producer as a beginner producer and Claude Code as a terminal agent that cannot hear audio.

Method: web search, GitHub/Codeberg/PyPI APIs (star counts, last push and release dates pulled live on 2026-10-20), Homebrew metadata, and a shallow clone of the Strudel source to check its export code. Anything not confirmed from a primary source is marked **(unverified)**.

---

## 0. Key findings

1. **Offline render to WAV is the deciding criterion.** The agent's feedback loop is: write code, render a WAV faster than realtime, analyse it (numbers plus spectrogram PNGs, which Claude can read as images), revise. Tools that only play live audio make that loop slow and lossy.
2. **Strudel has real offline rendering.** Its REPL "export" tab uses an `OfflineAudioContext` (`renderPatternAudio` in `packages/webaudio/webaudio.mjs`), with start/end cycle, sample rate, polyphony and a "Multi Channel Orbits" option. It also has an `arrange()` function for song sections and a ladder filter (`ftype: 'ladder'`). It is browser-only, so headless use goes through Playwright. Upstream is openly cool on AI (see section 1).
3. **Python + DawDreamer is the strongest headless DAW-like option.** DawDreamer 0.9.0 (Aug 2026) hosts VST3/AU instruments and effects, takes MIDI, automates parameters, runs Faust, and renders offline. Wheels exist for macOS arm64 and Intel.
4. **There is now a free, open-source TB-303 plugin: JC303.** It is a JUCE port of Robin Schmidt's Open303, GPL-3.0, ships VST3/AU/CLAP/LV2 for macOS universal, includes an acid step sequencer, and was pushed on 2026-10-20.
5. **Sonic Pi 5.0.0 (Aug 2026) is the gentlest beginner path.** It has built-in `:tb303` and `:gabberkick` synths, but no official headless or offline render. Recording runs in realtime through `recording_start`/`recording_save`.
6. **TidalCycles' GitHub repo is archived.** Development moved to Codeberg (`uzu/tidal`, v1.10.3 released July 2026). The Haskell install pain the producer hit is still the main complaint.
7. **Claude has no native audio input (unverified but strongly indicated).** "Hearing" has to go through analysis: librosa/Essentia features, LUFS, spectrogram images, or a second model that accepts audio (Gemini).

---

## 1. Music-as-code languages and environments

Columns: **Install** (macOS pain), **Synth** (can it do a convincing 303 and a distorted hardstyle/uptempo kick natively), **Offline WAV**, **Arrangement** (sections over minutes), **Agent-friendly** (can an LLM write and run it headlessly).

| Tool | Status (2026-10) | Install | Synth depth | Offline WAV | Arrangement | Agent-friendly |
|---|---|---|---|---|---|---|
| **Strudel** | Active. Codeberg `uzu/strudel`, 1,264 stars, commits 2026-10-19; `@strudel/*` 1.3.0 tags; AGPL-3.0 | None (browser) or local Node dev server | Medium. Subtractive synths, ladder/24 dB filters, `distort`/`shape`/`crush`, samples, optional Csound | **Yes.** `OfflineAudioContext` export tab | `arrange()`, `seqPLoop()`, `pick`/`inhabit`; pattern-first, so long forms need discipline | Good with Playwright. JS is easy for LLMs. No native CLI |
| **TidalCycles** | GitHub archived 2025-06; Codeberg `uzu/tidal` v1.10.3 (2026-07-02) | **High** (ghcup + cabal + SuperCollider + SuperDirt) | High (via SuperDirt/SC) | Indirect (record SuperDirt, or SC NRT) | Weak natively; live-loop oriented | Poor: needs GHCi + SC running |
| **Sonic Pi** | v5.0.0 (2026-08-07), scsynth replaced by "SuperSonic"; Homebrew cask 5.0.0 | **Low** (`brew install --cask sonic-pi`) | Medium-high. Built-in `:tb303` (cutoff/res/env), `:gabberkick`, sc808 kit, FX incl. `:distortion`, `:krush` | Realtime only. `recording_start` / `recording_save` in code. No official headless mode | Ruby: plain sequential code, threads, `live_loop`; long-form is fine as script | Medium. Via OSC or community MCP servers. GUI app must run |
| **SuperCollider** | 3.14.1 (2025-11-24); Homebrew cask | Low-medium (cask; Apple Silicon fine) | **Highest**. Any synthesis: 303 (RLPF/MoogFF/diode-style), clipped kicks, waveshaping | **Yes**, true non-realtime via `Score.recordNRT` / `scsynth -N` | Full: Patterns (`Pbind`, `Ppar`, `Pseq`) or explicit Score timelines | Good: `sclang file.scd` runs headless; NRT renders fast. Steep for the producer |
| **Glicol** | Last release v0.12.5 (2022-05); last push 2025-04; "highly experimental" | None (browser) | Medium (graph DSP) | **Unverified** (no documented offline export) | Weak | Low. Stagnant; skip |
| **ChucK** | Active (pushed 2026-10-18); Homebrew 1.5.5.8 | Low (`brew install chuck`) | High (sample-accurate, STK UGens) | **Yes**: `chuck --silent` + `WvOut` renders as fast as the machine allows | Manual: explicit time (`now`) scripting | Good: single binary, plain text, deterministic |
| **Csound** | 7.0.0-beta.18 (2026-10-01); Homebrew stable is 6.18.1 | Low (brew) | **Very high** (decades of opcodes) | **Yes**, native (`csound -o out.wav file.csd`) | Score section = explicit note list; any length | Very good headless, but archaic syntax |
| **Faust** | 2.88.0 (2026-09-09); Homebrew | Low (brew) | DSP only (no sequencer). `ve.diodeLadder`, `ve.moogLadder`, `ve.korg35LPF` in faustlibraries | Via host (DawDreamer, `faust2sndfile`) | None by itself | Excellent for building instruments that DawDreamer drives |
| **FoxDot** | Dormant (pushed 2025-06) | Medium (Python + SC) | Via SC | Indirect | Weak | Medium |
| **Renardo** (FoxDot fork) | v1.0.0a22 (2026-07-02), 95 stars | Medium (Python + SC, can auto-boot SC) | Via SC | Indirect | Weak | Medium. Small project |
| **Hydra** | Visuals only; Strudel ships a `hydra` package | n/a | n/a | n/a | n/a | Not relevant to audio |

### Notes per tool

- **Strudel**
  - **Export:** the official FAQ says "Use the 'export' tab to render and download as an audio file" ([FAQ](https://strudel.cc/learn/faq/)). In source, `renderPatternAudio(pattern, cps, begin, end, sampleRate, maxPolyphony, multiChannelOrbits)` builds an `OfflineAudioContext` and downloads a WAV ([codeberg.org/uzu/strudel](https://codeberg.org/uzu/strudel), `packages/webaudio/webaudio.mjs`). Separate stems would need one export per muted/soloed part. "Multi Channel Orbits" exists, but its interaction with WAV export was not checked.
  - **AI stance:** the FAQ asks that AI discussion stay in dedicated channels and warns that ChatGPT "generally give[s] wrong answers". It says contributed tunes must not be used for training. Strudel has no built-in AI features. Its AGPL-3.0 license means any custom frontend that integrates Strudel must also be AGPL.
  - **Move to Codeberg:** confirmed by the [GitHub repo description "MOVED TO CODEBERG"](https://github.com/tidalcycles/strudel).
- **TidalCycles**
  - **Install:** the official macOS route is the tidal-bootstrap script (ghcup, cabal, Tidal, Pulsar, SuperCollider, SuperDirt; "20 - 30+ mins") ([docs](https://tidalcycles.org/docs/getting-started/macos_install)). The forum has GHC/linker build failures on recent macOS ([thread](https://club.tidalcycles.org/t/macos-ventura-installation-issues-no-fixup-chains/4923)).
  - **Recommendation:** Strudel is the practical replacement, since it is the same pattern language in JS.
- **Sonic Pi**
  - **v5:** the [release notes](https://github.com/sonic-pi-net/sonic-pi/releases/tag/v5.0.0) cover the SuperSonic engine, Link audio, MIDI clock follow, a new limiter and `set_drive!`, and fixes to `:krush` and `:gabberkick`.
  - **Gabberkick synth:** described in source as "An aggressive Gabber synth sound … Play a `:g1` with default values at about 200 bpm" ([synthinfo.rb](https://github.com/sonic-pi-net/sonic-pi/blob/main/app/server/ruby/lib/sonicpi/synths/synthinfo.rb)).
  - **Headless:** there is no official headless mode ([forum](https://in-thread.sonic-pi.net/t/how-do-you-run-sonic-pi-in-headless-mode-on-windows/393)). Scripting record over OSC was reported broken on 4.5.1 ([forum](https://in-thread.sonic-pi.net/t/play-start-stop-recording-with-sonic-pi-programmatically-e-g-from-python-script/9116)). Behaviour on v5 is **unverified**.
- **SuperCollider**
  - **NRT:** see the [NRT guide](https://docs.supercollider.online/Guides/Non-Realtime-Synthesis.html) and [Score.recordNRT](https://docs.supercollider.online/Classes/Score.html).
  - **Caveat:** a single OSC message is capped at 65,516 bytes, so large SynthDefs should be written to disk first.
  - **Python driver:** [sc3nb](https://sc3nb.readthedocs.io/) offers `Score.record_nrt` from Python.
- **ChucK:** `--silent` "runs as quickly as the machine allows" ([chuck-users 2014](https://lists.cs.princeton.edu/pipermail/chuck-users/2014-June/007654.html)). The source is dated, and current behaviour was not re-checked.
- **Glicol:** the [README](https://github.com/chaosprint/glicol/blob/main/README.md) says "highly experimental", and there are no releases since 2022.

---

## 2. Code-to-DAW bridges, renderers and instruments

### MIDI generation (Python)

| Library | Version | Use |
|---|---|---|
| [mido](https://github.com/mido/mido) | 1.3.3 | Low-level MIDI messages and files; active (2026-09) |
| [pretty_midi](https://github.com/craffel/pretty-midi) | 0.2.11 | Easy note lists and timing; active |
| [music21](https://github.com/cuthbertLab/music21) | 10.5.0 | Theory and analysis (scales, chords, voice-leading); heavy |

MIDI files are the most portable artifact: they open in any DAW, so the producer can drag them into Ableton or REAPER and learn by tweaking.

### Offline renderers (Python)

- **[DawDreamer](https://github.com/DBraun/DawDreamer)** v0.9.0 (2026-08-12)
  - **What it does:** VST/AU instrument and effect hosting, MIDI notes (`add_midi_note`), PPQN-based automation, Faust processors, graph rendering (`engine.render(seconds)`), and multi-processor output.
  - **Platform:** macOS 11+, arm64 and x86 wheels, Python 3.11 to 3.14 ([release](https://github.com/DBraun/DawDreamer/releases/tag/v0.9.0), [docs](https://dirt.design/DawDreamer/)).
  - **Licence:** GPL-3.0.
  - **Caveats:** import DawDreamer before JAX/LLVM libraries. Some plugins misbehave without their GUI **(unverified per plugin)**.
- **[Pedalboard](https://github.com/spotify/pedalboard)** v0.9.26 (2026-10-07)
  - **What it does:** loads VST3/AU instruments and effects, renders MIDI to audio, and chains effects. Very solid for effects and mastering chains.
  - **Gaps:** less sequencing and automation support than DawDreamer.
  - **Known issue:** parameters reset when rendering MIDI ([issue #339](https://github.com/spotify/pedalboard/issues/339), status not rechecked).
  - **Compatibility:** some plugins may crash Python.
- **[Surge XT](https://github.com/surge-synthesizer/surge)** 1.3.4 (Homebrew cask; repo very active)
  - **Plugin use:** free, GPL, deep (ladder/diode-ish filters, waveshapers, FM). It is a strong candidate to host inside DawDreamer.
  - **surgepy:** Python bindings exist but must be built from source with `-DSURGE_BUILD_PYTHON_BINDINGS=ON` ([surge-python](https://github.com/surge-synthesizer/surge-python)). No PyPI wheel was found.

### DAW scripting and bridges

- **REAPER** 7.82
  - **Cost:** cheap licence, with a generous trial.
  - **Scripting:** ReaScript (Lua/Python/EEL) and a large MCP ecosystem (see section 3).
  - **Command line:** a `-renderproject` flag is widely believed to exist **(unverified; no primary doc found)**.
  - **Pick if a DAW is wanted:** REAPER is the most scriptable DAW for an agent.
- **Ableton Live**
  - **Bridges:** MIDI Remote Scripts (Python), Max for Live, and [AbletonOSC](https://github.com/ideoforms/AbletonOSC) (829 stars, pushed 2025-11).
  - **Rendering:** no command-line render was found **(unverified)**. Rendering stays a GUI action, which hurts the feedback loop.
- **Bitwig**
  - **Bridges:** Java/JS controller API. Community MCP options: [WigAI](https://github.com/fabb/WigAI) (pushed 2026-10-18) and [bitwig-mcp-server](https://github.com/WeModulate/bitwig-mcp-server) (stale since 2025-04).

### 303 emulations

| Plugin | Licence/cost | Notes |
|---|---|---|
| **[JC303](https://github.com/midilab/jc303)** | Free, GPL-3.0 (engine MIT) | Open303 port. VST3/AU/CLAP/LV2, macOS universal. Built-in Aciduino-style 16-step sequencer with slide/accent. Needs `xattr` de-quarantine on macOS. **Best fit** |
| [Open303](https://github.com/RobinSchmidt/Open303) | Open source | Engine JC303 is based on; dormant since 2024 |
| AudioRealism ABL3 | Paid (historically about €95; current price unverified) ([KVR](https://www.kvraudio.com/news/32228)) | Well-regarded commercial 303 |
| TAL-BassLine-101 | Paid | **SH-101 emulation, not a 303** ([TAL](https://tal-software.com/Products/TAL-BassLine-101)). Can approximate acid but is not authentic |
| Sonic Pi `:tb303` | Free, built-in | Decent, not authentic |
| DIY in Faust/SC | Free | Saw/square into `ve.diodeLadder` with accent envelope and slide (portamento) |

### Kick design

- **Kick 3 (Sonic Academy):** released Oct 2024 at £79.95, with a 14-day trial ([KVR](https://www.kvraudio.com/news-print.php?id=62006)). It is the standard hardstyle/hardcore kick designer, but it is GUI-centric.
- **Free route:** synthesize the kick in code: a sine with a fast exponential pitch envelope plus a click transient, then heavy waveshaping/clipping, EQ, and a reverb tail for hardstyle "tok". Every one of SuperCollider, Faust, Csound, ChucK and Surge XT can do this. Sonic Pi's `:gabberkick` is a ready-made gabber kick.
- **Vital:** free tier is a good wavetable synth (GUI). Its headless compatibility in DawDreamer is **unverified**.
- **No named free kick plugin was verified**, so none is listed.

---

## 3. Existing AI/agent integrations

All are community projects. Star counts and dates come from the GitHub API on 2026-10-20.

| Project | Target | Stars | Last push | Notes |
|---|---|---|---|---|
| [ahujasid/ableton-mcp](https://github.com/ahujasid/ableton-mcp) | Ableton | 3,214 | 2026-09-22 | Remote Script socket server + Python MCP. Tracks, clips, instruments, transport. MIT |
| [uisato/ableton-mcp-extended](https://github.com/uisato/ableton-mcp-extended) | Ableton | 279 | 2026-05 | Extended fork |
| [jpoindexter/ableton-mcp](https://github.com/jpoindexter/ableton-mcp) | Ableton | 59 | 2026-05 | Claims 200+ tools, M4L |
| [Simon-Kansara/ableton-live-mcp-server](https://github.com/Simon-Kansara/ableton-live-mcp-server) | Ableton via OSC | 394 | 2025-03 | Stale |
| [williamzujkowski/live-coding-music-mcp](https://github.com/williamzujkowski/live-coding-music-mcp) | Strudel | 246 | 2026-10-19 | Formerly strudel-mcp-server. Playwright-driven strudel.cc, 28 tools, "audio analysis via Web Audio API (FFT, tempo, key)", MIDI import/export, optional Gemini assist. Beta, AGPL |
| [linxule/mcp-music-studio](https://github.com/linxule/mcp-music-studio) | ABC notation + Strudel | 75 | 2026-10-19 | Scored composition plus live mode |
| [phildougherty/strudel-mcp-bridge](https://github.com/phildougherty/strudel-mcp-bridge) | Strudel (browser extension) | 22 | 2025-10 | |
| [jeremyruppel/claude-collider](https://github.com/jeremyruppel/claude-collider) | SuperCollider | 59 | 2026-10-16 | Ships CLAUDE.md + skills, 27+ synths, 18+ FX, arrangement helpers, "tape" recording. Explicitly lists 303 acid and full arrangements as example prompts |
| [Tok/SuperColliderMCP](https://github.com/Tok/SuperColliderMCP) | SuperCollider via OSC | 26 | 2025-05 | |
| [abhishekjairath/sonic-pi-mcp](https://github.com/abhishekjairath/sonic-pi-mcp), [vinayak-mehta/mcp-sonic-pi](https://github.com/vinayak-mehta/mcp-sonic-pi) | Sonic Pi | 19 each | 2026-03 / 2025-03 | Small; need GUI running |
| [bonfire-systems/reaper-mcp](https://github.com/bonfire-systems/reaper-mcp) | REAPER | 159 | 2026-10-07 | MIDI, FX, mixing, mastering, render, audio analysis |
| [dschuler36/reaper-mcp-server](https://github.com/dschuler36/reaper-mcp-server) | REAPER (analysis) | 131 | 2026-10-16 | |
| [xDarkzx/Reaper-MCP](https://github.com/xDarkzx/Reaper-MCP) | REAPER | 103 | 2026-10-20 | Claims 181 tools |
| [shiehn/total-reaper-mcp](https://github.com/shiehn/total-reaper-mcp) | REAPER | 87 | 2026-08 | Lua bridge, tool profiles |
| [fabb/WigAI](https://github.com/fabb/WigAI) | Bitwig | 62 | 2026-10-18 | Controller extension exposing MCP |
| [hugohow/mcp-music-analysis](https://github.com/hugohow/mcp-music-analysis) | librosa analysis | 30 | 2025-08 | librosa + whisper as MCP tools |

No official Strudel AI features exist. Upstream discourages AI discussion in main channels (FAQ above). No DawDreamer MCP server was found, which is a gap the producer and the agent could fill themselves.

### How an LLM can "hear"

- **Claude audio input:** no native audio input in the Claude API. A July 2026 feature request states it cannot process audio ([claudeissues](https://claudeissues.com/issue/73566-feature-allow-claude-to-hear-and-process-audio)). **Unverified against official docs**, but consistent with everything found.
- **Spectrograms as images:** Claude Code reads PNGs. Mel spectrograms, waveform overviews and chromagrams of each section are the best direct "ears".
- **Numeric features:**
  - [librosa](https://github.com/librosa/librosa) 1.0.0: tempo/beat, onset strength, spectral centroid/rolloff, chroma, RMS.
  - [Essentia](https://github.com/MTG/essentia) 2.1b6.dev: macOS arm64 wheels exist on PyPI. Offers key/BPM, loudness and pretrained classifiers.
  - [pyloudnorm](https://pypi.org/project/pyloudnorm/) or `ffmpeg -af ebur128` for integrated LUFS and true peak.
- **Embedding models:** [LAION CLAP](https://github.com/LAION-AI/CLAP) gives text-audio similarity, so a render can be scored against "distorted hardstyle kick" or a reference track. It is research-grade and was last pushed 2025-05.
- **Audio-capable second model:** Gemini accepts audio files via the Files API or inline ([docs](https://ai.google.dev/gemini-api/docs/audio)). It can describe music, as wrapped by the live-coding-music-mcp `ai_assist` tool. Its accuracy on fine production details (kick tail, 303 resonance) is **unverified** and likely coarse.
- **Reference matching:** compare a render's spectrum, LUFS and crest factor against a commercial reference track in the same genre. This is a concrete, checkable target.
- **Research context:** Libretto (arXiv 2606.22708) gives LLM agents structural feedback on symbolic music ([pdf](https://arxiv.org/pdf/2606.22708)). CoComposer notes that MIDI-synth audio limits synthesis-heavy genres ([pdf](https://arxiv.org/pdf/2509.00132)).
- **the producer stays essential.** the producer's ears remain the final judge for groove and "does it slap". The tools catch objective problems: clipping, mud, wrong tempo, mono compatibility, sections that never change.

---

## 4. Candidate stacks

### Stack A: Python + DawDreamer + JC303/Surge XT/Faust (recommended primary)

- **Shape:**
  - A Python "score" package describes the song as sections (intro, build, break, drop, outro) of patterns at a given BPM.
  - It emits MIDI (pretty_midi) and renders per-track stems plus a mix via DawDreamer.
  - Instruments: JC303 for acid, Surge XT for leads and pads, Faust for custom kicks and distortion.
  - Mastering chain: Pedalboard effects.
  - An analysis script writes LUFS, spectra and spectrogram PNGs per section.
- **Strengths:**
  - Fully headless, deterministic and fast. Stems come free.
  - Plain Python and text in git means atomic commits per musical idea.
  - The agent can run the whole render-analyse loop alone.
  - MIDI and stems drop straight into any DAW when the producer wants hands-on work.
  - the producer learns real production concepts: synth params, gain staging, arrangement.
- **Weaknesses:**
  - the producer must build the sequencer/arrangement layer himself. That is the learning project, but it is upfront work.
  - No live auditioning while editing unless a small "render 8 bars and play" command is added.
  - Plugin quirks: GUI-less parameter setting, JC303's internal sequencer versus host MIDI, and the macOS quarantine flag.
  - GPL licence (DawDreamer/Surge/JC303) applies if the tool is distributed.
  - No existing MCP server for this stack. The agent just runs scripts, which is arguably simpler.

### Stack B: Strudel (local) + Playwright + custom frontend/MCP (best for fast sketching and learning)

- **Shape:**
  - Run Strudel locally from Codeberg, or use strudel.cc.
  - The agent writes `.js` pattern files.
  - A Playwright script loads each into the REPL and triggers `renderPatternAudio` for N cycles, which gives a WAV.
  - The same analysis script as Stack A.
  - Optionally start from [live-coding-music-mcp](https://github.com/williamzujkowski/live-coding-music-mcp) instead of writing the glue.
- **Strengths:**
  - Zero install. Instant live playback for the producer while the agent edits.
  - Tidal-style patterning (the producer already knows the concepts).
  - `arrange()` for sections. Offline export already exists.
  - JS is LLM-friendly. Huge example corpus on strudel.cc.
- **Weaknesses:**
  - Browser-only engine, so headless rendering means driving Chromium. The Playwright-to-export path is **untested** here.
  - Synthesis depth is the ceiling. A 303 via ladder filter is passable, but a really hard uptempo kick will likely need samples you design elsewhere.
  - Stems need multiple renders.
  - AGPL for any integrated frontend. Community culture is wary of AI, so don't lean on upstream for help with agent-generated code.

### Stack C: SuperCollider headless + sclang scores (deepest sound, steepest curve)

- **Shape:**
  - The agent writes SynthDefs (diode/Moog-ladder 303 with accent and slide; layered, clipped kick).
  - Arrangement as `Pbind`/`Ppar` or explicit Score timelines.
  - `Score.recordNRT` renders WAV and stems faster than realtime.
  - Optionally use [claude-collider](https://github.com/jeremyruppel/claude-collider) for live jamming and its prebuilt synths and arrangement helpers.
- **Strengths:**
  - Best synthesis quality and control of any free tool. No plugins needed. Fully scriptable.
  - Brew-installable (no Haskell). Mature (3.14.1).
- **Weaknesses:**
  - sclang is hard for a beginner to read, so the producer learns SC rather than production.
  - NRT has quirks (OSC message size limit, no server feedback).
  - The agent-written DSP can sound "programmer-y" until tuned by ear.

### Honourable mentions

- **Sonic Pi 5:** easiest start and has `:tb303` + `:gabberkick`. It would be a fine first-week toy, but there is no offline render, so it is a weak base for the agent loop.
- **Csound:** excellent offline, but archaic syntax.
- **REAPER + reaper-mcp:** pick this if the producer wants a real DAW in the loop. It is the most scriptable DAW, and command-line render is unverified.

### Suggested path

1. **Weeks 1-2:** sketch grooves in Strudel (Stack B, no install) to learn pattern thinking and genre conventions (303 accents/slides, kick-bass relationships at 150-200 BPM).
2. **Then:** build Stack A as the "real" composing engine, with JC303 for acid, a Faust/Surge kick, and the analysis loop.
3. **Throughout:** keep MIDI and stems DAW-compatible so REAPER or Ableton can come in later for mixing.
