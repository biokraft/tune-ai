# tune-ai studio: design handover

Phase 1 of `docs/roadmap.md` turned the brief
(`2026-10-21-studio-ux-brief.md`) into a design exploration on a Claude
Design canvas. This document hands that exploration to whoever builds it in
Claude Code. It says where the designs are, what each board covers, what is
still undecided, what was decided along the way, and what the engine and
studio core must grow to support it.

Phase 1 exits when the producer approves the design. That has not happened
yet: the variation picks in §4 are open.

## 1. Where the design lives

- **The canvas**: `https://claude.ai/artifact/GDNbBd8U3s1QDbwqJPG2z3`
  (private to the producer until shared from its Share menu). Seven pages,
  28 artboards. Claude Code can read it with the Artifact tool (`read`, with
  a `path` such as `project/Main.dc.html`).
- **The sources**, copied into the repo at `docs/design/studio-canvas/`: one
  `.dc.html` file per artboard plus `canvas.json` (the board index: page,
  position, size and title of each board). They are a snapshot of the canvas
  on 2026-10-21; if the canvas is edited later, the canvas wins.
- **Reading a source file.** Each `.dc.html` is a Design Component page:
  HTML inside `<x-dc>`, a `<helmet>` with the fonts and a `<style>` block,
  and a `class Component extends DCLogic` script whose `renderVals()`
  computes the data the markup loops over (`<sc-for>`, `<sc-if>`,
  `{{holes}}`). The files need the canvas runtime (`support.js`) to render,
  so open them on the canvas, not in a browser. Read them for exact values:
  colours, sizes, spacing, copy and layout.
- **The shared style sheet.** Every board starts with the same `<style>`
  block, first written in `Main.dc.html`. It is the de facto token sheet
  (CSS variables on `.app`, the three looks as `.look-a/.look-b/.look-c`)
  and holds the classes all boards share: `.glass`, `.transport`, `.tl`,
  `.row`, `.blk`, `.chat`, `.card`, `.ib` and so on. The knob markup and
  CSS (`.knob`, `.dial`, `.arc`, `.cap`) are identical on every board that
  has knobs.
- **The `look` tweak.** Every board takes a `look` prop (`a`, `b` or `c`),
  editable in the canvas's Tweaks panel, so any board can be seen in any of
  the three styles. `Main.dc.html` also takes `chat` (`open` or `rail`).

## 2. Not verified

- **No board has been seen rendered.** They were written and published, and
  their tags balance, but nobody has looked at them in the canvas yet. Some
  may render wrong. Look at each board on the canvas before building from
  it, and treat a broken board as a bug in the design file, not as the
  intended design.
- **The prototype has not been play-tested.** `Proto.dc.html` wires all
  seven flows in §3, but none has been clicked through.

## 3. Board map

Brief §8 item numbers in brackets.

**Page 1, Base view & timeline**

| Board | Shows |
| --- | --- |
| `Main` | Base view A [2]: transport, zoomed-to-fit timeline of first-acid playing the drop with loop 81–96, cutoff lane, slim agent panel with an action card. `chat=rail` shows the collapsed rail. |
| `BaseB` | Base view B [2]: transport merged with a song-map ribbon (energy per section, loop, playhead), timeline zoomed to bars 65–112 following the playhead, agent as a 300 px drawer. |
| `Timeline` | Timeline details [3]: playing state, loop drag, automation lanes with breakpoint drag and pen, linked blocks (`×5`, Make unique, context menu), Phase 2 free clips. |
| `EmptySong` | Empty song [3]: one 8-bar section, "Drag a sound here or ask the agent", starter chips, learning mode on. |

**Page 2, Style, tokens & controls**

| Board | Shows |
| --- | --- |
| `StyleA` / `StyleB` / `StyleC` | Overall style [1]: `Main` and `Dock303` imported in look A (Dusk Glass), B (Night Console) and C (Aurora). |
| `Tokens` | Design tokens [15]: colours, glass per look, type, spacing, radii, blur, motion, glass budget. |
| `Controls` | Knob and control set [5]: three knob styles × idle, hover, dragging, automated, agent-edited, disabled; faders, meters, step buttons, toggle, segmented control, tooltip. |

