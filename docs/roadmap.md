# tune-ai roadmap

Written 2026-10-21 from the producer's notes after Plans 1–5 landed. Each
phase below becomes one or more specs and plans under `docs/superpowers/`
before anything is built. Phases are ordered; work inside a phase can be
reordered.

## Where we are

Plans 1–5 are merged on `main`:

- A bit-exact Rust port of Open303, 909-style drums (kick, clap, hats, ride),
  distortion, EQ, a dotted-eighth delay, a short reverb, kick-grid ducking and
  a true-peak master limiter.
- Songs as TOML: patterns, sections, automation lanes, sends, a `[mix]` table.
  The engine renders offline and plays live with hot reload, and both give
  bitwise-identical output.
- The `tune` CLI (`--json` everywhere): validate, info, params, render, play,
  set, changes, serve, analyze, compare, refs. Claude's "ears" turn renders
  into loudness, bands and spectrogram PNGs, and compare them against a
  reference track.
- A first studio UI (Tauri + React): 303 panel, step sequencer, drum grid,
  arrangement, mixer with sends, FX/master panel, edit log with Claude edits
  ringed in cyan and an Undo.
- `songs/first-acid` mixed to −8.8 LUFS, −1.5 dBTP.

Plan 6 reskinned the studio onto the Claude Design "Dusk Glass" look: a
glass transport, the timeline, a dock under the timeline with Sound, Steps
and Mixer tabs, and a toast card for Claude's edits. It added no features,
so the studio is still a working prototype until Phase 4 replaces it.

## Direction

The producer's goals for the next stage:

1. **A DAW-class UI/UX**, in the spirit of Ableton Live and FL Studio. You
   can assemble beats and songs, modulate and edit synths, and add or remove
   instruments and effects, all in a well-structured modular layout.
   Glassmorphic, modern and futuristic. The full design is worked out in
   Claude Design first.
2. **It must feel like an app, not a website.** The window never scrolls as a
   page. Panels scroll inside themselves when they must.
3. **A chat docked on the right, always available and minimisable.** It is the
   entry point for working with the agent, which can do everything a human
   can do in the UI, plus agentic work on top (render, listen, compare,
   arrange, explain).
4. **A hybrid tool.** Humans can do everything in the frontend; agents can do
   everything humans can. Every UI action has an agent equivalent, and every
   agent edit is visible and undoable in the UI.
5. **Plug and play for samples and synths.** Drop in a sample folder and it
   works. Add a synth to a track and it plays.
6. **A file browser** for samples, instruments, effects and songs, with
   preview.
7. **An agent harness the producer controls from the frontend.** The leaning
   is OpenCode, with proper use of memories and skills. Chat sessions can be
   listed, resumed, renamed and deleted.
8. **Vibe and watch.** The producer tells tune-ai what to do and the agent
   does it in front of them, on screen. Watching it happen is how the
   producer learns.
9. **Teach mode.** A slower, step-by-step mode. The agent performs one
   action at a time, highlights the control it is about to touch, and fills a
   speech bubble next to it explaining what it does and why. The producer can
   pause, step forward, ask "why?", or take over and try it themselves.
10. **UI/UX feel comes first.** The producer then learns to build acid techno
   in the new UI as its first real test. New synths come after that.

### What this changes in the spec

The design spec (`docs/superpowers/specs/2026-10-20-tune-ai-design.md`) has
to be revised before Phase 1 is planned:

- §3 lists "an in-app chat with Claude" as a non-goal. That is now a goal.
- §6.5 plans an MCP server. The producer has since chosen the CLI with
  `--json` as the agent interface; the in-app agent should drive the same CLI
  (or the same Rust core behind it).
- §6.8 lists the v1 screens. The new design replaces them.
- §4's anti-goal "the producer learns nothing because Claude does everything"
  still stands, and teach mode is the answer to it. The agent may do the
  work, but it does it visibly, step by step, explaining each move (G8), and
  the producer can take over at any step.
