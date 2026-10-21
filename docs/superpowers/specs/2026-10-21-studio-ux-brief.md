# tune-ai studio — UX/UI design brief (for Claude Design)

Phase 1 of `docs/roadmap.md`. This brief is the input for a Claude Design
exploration of the whole studio app. Claude Design must **explore**: for
every area below it shows several distinct variations side by side, so the
producer can pick and combine. The decisions marked as decided are the
starting direction, not the only option to draw.

## 1. What tune-ai is

tune-ai is a desktop music workstation (macOS, Tauri + React) for making
electronic music, acid techno first. Two kinds of users work in it at the
same time:

- **The producer**, a beginner who wants to make tracks and learn while
  doing it. Keyboard and mouse, space bar to play and pause.
- **An AI agent** in a docked chat, which can do everything the producer can
  do in the UI, and more: render, listen (loudness and spectrum analysis),
  compare against a reference track, arrange, explain.

The producer's main way of working: tell tune-ai what to do, watch it happen
on screen, and learn from it. Then do more by hand over time.

What makes it special: a faithful TB-303 ("the acid line") at its heart, an
agent that works *in front of you* rather than behind your back, and a
learning mode that turns any agent action into a step-by-step lesson.

## 2. Goals

1. A DAW-class workstation in the spirit of Ableton Live and FL Studio: build
   beats and songs on a timeline, shape sounds in instrument and effect
   modules, mix, automate.
2. It feels like an app, never like a website. **The window never scrolls as
   a page.** Panels scroll inside themselves only when they must.
3. A calm, understandable base view that a beginner can read at a glance.
4. The agent is always one glance away (right-hand panel) and its work is
   always visible and undoable.
5. A modern glassmorphic, futuristic look (see §5).

## 3. Anti-goals

The design fails, even if it technically works, if:

- **The screen is cluttered with widgets the producer has to close.** The
  base view must be straightforward; detail appears only when asked for.
  (The producer's own words: "if the screen is just way too cluttered with
  multiple widgets and I will have to close all of them".)
- It feels like a web page: page scroll, document-style layout, form fields.
- Agent edits feel like a black box: something changed and you can't see
  what, where or why.
- The producer learns nothing because the agent does everything invisibly.
- Glass effects cost so much rendering time that the playhead or meters
  stutter.

## 4. Layout and behaviour (decided starting direction)

### 4.1 Base view

On open, only these are visible:

- **Transport bar** (top): play/stop, BPM, position (bar:beat), loop range,
  and a live loudness readout.
- **Timeline** (centre, most of the screen).
- **Chat** (right), slim, collapsible to an icon rail.

Everything else opens on demand and closes with Esc or one click. The app
remembers the producer's layout. A "reset to base view" action is always
available.

### 4.2 Timeline (the main view)

- Tracks are rows (kick, clap, hats, ride, acid…). Time runs left to right in
  bars. A playhead sweeps across.
- The song is made of **sections** (intro, build, breakdown, drop, outro…)
  and each track plays a **pattern** or clip block within a section.
- Blocks light up while they play, so the producer always sees where the
  song is. The playing section is marked.
- **Automation lanes** (e.g. the 303's cutoff opening over 32 bars) sit
  under their track and can be drawn.
- Looping a range of bars is a primary action; the producer loops the song
  and fixes what sounds off.
- No Ableton-style clip-launch (session) grid for now. Timeline only.
- **What a block is** *(brief author's call)*. Today a block is "track X plays
  pattern P for all of section S": it is locked to its section, and several
  blocks can use the same pattern, so editing the pattern changes every block
  that uses it. Show that link (e.g. a "×3" badge, or matching tint) and
  offer "make unique". Phase 2 adds free note clips that can be moved,
  resized and split. Design for the section-locked block first and show the
  free clip as a variation.
- **Density** *(brief author's call)*. At 1440×900 about 12 track rows fit
  without vertical scrolling inside the timeline. A whole 4-minute song
  (~144 bars) fits with "zoom to fit"; otherwise the timeline scrolls
  horizontally *inside its own region*, following the playhead. More tracks
  scroll vertically inside the timeline region, never the page.

### 4.3 Module dock (bottom) and floating modules

- Clicking a block opens **the module that makes that sound** in a dock at
  the bottom: the 303 for the acid track, the drum voice for a drum track,
  later the sampler for a sample track (where you shape or cut the
  waveform). The timeline stays visible above it and keeps playing.
- Clicking another block swaps the dock's content. *(Designer's guess.)*
- One action pops the module out into a **floating glass window**: draggable,
  several can be open at once, each closable.
- Double-clicking a block opens its notes or steps (303 step sequencer, drum
  step grid, later a piano roll) rather than its sound. *(Designer's guess:
  the designer may place the note editor in the dock, as a tab, or in its own
  floating window; explore.)*
- A track's module shows its whole chain: instrument, then effects (EQ,
  distortion…), then its mixer strip (gain, pan, mute/solo, delay and reverb
  sends, duck).

### 4.4 Browser (left, closed by default)

Opens from a button, a shortcut, or when the producer starts a drag. It
contains **Sounds** (samples, synths, drum voices), **Effects** and **Songs**,
with search, tags or favourites, and preview (audition a sample before
using it). Dragging an item onto the timeline creates a track or replaces a
sound. An empty song's timeline says "drag a sound here or ask the agent".

### 4.5 Mixer and master

A mixer view with one strip per track (gain, pan, mute/solo, sends to the
shared delay and reverb, ducking under the kick, a level meter), plus the
shared effects (delay, reverb) and the master section (master gain, mode
"soft clip" or "limiter", ceiling, release, loudness and true-peak meters).
Where it lives (a dock tab, a view the timeline swaps to, a floating window)
is for the designer to explore.

### 4.6 Agent panel (right)

Tabs or sections, all starting points for the designer to vary:

- **Chat** (default): messages, streaming replies, **action cards** (what the
  agent did, one line per change, click to jump to it, undo per line, and
  "teach me this", which replays that action as a learning-mode lesson),
  and the switch for learning mode.
- **Approvals** (the producer's decision): small tweaks happen instantly
  with undo. Big changes wait for approval: deleting a track or section,
  rearranging the song, replacing a pattern wholesale. They show as a
  pending action card with Approve / Reject and a preview of what will
  change. A small permissions setting decides what counts as "big".
- **Sessions**: past chats; resume, rename, delete.
- **Memory & skills**: what the agent remembers about the producer and the
  song, and what it knows how to do; viewable and editable.
- **Activity**: a running log of every agent change, each undoable.

The panel minimises to a thin rail; a keyboard shortcut toggles it.

### 4.7 How agent work shows up

Two modes, one switch in the chat:

- **Normal mode (default).** The agent acts immediately, but visibly:
  - new track rows fade in;
  - knobs sweep to their new values;
  - steps light up.

  All of it happens together in about a second. Every agent-changed
  control glows briefly in the **agent colour** (cyan, distinct from every
  track colour), and the chat posts an action card.
- **Learning mode.** The same actions play out one at a time:
  - **Spotlight:** a spotlight dims everything but the control about to
    change.
  - **Speech bubble:** the agent's bubble, anchored next to that control,
    explains what it does and why, in plain words for a beginner.
  - **Pacing:** the change happens slowly enough to watch.
  - **Back / Next:** these step through the actions like a tutorial video.
    Going back really undoes that step.
  - **Let me try:** hands the step to the producer. The agent waits, then
    comments on what the producer did.
  - **Recap:** a short recap closes the lesson.

The producer can always point the agent at something on screen (e.g.
right-click a block or knob → "ask about this").

More rules for how agent work shows up:

- **Changes to controls that aren't on screen** *(brief author's call)*:
  - In normal mode the agent does **not** open panels. The affected track
    row and block glow in the agent colour, and the action card names the
    control; clicking the card opens the module with that control
    highlighted. This avoids clutter.
  - In learning mode the agent **does** open the module in the dock for
    that step (spotlight on the control) and returns the layout to how it
    was at the end of the lesson. Showing is the point of the mode.
- **Learning-mode controls** *(brief author's call)*: Back, Next, Pause, a
  speed control (slower/faster), "Why?" (the agent explains more), "Let me
  try", and Exit. Audio keeps looping during a lesson so each change can be
  heard; a mute-while-explaining toggle is optional.
- **Replay** (the producer's decision): any past action card or Activity
  entry can be replayed as a lesson ("teach me this"), not only actions taken
  with learning mode switched on.
- **Undo** *(brief author's call)*: the app-wide undo stack is the default (Cmd-Z).
  Per-line undo on a card or Activity entry reverts just that change. If a
  later change touched the same control, the undo control warns "this also
  reverts your later change" and shows what; if a later agent step depends on
  it, the card offers to undo the whole action instead.

### 4.8 Listening view (the producer's decision: a light version)

A view or panel where the producer sees what the agent "hears":

- integrated loudness and true peak against the acid targets (−8 to −6 LUFS,
  true peak ≤ −1 dBTP);
- the energy curve per section, which shows that the drop is louder than the
  breakdown;
- band balance;
- the spectrogram of the song or a passage;
- **our track vs. a reference track**, with stacked spectrograms and plain
  verdict lines ("the drop is hollow in the low mids").

It also needs a small place to pick and manage reference tracks. These are
local files the producer owns; they are never uploaded or shared. The
transport's loudness readout opens this view. Keep it light: the readouts
and pictures the agent already produces, not a full metering suite.

## 5. Visual direction (decided starting direction: "Dusk, colour-coded")

From the producer's references (glassmorphic crypto app, glass Messenger,
"Lumina" dashboard) and the chosen mockup:

- Deep, near-black violet background with slow, soft colour blobs (magenta,
  indigo) glowing behind the glass.
- Panels are frosted dark glass with thin light edges and generous rounded
  corners, but **denser than the references**: a full song must fit.
- **Colour means a track.** Each track has its own colour, carried through
  its timeline blocks, its module, its knob rings, its mixer strip, and the
  agent's mention of it. Starting palette: kick pink-red, clap amber, hats
  cyan-blue, ride lavender, acid acid-green.
- **Agent colour:** cyan shimmer, used only for "the agent did this".
  *(Designer's guess: if cyan clashes with the hats, pick another reserved
  hue.)*
- Glow is reserved for what is live: the playing block, the playhead, a knob
  being moved, the agent's highlight.
- Small crisp labels; live numbers (BPM, bar, loudness, knob values) in a
  monospace or digital-feeling face. *(Designer's guess.)*
- Dark theme only for now.

Explore at least three variations of the overall style within this
direction, e.g. how much blur and glow, how dense, how saturated the
background, flat vs. dimensional knobs.

## 6. What exists today (design for these real controls)

The engine and song format exist; the design must show real controls, not
placeholders. Parameter ids as the engine names them:

- **Acid (303)**:
  - `tuning` Hz, `cutoff` Hz, `resonance` %, `env_mod` %, `decay` ms,
    `accent` %, `slide_time` ms, `waveform` (saw ↔ square), `volume` dB;
  - its distortion: `drive` dB, `mix`, `output` dB, mode soft / hard /
    overdrive.

  The 303 pattern is 16 steps (length configurable), each step with a note
  (e.g. C2, D#2), an accent flag and a slide flag, or a rest.
- **Kick**: `tune` Hz, `decay` ms, `punch` %, `click` %, `drive` dB.
- **Clap**: `tone` Hz, `decay` ms, `spread` ms.
- **Closed hat, open hat, ride**: `tone` Hz, `decay` ms, `metal` %.
- Drum patterns are step rows: hit, accented hit, rest.
- **Track EQ**: `low_cut` Hz, `mid_freq` Hz, `mid_gain` dB, `mid_q`,
  `high_gain` dB, `high_cut` Hz.
- **Per track**: gain dB, pan, mute, solo, `send.delay` dB, `send.reverb` dB,
  `duck.depth` dB, `duck.release` ms.
- **Delay (shared)**: `time` in 16ths (dotted eighth = 3), `feedback` %,
  `low_cut` Hz, `high_cut` Hz, `width` %.
- **Reverb (shared)**: `size` %, `damp` %, `pre_delay` ms, `low_cut` Hz,
  `width` %.
- **Master**: `gain` dB, `ceiling` dB, `release` ms, mode clip / limit.
- **Arrangement**: sections with bar counts; automation lanes of breakpoints
  (e.g. cutoff over a section).
- **Analysis** the agent uses (could appear in the UI): integrated loudness
  (LUFS), true peak, energy per section, a spectrogram picture.

Every control already has a short "what this does musically" help text; the
design needs a place for it (hover or focus → a help line or tooltip).

Coming soon, so the design should leave room for it:

- a sampler and a sample browser;
- adding and removing instruments and effects per track;
- a piano roll for note clips;
- recording knob moves into automation.

## 7. Constraints

- **Desktop only**, macOS first, a Tauri window (WebKit WebView). Target
  window sizes:
  - 1280×800 minimum;
  - 1440×900 typical;
  - large external displays.

  Regions resize and collapse; nothing ever page-scrolls.
- **Realtime**: the playhead, meters and knob motion must stay smooth while
  audio plays. Budget the glass: few large blurred surfaces rather than blur
  on many small elements, and reduced effects as a fallback setting.
- Built with React + Tailwind and shadcn/Radix primitives for standard
  controls. Audio controls are custom: knob, fader, meter, step grid,
  timeline block, automation lane, waveform, spectrum, device chain.
- Keyboard first-class:
  - space = play/pause;
  - Esc closes the topmost panel;
  - shortcuts for the browser, the mixer, the agent panel and loop;
  - every control reachable by keyboard.
- Accessible contrast for text and values despite the glass; track colours
  must stay distinguishable.

## 8. What Claude Design should deliver

For each, several variations side by side where marked (≥2–3):

1. **Overall style**: three variations within §5 (≥3).
2. **Base view**: the transport bar, timeline and slim chat, with a song
   playing (≥2).
3. **Timeline details**:
   - the playing state;
   - section markers;
   - loop range;
   - automation lanes;
   - an empty song.
4. **Module dock** with the 303 open, docked and floating (≥2). The same for
   a drum voice.
5. **Knob and control set**:
   - knob idle, hover, dragging, automated, agent-edited, disabled;
   - fader, meter, step button, toggle (≥3 knob styles).
6. **303 step sequencer** and the **drum step grid** (≥2).
7. **Mixer and master** (≥2 placements).
8. **Browser**, with preview and drag onto the timeline.
9. **Agent panel**: chat with action cards, sessions, memory & skills,
   activity, and the collapsed rail (≥2).
10. **Normal-mode agent edit**: a before/after frame sequence, including a
    change to a control that is off screen, and a pending "big change"
    card with Approve / Reject.
11. **Learning mode**: spotlight, speech bubble, the full control bar (Back,
    Next, Pause, speed, Why?, Let me try, Exit), a recap, and starting a
    replay from an old action card (≥2).
12. **Listening view**: loudness against targets, section energy, our
    track vs. the reference, and the reference picker (≥2).
13. **Permissions setting** for what counts as a "big" agent change.
14. A **clickable prototype** of the main flows:
    - open the song and play the loop;
    - click a block to open the 303 in the dock, then pop it out;
    - ask the agent "open the filter in the build", watching it in normal
      mode and then in learning mode;
    - drag a sound from the browser to make a new track;
    - approve a pending big change;
    - open the listening view from the loudness readout.
15. **Design tokens**: colours (background, glass layers, track palette,
    agent colour), radii, blur levels, typography, spacing, motion timings.

## 9. Left to the designer

- **Mixer and note editor:** where the mixer and the note editor live, as
  long as the base view stays calm.
- **Panel shape:** the exact shape of the agent panel's tabs and how the
  rail looks.
- **Motion:** easing, durations, and how "visible but fast" normal-mode edits
  look.
- **Iconography and typefaces:** within the §5 direction.
- **New interactions:** any idea the designer believes fits better than what
  is written here. Show it as an extra variation, labelled as such.