**Page 3, Modules & note editors**

| Board | Shows |
| --- | --- |
| `Dock303` | 303 docked [4]: the acid chain 303 → EQ → distortion → strip, "+ effect" slot, help line. |
| `Float303` | Floating modules [4]: the 303 and the clap in floating glass windows. |
| `DockDrum` | Kick docked [4]: sound and steps side by side, pattern link bar. |
| `Seq303` | 303 step sequencer [6]: A columns, B pitch lane. |
| `DrumGrid` | Drum step grid [6]: A all voices, B one voice with accent lane. |

**Page 4, Mixer, browser & listening**

| Board | Shows |
| --- | --- |
| `MixerDock` | Mixer A [7]: a Mixer tab in the bottom dock, strips, delay and reverb buses, master. |
| `MixerView` | Mixer B [7]: a full Mix view the timeline swaps to (Arrange \| Mix switch). |
| `Browser` | Browser [8]: Sounds / Effects / Songs, Ride previewing, a drag onto the timeline with a new-track drop row. |
| `Listen` | Listening A [12]: a full view: loudness and true peak against targets, section energy, bands, spectrogram, compare with a reference, reference picker. |
| `ListenB` | Listening B [12]: a 460 px right drawer that stands in for the chat. |

**Page 5, Agent panel & edits**

| Board | Shows |
| --- | --- |
| `AgentPanelsA` | Agent panel A [9]: Chat, Approvals, Sessions, Memory & skills, Activity, top tabs. |
| `AgentPanelsB` | Agent panel B [9]: side tabs in context, plus the collapsed rail with a toast. |
| `Permissions` | Permissions [13]: presets and a per-change-kind table. |
| `AgentEdit` | Normal-mode edit [10]: six frames from before to after, an off-screen change, the undo-conflict warning. |
| `Approval` | Pending big change [10]: ghost preview on the timeline, Approve / Reject card. |

**Page 6, Learning mode**

| Board | Shows |
| --- | --- |
| `Learn` | Learning A [11]: spotlight, speech bubble on the cutoff knob, full lesson bar. |
| `LearnB` | Learning B [11]: lesson rail in the agent panel, softer spotlight, "Let me try". |
| `LearnRecap` | Recap card, and replaying an old action card as a lesson [11]. |

**Page 7, Prototype**

| Board | Shows |
| --- | --- |
| `Proto` | Clickable prototype [14]: play the loop; click a block to open the 303 in the dock, pop it out and drag it; ask "open the filter in the build" in normal mode and in learning mode; drag Ride from the browser to make a track; approve a pending rearrangement; open listening from the loudness readout. A sticky note beside it lists the steps. |

## 4. Open decisions

The producer picks one option per row (or a combination) before the design
counts as approved. "Lean" is the designer's suggestion, not a decision.

| Decision | Options | Lean |
| --- | --- | --- |
| Overall style | A Dusk Glass (`StyleA`), B Night Console (`StyleB`), C Aurora (`StyleC`) | A, with B's density as the reduced-effects setting |
| Base view | A panel layout (`Main`), B song-map ribbon (`BaseB`) | A, with B's ribbon as an option |
| Agent panel | A top tabs (`AgentPanelsA`), B side tabs (`AgentPanelsB`) | A tabs, plus B's rail toast |
| Module placement | Dock under the timeline only (`Dock303`) or under timeline and chat | Under the timeline only |
| Note editor | Tab in the dock (`Dock303`), side by side with the sound (`DockDrum`), in a floating window (`Float303`) | Side by side for drums, tab for the 303 |
| 303 sequencer | A columns, B pitch lane (`Seq303`) | Undecided |
| Drum grid | A all voices, B one voice (`DrumGrid`) | A, with B as a zoom |
| Mixer | A dock tab (`MixerDock`), B full view (`MixerView`) | Undecided |
| Listening view | A full view (`Listen`), B side drawer (`ListenB`) | B, with "open full" to A |
| Learning mode | A spotlight and bubble (`Learn`), B lesson rail (`LearnB`) | A on the control, B's rail for the step list |
| Permissions defaults | The Balanced preset as drawn in `Permissions` | Balanced |
| Free note clips | Phase 2 variation in `Timeline` (e) | Phase 2, as the brief says |