- §3 keeps third-party plugin hosting (VST3/AU/CLAP) a non-goal for now.
  "Plug and play synths" means tune-ai's own instrument modules at first.

## Phases

### Phase 1: Product and UX design (Claude Design)

Goal: a complete, approved design for the whole app before any UI code is
rewritten.

- **Design brief.** Users (the producer plus the agent), core workflows
  (make a beat, build a song, sound-design a synth, mix, work with the
  agent), the reference products (Ableton, FL Studio) and what to take from
  each, and the no-page-scroll rule.
- **Information architecture.** A fixed-viewport app shell with these
  regions:
  - transport and global controls on top;
  - a browser on the left (samples, instruments, effects, songs);
  - the main workspace in the centre (arrangement, pattern or clip editor,
    piano roll, step grid);
  - a device and detail area at the bottom (instrument and effect chains,
    synth editors, mixer);
  - the chat dock on the right.

  Every region resizes, collapses and remembers its state.
- **Visual language.** A glassmorphic, futuristic design system: colour,
  depth, blur, light, typography, motion and density. It also needs a
  performance budget, because heavy `backdrop-filter` in the Tauri WebView
  can cost frames next to a realtime playhead.
- **Components.** shadcn/Radix primitives for standard controls, plus custom
  audio controls: knob, fader, meter, step grid, piano roll, clip, automation
  lane, waveform, spectrum, device chain. States for each: idle, hover,
  active, automated, Claude-edited, disabled, error, empty.
- **Agent UX.** How agent edits show up: highlight, diff, undo and an
  explanation. How the chat refers to things on screen, and how the user
  points the agent at something.
- **Agent at work, on screen.** The agent's actions play out visibly, so the
  producer can watch: the knob turns, the step lights up, the clip moves.
  They are not applied silently in one jump.
- **Teach mode.** Design the step-by-step flow:
  - spotlight on the target control;
  - a speech bubble anchored next to it, written by the agent;
  - pacing controls (pause, next step, slower, faster);
  - "why?", and "let me try", which hands the step to the producer and has
    the agent check the result;
  - a recap at the end.
- **Deliverables.** A design system plus screen designs and a clickable
  prototype in Claude Design, covering every view and the main flows.
- **Exit:** the producer approves the design.

### Phase 2: Modular model and engine foundations

Goal: the song model and engine can support what the design needs, behind
stable interfaces (spec G9).

- **Devices.** Each track holds an instrument plus an effect chain, built
  from a registry of modules. Instruments and effects can be added, removed
  and reordered.
- **Generic note clips** alongside the 303 patterns and drum steps, for the
  piano roll and for future synths.
- **Clips and patterns** usable from the arrangement, with room for an
  Ableton-like session view if the design calls for one.
- **Song format v2** with an automatic migration from today's files. The
  backward-compat render pins keep v1 songs sounding identical.
- **App-wide undo/redo** across human and agent edits.
- **Action parity.** One action layer that both the UI and the agent call.
  Every action is reachable from the CLI, and a test fails when a UI action
  has no agent equivalent.
- **Exit:** a new song with a dynamically added instrument and effect chain
  renders and hot-reloads. v1 songs still render bitwise as before.

### Phase 3: Samples and the browser

Goal: samples are plug and play.

- **Sampler instrument** (spec §6.7): pitch, start, decay, reverse,
  distortion. One-shots plus simple slicing.
- **Sample library.** A configured, gitignored sample root (never
  committed). Folder scanning, tags and favourites.
- **Browser backend.** File listing, search and preview through the engine.
  Formats: WAV, AIFF, FLAC, MP3.
- **Drag and drop** from the browser onto tracks and pads, from the agent
  too: "put a 909 open hat from my library on track 4".
- **Exit:** the producer drops a sample folder in, previews, and builds a
  beat from it without touching a file path.

### Phase 4: Build the new UI

Goal: implement the Phase 1 design on the Phase 2–3 foundations, replacing
the prototype screens.