## 5. Decisions made in the design

These go beyond the brief or settle a "designer's guess" in it.

- **Agent colour.** Cyan `#3FDCEB`, used only for "the agent did this".
  To keep it apart, the hats moved from cyan-blue to blue: closed hat
  `#5E8CFF`, open hat `#A3C0FF` (differing in lightness as well as hue).
- **Track palette.** Kick `#FF5470`, clap `#FFB547`, closed hat `#5E8CFF`,
  open hat `#A3C0FF`, ride `#B79CFF`, acid `#A8F04C`.
- **Loop range is white**, not amber, because amber belongs to the clap.
  The playhead is near-white `#F7F2FF` with a soft glow.
- **Type.** Manrope for the UI, JetBrains Mono for live numbers (BPM,
  position, loudness, knob values). Labels are 10–10.5 px uppercase with
  letter spacing.
- **The dock sits under the timeline only**, so the agent panel keeps its
  full height while a module is open.
- **Linked blocks.** A pattern used by several blocks shows a `×N` badge and
  the same tint; "Make unique" sits on the block's context menu and in the
  editor header.
- **Automation lane values show as % of the parameter's range**, because the
  mapping from a lane's normalised value to Hz is not defined in the UI
  yet. Showing the plain value (e.g. Hz for cutoff) is better once it is.
- **Help text** shows in a help line at the bottom of the timeline or dock
  for the hovered or focused control, and in a tooltip on hover.
- **Glass budget.** At most three large blurred surfaces on screen
  (transport, timeline, agent panel; a floating window counts as one).
  Meters, knobs, blocks and the playhead are never blurred. The
  reduced-effects fallback turns blur off and uses a solid `#160F24`.
- **Motion.** A normal-mode edit takes about 900 ms in total, ease-out; a
  knob sweep 600 ms; the agent glow fades over 2.4 s to a soft mark; a
  learning step paces at about 1.6 s per change at 1×; panels open in
  220 ms; reduced motion makes all of it instant.

## 6. Placeholders and invented values

- **Analysis numbers** (loudness per section, bands, LRA, verdict lines) are
  examples, not measurements of first-acid. The real ones come from
  `tune analyze` and `tune compare`.
- **Reference tracks** show as "[reference A]", "[reference B]", tagged
  `refs/` or `tune.local.toml`. Never put a real reference name or path in
  the app's design or tests.
- **Parameter values the song file does not set** use plausible defaults:
  kick decay 400 ms, punch 60 %, click 35 %; clap tone 1.2 kHz, decay
  180 ms, spread 12 ms; 303 slide time 60 ms, volume −6 dB. Take the real
  defaults from `tune params --json`.
- **Agent copy**, session names, memory items and activity entries are
  examples of tone and length, not product copy.
- **Prototype shortcuts**: the loop is fixed at 81–96 (a lesson moves it to
  49–64); chat text without "filter" or "cutoff" gets a canned reply;
  mute/solo, Fit song / Follow, the browser's Effects and Songs tabs, search
  and the reference picker do nothing; opening the browser closes a docked
  module.

## 7. From design to code

### 7.1 Existing components

`packages/ui` already exports `Knob`, `AcidStepSequencer`, `DrumGrid`,
`Arrangement`, `TrackStrip`, `TransportBar` and `FxPanel`, styled with the
gunmetal "instrument faceplate" theme in `packages/ui/src/theme/tokens.css`.
That theme is a placeholder (`docs/roadmap.md`). The new look replaces the
tokens; the components keep their behaviour and tests where they can.

| Component | Becomes |
| --- | --- |
| `tokens.css` | The variables from §5 and `Main.dc.html`'s `.app` block, with the look variables. Keep the existing `--claude` role, renamed to the agent colour. |
| `Knob` | The ring knob in `Controls`, with all six states. The existing `Highlight` prop maps onto the agent-edited state. |
| `TransportBar` | `Main`'s transport: loop toggle, stop, play, bar.beat, time, BPM, loop range, browser and mixer buttons, loudness readout, reset view. |
| `Arrangement` | The timeline: tracks as rows, section-locked blocks with pattern glyphs and link badges, section row, ruler, loop overlay, playhead, automation lanes, drop zone. |
| `AcidStepSequencer`, `DrumGrid` | The picked variations in `Seq303` and `DrumGrid`. |
| `TrackStrip`, `FxPanel` | The device chain in `Dock303` / `DockDrum` and the mixer strips in the picked mixer board. |

New components: app shell (fixed viewport, resizable and collapsible
regions, remembered layout, reset), module dock and floating window, device
chain, fader, meter, loudness meter, segmented control, agent panel (chat,
action card, approvals, sessions, memory, activity, rail, toast), spotlight
and speech bubble, lesson bar, listening view (spectrogram, energy bars,
band bars, compare), browser, permissions sheet.

### 7.2 What the studio core and protocol need

Today's protocol (`apps/desktop/src/client/protocol.ts`) has transport, loop,
`set_param` (mix params included), `set_fx_mode`, step edits, section move,
resize, duplicate and delete, `set_section_play`, `set_track_mix`, undo, redo
and `resolve_offered`. Events are `state`, `playhead`, `applied`,
`external_change` and `error`. The design needs more:

- **Action grouping.** An action card is one agent action with several
  changes and an explanation. `applied` carries a source and a list of
  changes but no action id, title or reason.
- **Per-change undo.** `UndoEntry` stores the whole file before and after
  one write burst, so undo works per burst, not per change. Undoing one line
  of a card needs an inverse edit per `Change`, plus a check for later
  changes to the same path ("this also reverts your later change") and for
  later steps that depend on it.
- **Approvals.** A pending action that is not applied: a dry-run that
  returns its changes for the preview, then apply or discard. Plus the
  permissions setting that decides which change kinds wait.
- **Learning mode and replay.** Applying an action one change at a time
  with pacing; Back undoes one step; replay first restores the song to the
  state before the action, then plays the steps again. That needs the
  action log to keep each action's before-state.
- **Missing edit commands**: automation lane breakpoints (lanes are
  read-only in `tune set` today), Make unique (copy a pattern and point one
  block at it), add and rename a section, add and remove a track and its
  devices (roadmap Phase 2).
- **Live meters.** The loudness readout, track meters and the limiter's
  gain reduction need a level event; only `playhead` streams today.
- **Listening in the app.** `tune analyze` and `tune compare` exist in the
  CLI only. The listening view needs them as studio commands, with the
  spectrogram served to the app.
- **Browser and samples**: roadmap Phase 3.
- **Help text** already exists (`help` on `ParamSpecView`); the help line
  and tooltip can use it as is.

These belong in the Phase 2 spec (modular model, app-wide undo, action
parity) and the Phase 5 spec (the in-app agent), not in the UI build alone.

## 8. Next steps

1. The producer looks at every board on the canvas and notes what renders
   wrong.
2. The producer makes the picks in §4. Record them in this file and mark
   Phase 1 approved in `docs/roadmap.md`.
3. Revise `2026-10-20-tune-ai-design.md` as `docs/roadmap.md` asks (chat is
   a goal, the new screens replace §6.8), using this handover and the picked
   boards.
4. Plan Phase 2 with the gaps in §7.2 in scope.