- The app shell with no page scroll, plus resizable, collapsible regions.
- Views:
  - arrangement with drawable automation lanes;
  - pattern and step editor and piano roll;
  - device rack with synth editors (the 303 first);
  - mixer;
  - browser;
  - transport.
- **Knob recording into automation lanes** (spec §6.3, the producer's
  choice), with thinning to breakpoints.
- Keyboard shortcuts and the focus model.
- **Exit:** every Phase 1 screen works against the real engine. Real audio
  is verified on the producer's machine, including a headphone unplug and
  replug.

### Phase 5: The in-app agent

Goal: the chat dock is a full agent that can operate the studio.

- **Harness choice, settled by a short spike.** OpenCode (the producer's
  leaning: open source, provider-agnostic, a server mode with an SDK,
  sessions built in) against the Claude Agent SDK (first-party skills,
  memory and tool use). The spike compares session handling, memory, skills,
  tool permissions, streaming into the UI and cost.
- **Tools.** The `tune` CLI and the Phase 2 action layer. The agent can do
  everything the UI can, including transport (play is fine in-app, where the
  producer is listening).
- **Skills.** Listening, mixing, arranging, sound design and teaching. These
  are versioned in the repo.
- **Memories.** Producer preferences, per-song notes and decisions, which the
  producer can view and edit.
- **Frontend.**
  - Chat with streaming;
  - session list, resume, rename and delete;
  - model and provider settings;
  - a permission policy (what the agent may do without asking);
  - a visible activity log of agent actions, each undoable.
- **Safety.** The agent's shell and file access is limited to the repo and
  the sample root. No credentials pass through the agent.
- **Visible actions.** The agent's edits are animated in the UI as they
  happen: knob sweeps, step toggles, and clips moving into place.
- **Teach mode.**
  - Actions run as a queue of steps, one at a time.
  - Each step carries a target (a UI element id from the action layer) and an
    explanation; the UI spotlights the target and shows the agent's speech
    bubble beside it.
  - The producer can pause, step, change the speed, ask "why?", or take a
    step over ("let me try"). In that case the agent waits, reads the
    producer's edit from the edit log, and comments on it.
  - Teach mode and normal mode share one action layer, so nothing behaves
    differently, only slower and narrated.
- **Exit:**
  - The producer can say "make the build build more and show me what you
    changed", see the edits ringed, hear them, and undo any of them.
  - In teach mode, the agent builds a 16-bar acid loop step by step with a
    bubble on every step, and the producer can take over mid-way.

### Phase 6: Learning test with acid techno

Goal: the producer builds an acid techno track in the new UI. They start by
vibing with the agent in teach mode and watching it work, and do more of it
by hand as they learn. This is spec milestone G1.

- **Learning content.** Explanations for every control and step (spec G8),
  drawn from `docs/research/`.
- **Feedback loop.** UX issues get logged, fixed, and retested.
- **Exit:** a finished ~4-minute track the producer is happy with, rendered
  at reference-class loudness.

### Phase 7: Expand

After the learning test, ordered by what the track and the producer ask for:

- **Mastering.** A clipper before the limiter and kick transient shaping, to
  close Plan 5's −8 LUFS gap. Also a release-master ceiling for lossy
  encodes.
- **More synths and effects** as modules: wavetable or FM, supersaw, more
  drum voices.
- **MIDI controller mapping.** Parameters already have stable ids and
  normalised ranges.
- **Hardtekk, tekkno and uptempo tooling** (spec §3).
- **Third-party plugin hosting (CLAP/VST3/AU),** if still wanted then.

## Open decisions for the producer

1. **Harness:** OpenCode or the Claude Agent SDK. The Phase 5 spike decides,
   unless you already know.
2. **Model access and billing** for the in-app agent: an API key, a
   subscription-backed harness, or a local model.
3. **Session view:** do you want an Ableton-style session or clip-launch view
   next to the arrangement, or arrangement only?
4. **Plugin hosting:** confirm it stays out until Phase 7.
5. **Spec revision:** approve the changes listed under "What this changes in
   the spec" so Phase 1 can start from an updated spec.
