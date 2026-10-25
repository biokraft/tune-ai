# Plan 6: Studio Reskin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move today's studio onto the approved Claude Design (look A, Dusk Glass) as a fixed-viewport app: transport on top, a fit-to-width timeline, and a module dock under it. It is a reskin and a UI/UX refactor of features that already exist, with no new engine or protocol features.

**Architecture:** The theme tokens and fonts change first. The existing `@tune/ui` components then take the new look while keeping their behaviour, ARIA and test ids: knob, controls, transport, timeline. New dock content (device chain, steps, mixer) is built against today's `StudioState` and commands. Finally `StudioScreen` becomes the shell that wires everything together and drops the old page layout (Panel303, MixPanel, the drum section at the bottom).

**Tech Stack:** React 18.3.1, TypeScript, Tailwind 3.4.17, Radix Popover 1.3.1, Vitest 4.1.10 + Testing Library, jsdom 29.1.1. New dependencies: `@fontsource/manrope` 5.3.0 and `@fontsource/jetbrains-mono` 5.3.0 (pinned exactly). No Rust changes.

**Spec:**
- `docs/superpowers/specs/2026-10-21-studio-ux-brief.md` (the brief).
- `docs/superpowers/specs/2026-10-21-studio-design-handover.md` (§4 open decisions, §5 decisions made, §7.1 component mapping).
- The boards in `docs/design/studio-canvas/`: `Main`, `Tokens`, `Controls`, `Timeline`, `Dock303`, `DockDrum`, `Seq303`, `DrumGrid`, `MixerDock`, `AgentEdit`. The rendered canvas is https://claude.ai/artifact/GDNbBd8U3s1QDbwqJPG2z3.

### The producer's picks (binding for this plan)

| Decision | Pick |
| --- | --- |
| Overall style | A Dusk Glass |
| Base view | A panel layout (`Main`) |
| Module placement | Dock under the timeline only |
| 303 note editor | Sound \| Steps tabs in the dock |
| 303 sequencer | A columns (`Seq303`) |
| Drum note editor | Side by side with the sound (`DockDrum`) |
| Drum grid | A all voices (`DrumGrid`), with the docked voice emphasised |
| Mixer | A dock tab (`MixerDock`) |
| Right panel | None. Timeline and dock take the full width. |
| Claude's edits | The existing change notice becomes a cyan action-card toast, floating top-right |
| Drum voice knobs | Yes: the dock shows generic knobs from `track.params` |
| Listening view | B (side drawer) is the pick, but it is not built in this plan |

## Global Constraints

- No Rust, protocol or song-format changes. The UI uses only today's `Command`s and `StudioState`.
- Build no feature the studio does not already have. In particular, leave out: browser, chat or agent panel, loudness or level meters, the listening view, teach mode, approvals, "Make unique", add/rename section, add/remove track or device, layout persistence, and resizable regions. Leave out any control for these too, including disabled ones.
- Only look A is built. There is no theme switcher.
- Colour tokens are copied verbatim from `Main.dc.html` `.app`:
  - `--bg:#0A0612; --ink:#EEE9F8; --ink2:#B5ADCB; --ink3:#9790AE; --line:rgba(255,255,255,.085); --line2:rgba(255,255,255,.16); --glass:rgba(23,16,39,.56); --glass2:rgba(10,6,18,.45)`
  - `--blur:22px; --r:16px; --rs:9px; --row:46px; --blobop:.55`
  - `--kick:#FF5470; --clap:#FFB547; --hat:#5E8CFF; --ohat:#A3C0FF; --ride:#B79CFF; --acid:#A8F04C; --agent:#3FDCEB; --play:#F7F2FF`
  - The reduced-effects solid is `#160F24`.
- Fonts are bundled through `@fontsource`; nothing loads from the network at run time. Manrope is the UI font (base 13px / 1.35). JetBrains Mono with `tabular-nums` is used for live numbers.
- Labels are 10–10.5px, uppercase, letter-spacing .1em, weight 700, colour ink2.
- Cyan (`--agent`) means only "Claude did this". Nothing else uses it: no focus rings, selection or buttons.
- The loop range is white. The playhead is `--play` with a soft glow.
- Glass budget: at most 3 surfaces use `backdrop-filter` (transport, timeline, dock). The change toast uses a solid fill. Knobs, meters, blocks and the playhead are never blurred.
- Motion: the Claude-edit glow fades over 2.4s and the dock opens in 220ms. Under `prefers-reduced-motion: reduce`, all of it is instant.
- The window never scrolls as a page. Panels scroll inside themselves. The minimum window is 1280×720.
- Keep every existing ARIA role, accessible name and `data-testid` that a test or a shortcut relies on, unless a task here renames it explicitly.
- Copy is in sentence case, except the uppercase `.lbl` labels, which are styled with CSS (`uppercase`), not typed in capitals.
- Every commit passes `task verify`. Commit atomically. No screenshots, renders or `scratch/` files in git. Visual checks use `tune serve --device null`.

## Review Focus

1. **A 1280×720 window.** Nothing scrolls the page, with or without the dock open. The timeline shrinks and scrolls inside itself. (Task 11 visual check: `scrollHeight <= innerHeight`.)
2. **A very long or very short song.** Fit-to-width stays readable: px per bar is clamped, and a long song scrolls the timeline sideways instead of squeezing the bars to nothing. (Task 5 `fitPxPerBar` tests.)
3. **The docked track disappears.** If an external edit removes the docked track, the dock closes and nothing crashes. (Task 9 `dockReducer` prune test and StudioScreen test.)
4. **The selected section is stale** after a delete, from the UI or from disk. The dock falls back to the section under the playhead and never indexes past `sections`. (Task 9 `dockSection` test.)
5. **No pattern, or no tracks of a kind.** A track with no pattern in the shown section, or a song with no acid or no drum tracks, shows an empty state and no crash. (Task 7 tests.)
6. **Esc inside a popover** closes the popover only. A second Esc closes the dock. (Task 9 test.)

---

## File Structure

**`packages/ui/src`**

| File | Change |
| --- | --- |
| `theme/tokens.css` | Rewritten: Dusk tokens as RGB triplets, plus the fonts |
| `theme/glass.css` | New: `.glass`, `.glass-solid`, `.blob`, `.lbl`, `.kbd`, `.badge`, plus the reduced-effects and reduced-motion rules |
| `tailwind.preset.ts` | New colour, font and radius names |
| `lib/instruments.ts` | New: track colour and title per instrument |
| `lib/arrange.ts` | `fitPxPerBar`; `dropIndex` takes px per bar; `formatPosition` becomes `"88.3"`; new `formatTime` |
| `components/Knob.tsx` | Ring style A |
| `components/Fader.tsx`, `Segmented.tsx`, `MuteSolo.tsx`, `IconButton.tsx` | New |
| `components/TransportBar.tsx` | Per `Main` |
| `components/Arrangement.tsx` | The timeline frame, then the blocks |
| `components/TrackStrip.tsx` | Mixer strip card |
| `components/FxPanel.tsx` | Device or bus card |
| `components/AcidStepSequencer.tsx` | Sequencer A columns |
| `components/DrumGrid.tsx` | All-voice grid with an emphasised row |

**`apps/desktop/src`**

| File | Change |
| --- | --- |
| `screens/StudioScreen.tsx` | The shell |
| `screens/dock/dock.ts` | New: dock state reducer and section choice |
| `screens/dock/Dock.tsx` | New: dock frame, header, tabs |
| `screens/dock/PatternPicker.tsx` | New: moved out of the arrangement popover |
| `screens/dock/SoundTab.tsx` | New: device chain |
| `screens/dock/StepsTab.tsx` | New: acid and drum steps |
| `screens/dock/MixerTab.tsx` | New: replaces `MixPanel` and the `Mixer` aside |
| `screens/help.tsx` | New: `HelpProvider`, `useHelp`, `HelpLine`; replaces `HelpStrip.tsx` |
| `screens/ChangeNotice.tsx` | Toast card |
| `screens/Banners.tsx`, `screens/OpenSongScreen.tsx` | Restyled |
| `lib/useStudioKeys.ts` | Adds M and Esc |
| `lib/song.ts` | Adds `patternGlyph` |
| `styles/app.css` | Drops the faceplate, screw and Barlow rules |
| `screens/Panel303.tsx`, `screens/MixPanel.tsx`, `screens/HelpStrip.tsx` | Deleted in Task 9; their tests are ported |

---

### Task 1: Dusk tokens, fonts and instrument colours

**Files:**
- Modify: `packages/ui/src/theme/tokens.css`, `packages/ui/src/tailwind.preset.ts`, `packages/ui/package.json`, `apps/desktop/src/styles/app.css`, `THIRD_PARTY.md`
- Create: `packages/ui/src/theme/glass.css`, `packages/ui/src/lib/instruments.ts`, `packages/ui/src/lib/instruments.test.ts`
- Modify: `packages/ui/src/index.ts`

**Interfaces:**
- Produces Tailwind colours:
  - `bg`, `ink`, `ink2`, `ink3`, `agent`, `play`, `kick`, `clap`, `hat`, `ohat`, `ride`, `acid`. All are alpha-capable: `rgb(var(--x) / <alpha-value>)`, with the variables as RGB triplets.
  - `line`, `line2`, `glass`, `glass2` as raw `var(--x)`, with no alpha.
- Produces fonts `font-sans` (Manrope) and `font-mono` (JetBrains Mono), and radii `rounded-r` (16px) and `rounded-rs` (9px).
- Produces CSS classes `.glass` (backdrop blur var(--blur) + saturate(150%), `--glass` fill, 1px `--line` border, inset top highlight, shadow), `.glass-solid` (same but `#160F24` fill, no blur), `.blob`, `.lbl`, `.kbd`, `.badge` (mono 9.5px, 1px `line2` border, radius 5px, padding 0 4px, ink2).
- Produces `trackColor(instrument: string): string`, returning a CSS colour expression such as `"rgb(var(--acid))"`.
- Produces `trackColorVar(instrument: string): string`, returning the bare variable name such as `"--acid"`, for alpha mixes.
- Produces `instrumentTitle(instrument: string): string`.

- [ ] **Step 1: Write `instruments.test.ts`**

```ts
it("maps every instrument to its design colour", () => {
  expect(trackColorVar("acid303")).toBe("--acid");
  expect(trackColorVar("kick")).toBe("--kick");
  expect(trackColorVar("clap")).toBe("--clap");
  expect(trackColorVar("hat_closed")).toBe("--hat");
  expect(trackColorVar("hat_open")).toBe("--ohat");
  expect(trackColorVar("ride")).toBe("--ride");
  expect(trackColorVar("future_synth")).toBe("--ink2");
  expect(trackColor("kick")).toBe("rgb(var(--kick))");
});
it("titles instruments for the dock chip", () => {
  expect(instrumentTitle("acid303")).toBe("Acid 303");
  expect(instrumentTitle("hat_closed")).toBe("Closed hat");
  expect(instrumentTitle("hat_open")).toBe("Open hat");
  expect(instrumentTitle("future_synth")).toBe("Future synth");
});
```

- [ ] **Step 2: Run** `pnpm --filter @tune/ui test instruments`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement `lib/instruments.ts`** and export it from `index.ts`.
- [ ] **Step 4: Rewrite `tokens.css`.**
  - Replace the Barlow imports with `@fontsource/manrope` weights 400, 500, 600, 700, 800 (latin) and `@fontsource/jetbrains-mono` weights 400, 500, 600 (latin).
  - Define the Global Constraints tokens on `:root`. The colour tokens are RGB triplets (e.g. `--acid: 168 240 76`); the rgba glass and line tokens and the geometry tokens stay as given.
  - Keep the old token names until Task 11 as aliases of the new palette, so the untouched screens stay readable:

    | Old name | New value |
    | --- | --- |
    | `chassis`, `background` | `bg` |
    | `faceplate`, `faceplate-hi`, `card`, `popover`, `secondary`, `muted` | the reduced-effects solid `22 15 36` |
    | `silkscreen`, `foreground` (and the `*-foreground` names) | `ink` |
    | `border`, `input` | `255 255 255`; old uses already carry an alpha |
    | `led`, `ring` | `ink` |
    | `slide` | `ink2` |
    | `claude` | `agent` |
    | `primary`, `accent` | `play` |
    | `destructive` | `kick` |

  - Switch the preset's `token()` helper to `rgb(var(--x) / <alpha-value>)` for every name.
- [ ] **Step 5: Write `glass.css`.**
  - Under `@media (prefers-reduced-transparency: reduce)`, `.glass` falls back to `.glass-solid`'s look and `.blob` is hidden.
  - `.lbl` is 10px, 700, uppercase, `letter-spacing:.1em`, ink2.
  - `.kbd` is the key hint from `Main` (mono 10px, 1px `--line2` border, radius 4px, padding 0 4px).
  - Import it in `app.css` after `@tailwind components`.
  - Set `body` to `font: 13px/1.35 Manrope, sans-serif`, with background `rgb(var(--bg))` and colour `rgb(var(--ink))`.
- [ ] **Step 6: Swap the font dependencies.** In `packages/ui/package.json`, swap `@fontsource/barlow*` for the two new packages at exactly `5.3.0`, then run `pnpm install`. In `THIRD_PARTY.md`, replace the Barlow entry with Manrope and JetBrains Mono (SIL OFL 1.1, self-hosted through `@fontsource`).
- [ ] **Step 7: Run** `task verify`. Expected: PASS. No test asserts colours or fonts.
- [ ] **Step 8: Commit** `feat(ui): add the Dusk Glass tokens, fonts and track colours`.

---

### Task 2: Knob ring style A

**Files:**
- Modify: `packages/ui/src/components/Knob.tsx`, `packages/ui/src/components/Knob.test.tsx`

**Interfaces:**
- Consumes `trackColor` from Task 1.
- Produces: `KnobProps` gains `color?: string` (a CSS colour for the arc; default `rgb(var(--ink))`) and `disabled?: boolean`.
- Sizes become `PX = { lg: 56, md: 46, sm: 36 }`. The column width is `dial + 16`.
- Everything else (`spec`, `value`, `size`, `onChange`, `onCommit`, `onHelp`, `highlight`, `automated`) is unchanged.

The look is copied from `Dock303.dc.html` `.knob` and the `Controls` board:
- A 270° track arc (`rgba(255,255,255,.1)`, stroke 4, round caps) and a value arc in `color`.
- A radial-gradient cap (`#3B2F57`→`#140E1F`) with a 2×8px ink pointer.
- The value sits above the label in mono 11px/500. The label below it uses `.lbl`.

The six states:

| State | Look |
| --- | --- |
| Idle | As above |
| Hover | Track arc brightens to `.16` |
| Dragging | A value chip (mono, `glass-solid`, rounded-rs) floats above the dial and shows `formatValue` |
| Automated | `data-testid="knob-lane"` stays; drawn as a small dot in the track colour under the label |
| Claude-edited | `highlight === "claude"`: `data-testid="knob-highlight"` is a cyan ring with a cyan glow, plus an "AI" badge (mono 8.5px, cyan text on `agent/15`) at the top-right of the dial. `"external"` uses an ink2 ring and no badge. |
| Disabled | Opacity .4, `aria-disabled="true"`, ignores pointer and keys |

- [ ] **Step 1: Add the failing tests to `Knob.test.tsx`.** Keep every existing test.

```ts
it("draws the value arc in the track colour", () => {
  render(<Knob spec={cutoff} value={500} onChange={noop} onCommit={noop} color="rgb(var(--acid))" />);
  expect(screen.getByTestId("knob-arc")).toHaveAttribute("stroke", "rgb(var(--acid))");
});
it("marks Claude's edit with an AI badge, and other edits without one", () => {
  const { rerender } = render(<Knob spec={cutoff} value={500} onChange={noop} onCommit={noop} highlight="claude" />);
  expect(screen.getByTestId("knob-highlight")).toBeInTheDocument();
  expect(screen.getByText("AI")).toBeInTheDocument();
  rerender(<Knob spec={cutoff} value={500} onChange={noop} onCommit={noop} highlight="external" />);
  expect(screen.queryByText("AI")).not.toBeInTheDocument();
});
it("ignores keys and drags when disabled", async () => {
  const onCommit = vi.fn();
  render(<Knob spec={cutoff} value={500} onChange={noop} onCommit={onCommit} disabled />);
  const k = screen.getByRole("slider", { name: /cutoff/i });
  expect(k).toHaveAttribute("aria-disabled", "true");
  k.focus(); await userEvent.keyboard("{ArrowUp}");
  expect(onCommit).not.toHaveBeenCalled();
});
it("shows a value chip while dragging", () => { /* pointerDown on the slider → chip with formatValue text is visible; pointerUp → gone */ });
```

- [ ] **Step 2: Run** `pnpm --filter @tune/ui test Knob`. Expected: the new tests FAIL.
- [ ] **Step 3: Restyle `Knob.tsx`.**
  - Keep the drag maths, keys, double-click reset, ARIA and test ids. Keep `memo` and the render-count behaviour: `renderCost.test` requires 0 knob renders per playhead tick.
  - Put the dragging chip in local state that is set only on pointer down and up, so a drag does not re-render siblings.
  - The 2.4s glow fade is a CSS animation on `[data-highlight] [data-testid="knob-highlight"]`. It replaces `highlight-fade 4s` in `app.css`: keyframes hold full opacity to 40%, then go to .35. Turn it off under reduced motion.
- [ ] **Step 4: Run** the Knob tests and `pnpm --filter @tune/desktop test renderCost`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): restyle the knob as the Dusk ring with its six states`.

---

### Task 3: Controls kit

**Files:**
- Create: `packages/ui/src/components/{Fader,Segmented,MuteSolo,IconButton}.tsx`, each with a `.test.tsx`
- Modify: `packages/ui/src/index.ts`

**Interfaces:**
- `IconButton(props: { label: string; kbd?: string; size?: "md" | "sm"; pressed?: boolean; disabled?: boolean; onClick(): void; children: ReactNode; className?: string })`
  - Heights: 32px for md, 26px for sm. Radius `rs`.
  - Has `aria-label={label}`, plus `aria-pressed` when `pressed` is defined.
  - Shows `kbd` as a `.kbd` chip.
  - When pressed, the fill is `ink/12` and the border `line2`. Never cyan.
- `Segmented(props: { label: string; options: { value: string; label: string }[]; value: string; disabled?: boolean; onChange(v: string): void })`
  - A `role="radiogroup"` named `label`, holding `role="radio"` buttons with roving tabindex. Arrow keys move and select.
  - Move the behaviour from `Panel303`'s `ModeSwitch`.
- `MuteSolo(props: { track: string; mute: boolean; solo: boolean; onMute(v: boolean): void; onSolo(v: boolean): void })`
  - Two 22×20 `.ms` toggles, "M" and "S", with `aria-pressed`.
  - They take exactly the accessible names `TrackStrip` uses for mute and solo today. Read them from `TrackStrip.tsx` and keep them.
  - Mute on: fill `ink/80`, text `bg`. Solo on: fill `clap`, text `bg`.
- `Fader(props: { label: string; value: number; min: number; max: number; defaultValue: number; unit: string; color?: string; onChange(v: number): void; onCommit(v: number): void; onHelp?(text: string | null): void })`
  - A vertical `role="slider"` with `aria-orientation="vertical"`, `aria-valuetext` like `"-3.0 dB"`, and the track filled in `color`.
  - Drag: 160px covers the full range. Shift is 10× finer.
  - Keys: ArrowUp and ArrowDown step 0.5, PageUp and PageDown step 3. Each key commits.
  - Double-click commits `defaultValue`.
  - Use the same gain range and default that `TrackStrip`'s gain control uses today; read them from `TrackStrip.tsx`.

- [ ] **Step 1: Write the tests.** One file per component. The assertions:
  - `Segmented`: ArrowRight from `soft` calls `onChange("hard")`; `disabled` blocks clicks and keys.
  - `MuteSolo`: clicking M calls `onMute(true)` when `mute=false`; both buttons expose `aria-pressed`.
  - `Fader`: ArrowUp on value 0 commits 0.5; PageDown commits −3; double-click commits `defaultValue`; the value is clamped to `[min, max]`; `aria-valuetext` is `"0.0 dB"`.
  - `IconButton`: `pressed` sets `aria-pressed="true"`; `kbd` renders the key text.
- [ ] **Step 2: Run** `pnpm --filter @tune/ui test Fader Segmented MuteSolo IconButton`. Expected: FAIL.
- [ ] **Step 3: Implement** the four components and export them. Keep them `memo`'d where they take only primitive props.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): add the fader, segmented, mute/solo and icon button controls`.

---

### Task 4: Transport per `Main`

**Files:**
- Modify: `packages/ui/src/lib/arrange.ts`, `packages/ui/src/lib/arrange.test.ts`, `packages/ui/src/components/TransportBar.tsx`, `packages/ui/src/components/TransportBar.test.tsx`, `apps/desktop/src/renderCost.test.tsx`

**Interfaces:**
- Consumes `IconButton` from Task 3.
- Produces `formatPosition(bar, beat)`, which now returns `"88.3"` (one-based, no padding).
- Produces `formatTime(bar: number, beat: number, bpm: number): string`, giving `m:ss.d` for `(bar*4 + beat) * 60 / bpm` seconds, e.g. `"2:24.8"`.
- Produces `TransportBarProps`:
  - Removes `notice`.
  - Adds `totalBars: number`, `loopBars: [number, number] | null` (half-open, 0-based), `loopName: string | null`, `mixerOpen: boolean` and `onToggleMixer(): void`.
  - Keeps the rest.

Layout, left to right, per `Main`'s header:
- An acid-green squiggle icon, the song name (800 13px), and under it `"145 BPM · 144 bars"` in ink2 10.5px. Leave out "saved": there is no save status.
- A separator, then Loop (IconButton, `pressed=loopOn`, name `"Loop (L)"`), Stop, and Play/Pause. The Play/Pause button is a 40px round `--play` fill with a `bg` icon, named `"Play (space)"` or `"Pause (space)"`.
- Three readouts, each a `.lbl` over a mono value: `Bar · beat` (big mono 20px), `Time` (14px ink2) and `BPM` (big).
- The loop range: `.lbl` "Loop", mono `"81 – 96"` (1-based, inclusive), and the section name in ink2. Hide it when `loopBars` is null.
- A spacer, then Undo and Redo IconButtons, then Mixer (`kbd="M"`, `pressed=mixerOpen`).
- Leave out Browser, the LUFS readout and Reset view.
- The bar is a `.glass` surface: radius `r`, height 56px, padding 0 12px.

- [ ] **Step 1: Update the tests.**
  - `arrange.test.ts`: `formatPosition(87, 2) === "88.3"`; `formatTime(87, 2, 145)` equals the value from the formula. Work it out in the test as `(87*4+2)*60/145 = 144.83 s`, giving `"2:24.8"`. Also `formatTime(0, 0, 145) === "0:00.0"`.
  - `TransportBar.test.tsx`:
    - Position shows `"3.3"` for bar 2, beat 2.
    - The loop range reads `"81 – 96"` with name `"drop"` for `loopBars=[80,96]`.
    - The Mixer button toggles through `onToggleMixer`.
    - There is no `notice` slot.
  - `renderCost.test.tsx`: change the expected Position text from `"003:3"` to `"3.3"`.
- [ ] **Step 2: Run** `pnpm -r test arrange TransportBar renderCost`. Expected: FAIL.
- [ ] **Step 3: Implement** the changes. Update the `StudioScreen` call site to pass the new props:
  - `loopBars={state.loop_bars}`.
  - `loopName`: the name of the section whose bars equal the loop, else null.
  - `mixerOpen` and `onToggleMixer` stay stubbed (`false` and a no-op) until Task 9.
  - Render `ChangeNotice` temporarily just under the header, until Task 9 moves it.
- [ ] **Step 4: Run** `task verify`. Expected: PASS. Update any App or StudioScreen test that looked for the old `"003:3"` format.
- [ ] **Step 5: Commit** `feat(ui): rebuild the transport bar on the Main board`.

---

### Task 5: Timeline frame

**Files:**
- Modify: `packages/ui/src/lib/arrange.ts`, `packages/ui/src/lib/arrange.test.ts`, `packages/ui/src/components/Arrangement.tsx`, `packages/ui/src/components/Arrangement.test.tsx`, `apps/desktop/src/screens/StudioScreen.tsx`

**Interfaces:**
- Consumes `MuteSolo` (Task 3) and `trackColor` (Task 1).
- Produces:
  - `MIN_PX_PER_BAR = 6`, `MAX_PX_PER_BAR = 40` and `HEADER_W = 168`.
  - `fitPxPerBar(width: number, totalBars: number): number`, which returns `clamp((width - HEADER_W) / max(totalBars, 1), MIN, MAX)`. A width ≤ 0 returns `PX_PER_BAR` (12), the jsdom fallback.
  - `dropIndex(sections, x, pxPerBar = PX_PER_BAR)`.
- `ArrangementProps` changes:
  - `tracks` becomes `{ id: string; instrument: string; mute: boolean; solo: boolean }[]`.
  - New props: `openTrack: string | null`, `onOpenTrack(track: string): void` and `onMix(track: string, patch: { mute?: boolean; solo?: boolean }): void`.
  - The other props are unchanged.

The layout per `Main` and `Timeline`:
- **Title row.** "Arrangement" (800 13px) and `"8 sections · 5 tracks"` in ink2.
- **Ruler row.** Mono 10px ink3 bar numbers, 1-based. Label every 4 bars when px per bar ≥ 16, otherwise every 8 or 16, so that labels are at least 40px apart.
- **Sections row.** Each section is a chip with its name. The selected section is outlined in `line2` and filled `ink/8`. The existing drag-to-move, resize handles (`resize-i`), duplicate and delete controls and `section-i` test ids move here unchanged.
- **One row per track, `--row` 46px tall.**
  - A 168px sticky-left header (`.hdr`): a 3px colour bar in `trackColor`, the track id (700 12.5px), the instrument title in ink3, and `MuteSolo` on the right.
  - The whole header is a button named `"Open <id>"`. It calls `onOpenTrack`, and has `aria-pressed` when the track is `openTrack`.
  - The cells keep today's per-cell pattern popover until Task 10.
- **Loop range.** A white band at `ink/6` over the rows, with 1px white edges and a white bracket in the ruler (`data-testid="loop-range"`).
- **Playhead.** A 2px `--play` line with `drop-shadow(0 0 6px rgb(var(--play)/.8))` spanning the ruler and the rows, keeping its test id and `x1`.
- **Sizing.** The component measures its scroller with a `ResizeObserver` (guarded with `typeof ResizeObserver !== "undefined"`) and uses `fitPxPerBar`. Its content scrolls on both axes inside itself: the header column and the ruler and sections rows are sticky.

- [ ] **Step 1: Write the tests.**
  - `arrange.test.ts`:
    - `fitPxPerBar(1168, 100) === 10`.
    - `fitPxPerBar(1168, 4) === 40` (the cap).
    - `fitPxPerBar(1168, 1000) === 6` (the floor).
    - `fitPxPerBar(0, 144) === 12`.
    - `dropIndex(sections, x, 20)` uses 20 px per bar.
  - `Arrangement.test.tsx`:
    - Clicking `"Open acid"` calls `onOpenTrack("acid")`.
    - The open track's header has `aria-pressed="true"`.
    - Mute and solo call `onMix` with `{ mute: true }` and `{ solo: true }`.
    - The existing move, resize, duplicate, delete, seek, loop and playhead tests pass unchanged.
- [ ] **Step 2: Run** `pnpm --filter @tune/ui test arrange Arrangement`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement.** In `StudioScreen`:
  - Pass `tracks` with mute and solo.
  - `onMix` fires `set_track_mix` with the other fields null.
  - Until Task 9, `openTrack` is null and `onOpenTrack` is a no-op.
  - Wrap the arrangement in a `.glass` section that takes the remaining height (`min-h-0 flex-1`).
- [ ] **Step 4: Run** `task verify`, `renderCost` included (jsdom keeps 12 px per bar, so `x1 = 42/16*12` holds). Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): rebuild the arrangement frame as the fit-to-width timeline`.

---

### Task 6: Help line and Sound tab (device chain)

**Files:**
- Create: `apps/desktop/src/screens/help.tsx`, `apps/desktop/src/screens/help.test.tsx`, `apps/desktop/src/screens/dock/SoundTab.tsx`, `apps/desktop/src/screens/dock/SoundTab.test.tsx`
- Modify: `packages/ui/src/components/FxPanel.tsx`, `packages/ui/src/components/FxPanel.test.tsx`, `packages/ui/src/components/TrackStrip.tsx`, `packages/ui/src/components/TrackStrip.test.tsx`

**Interfaces:**
- Consumes `Knob` with `color` (Task 2), and `Segmented` and `Fader` (Task 3).
- Help produces:
  - `HelpProvider({ children })`.
  - `useHelp(): { show(spec: ParamSpecView | null): void; note(text: string | null): void }`.
  - `HelpLine()`: a 36px row with a top border in `line`, 12px ink2, `aria-live="polite"`.
  - It shows the note when one is set, otherwise the spec's label (700 ink) plus its `help`, otherwise the idle line `"Hover or focus a knob to learn what it does."`.
  - These are the same three states and strings as `HelpStrip`; port `HelpStrip.test.tsx` into `help.test.tsx`.
- `FxPanel` becomes the device card:
  - A card with radius `rs`, `glass2` fill and a `line` border. The header row has the title in 700 12px and an optional `mode` slot on the right.
  - New optional props: `color?: string` (passed to the knobs) and `header?: ReactNode` (the mode slot).
  - `inactiveNote` and the dimming behaviour are kept.
- `TrackStrip` becomes the "Mixer strip" card:
  - Contents: `Fader` (gain) in the track colour, a pan `Knob` (sm), the `send.delay`, `send.reverb` and `duck.depth` knobs (sm), `noDuck` text, and `MuteSolo`.
  - The props are unchanged, plus `color?: string` and `compact?: boolean`. Compact is the narrow vertical strip used by the Mixer tab.
- Produces `SoundTab(props: { song: SongView; track: TrackView; automatedIn: number | null; highlights: Map<string, HighlightKind>; send(cmd: Command): Promise<unknown>; fire(cmd: Command): void })`. It renders a horizontal device chain that scrolls sideways inside itself:
  1. **Instrument card**, titled `instrumentTitle`, with knobs from `track.params`.
     - For `acid303`, `Panel303`'s `BIG` ids are `md` and its `SMALL` ids are `sm`, in that order.
     - Any other instrument shows all its params at `sm`, in `track.params` order.
  2. **One card per `track.fx` entry.**
     - `distortion` is titled "Distortion". Its header holds a `Segmented` labelled `"Distortion mode"` over `fx.modes`, which fires `set_fx_mode`.
     - `eq` is titled "EQ".
     - Any other kind is titled with `sentenceCase(kind)`.
  3. **The `TrackStrip` card** titled "Mixer strip".

  Knob values go through `useParamDrag`, keyed as today:
  - The highlight key is `tracks.<id>.<param>`.
  - A param is automated when the lanes of section `automatedIn` include `{ track: id, param }`.
  - Focus and hover call `useHelp().show`.
  - Carry over any `note(...)` that `Panel303` posts today, with the same text.

- [ ] **Step 1: Write the tests.**
  - `help.test.tsx`: the ported `HelpStrip` cases.
  - `SoundTab.test.tsx`, using `test-fixtures`-style songs:
    - The acid track shows sliders named for every `BIG` and `SMALL` param, a `"Distortion mode"` radiogroup, and a "Mixer strip" region.
    - Picking `hard` fires `{ cmd: "set_fx_mode", args: { track: "acid", mode: "hard" } }`.
    - A kick track shows one slider per `track.params` entry and no radiogroup when it has no distortion.
    - A Claude highlight on `tracks.acid.<cutoff id>` shows the "AI" badge on that knob.
  - Port the `Panel303.test.tsx` assertions that concern knobs and the mode switch.
  - Leave out the "303 tracks" tablist tests: each acid track is now its own timeline row and opens its own dock.
  - Update the `TrackStrip` and `FxPanel` tests for the new props, keeping the old assertions.
- [ ] **Step 2: Run** `pnpm -r test help SoundTab TrackStrip FxPanel`. Expected: FAIL.
- [ ] **Step 3: Implement.** `SoundTab` is not mounted anywhere yet; Task 9 mounts it.
- [ ] **Step 4: Run** `task verify`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(desktop): add the help line and the Sound tab's device chain`.

---

### Task 7: Steps tab (sequencer A, all-voice drum grid)

**Files:**
- Modify: `packages/ui/src/components/AcidStepSequencer.tsx` and its test, `packages/ui/src/components/DrumGrid.tsx` and its test
- Create: `apps/desktop/src/screens/dock/StepsTab.tsx`, `apps/desktop/src/screens/dock/StepsTab.test.tsx`

**Interfaces:**
- `AcidStepSequencer` gets the restyle only (`Seq303` A, columns):
  - Each step is a column: the note name in mono, a step light that is lit in `acid` when the step sounds, an accent pip, and a slide pip.
  - The column under the playhead gets a `--play` top bar. Every 4th column has a stronger `line2` left border.
  - A Claude highlight on a step gets a cyan outline.
  - Props, the piano popover, keyboard behaviour and accessible names (`"Step N note/accent/slide"`, group `"303 step sequencer"`) are unchanged. New optional prop: `color?: string`.
- `DrumGrid` (`DrumGrid` A, all voices):
  - `DrumRow` gains `color?: string` and `emphasis?: boolean`.
  - Hit cells fill with the row colour at 55%, and accent cells at 100%. The playing column gets a `--play` outline.
  - Rows that are not emphasised dim to 55% opacity until hovered.
  - Group names and step labels are unchanged.
- Produces `StepsTab(props: { song: SongView; track: TrackView; section: number | null; highlights: Map<string, HighlightKind>; fire(cmd: Command): void; send(cmd: Command): Promise<unknown> })`:
  - **Acid track.** The `LiveSequencer` moves here from `Panel303`, unchanged in behaviour (playhead-driven step, `highlightedSteps`, `set_acid_step`), passing `color`.
  - **Drum track.** It shows the `DrumSection` and `LiveDrumGrid` logic moved from `StudioScreen`: every drum track's pattern in `section`, the docked track's row with `emphasis`, and each row coloured by `trackColor`.
  - **Empty states.** These are exact copy:
    - The docked track has no pattern in the section: `"<track> plays nothing in <section name>. Pick a pattern above."`
    - `section === null`: `"Move the playhead into a section to see its steps."`

- [ ] **Step 1: Write `StepsTab.test.tsx`.**
  - The acid track shows the group `"303 step sequencer"`. Clicking a step's accent fires `set_acid_step` with `accent: true`.
  - A drum track shows one group per drum track that plays in the section. The docked row has `data-emphasis="true"`.
  - Empty state when the track has no pattern: the exact copy above.
  - Empty state when `section` is null: the exact copy.
  - A song with no drum tracks and an acid track docked renders the sequencer without error.
- [ ] **Step 2: Run** `pnpm -r test StepsTab AcidStepSequencer DrumGrid`. Expected: FAIL.
- [ ] **Step 3: Implement.** The old `DrumSection` stays in `StudioScreen` until Task 9 removes it. Share the grid code by importing it from `StepsTab.tsx` rather than copying it.
- [ ] **Step 4: Run** `task verify`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(desktop): add the Steps tab with sequencer A and the all-voice drum grid`.

---

### Task 8: Mixer tab

**Files:**
- Create: `apps/desktop/src/screens/dock/MixerTab.tsx`, `apps/desktop/src/screens/dock/MixerTab.test.tsx`

**Interfaces:**
- Consumes `TrackStrip` with `compact` and `FxPanel` (Task 6), and `useHelp` (Task 6).
- Produces `MixerTab(props: { song: SongView; automatedIn: number | null; highlights: Map<string, HighlightKind>; fire(cmd: Command): void; send(cmd: Command): Promise<unknown> })`. Per `MixerDock`, it lays out three things left to right:
  1. **Track strips.** One compact `TrackStrip` per track, coloured by `trackColor`. `MixerStrip`, `whyNoDuck` and the `set_track_mix` wiring move here from `StudioScreen` unchanged.
  2. **Bus cards.** "Delay" and "Reverb" `FxPanel`s, with `MixPanel`'s `UNUSED` inactive note when the bus is not active.
  3. **Master card.**
     - Shows the master mode read-only: `.lbl` "Mode", then "Clip" or "Limit". The protocol has no command to switch it, so there is no control.
     - Shows the `master_params` knobs. In clip mode, the limiter-only knobs are dimmed with `MixPanel`'s `LIMITER_OFF` note `"Off in clip mode"`.
     - Shows the duck source line with `MixPanel`'s wording.
  - The row scrolls sideways inside itself. There are no meters.
- [ ] **Step 1: Port `MixPanel.test.tsx`** into `MixerTab.test.tsx`, keeping every assertion on copy and dimming. Add:
  - One strip region per track (`"Track <id>"`).
  - Moving a bus knob calls `send` with `set_param` for track `"mix"`, exactly as `MixPanel` does today.
- [ ] **Step 2: Run** `pnpm --filter @tune/desktop test MixerTab`. Expected: FAIL.
- [ ] **Step 3: Implement**, reusing `MixPanel`'s helpers by moving them into `MixerTab.tsx`. `MixPanel.tsx` stays until Task 9 deletes it.
- [ ] **Step 4: Run** `task verify`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(desktop): add the Mixer dock tab`.

---

### Task 9: App shell, dock and change toast

**Files:**
- Create: `apps/desktop/src/screens/dock/dock.ts`, `dock.test.ts`, `Dock.tsx`, `Dock.test.tsx`, `PatternPicker.tsx`
- Modify: `apps/desktop/src/screens/StudioScreen.tsx`, `StudioScreen.test.tsx`, `apps/desktop/src/lib/useStudioKeys.ts`, `apps/desktop/src/screens/ChangeNotice.tsx`, `ChangeNotice.test.tsx`, `apps/desktop/src/App.test.tsx`
- Delete: `apps/desktop/src/screens/Panel303.tsx`, `Panel303.test.tsx`, `MixPanel.tsx`, `MixPanel.test.tsx`, `HelpStrip.tsx`, `HelpStrip.test.tsx`

**Interfaces:**
- Consumes `SoundTab`, `StepsTab`, `MixerTab`, `HelpProvider` and `HelpLine` (Tasks 6–8), the `Arrangement` props `openTrack` and `onOpenTrack` (Task 5), and the `TransportBar` props `mixerOpen` and `onToggleMixer` (Task 4).
- `dock.ts` produces:
  - `type DockState = { kind: "track"; track: string; tab: "sound" | "steps" } | { kind: "mixer" } | null`.
  - `type DockAction = { type: "openTrack"; track: string } | { type: "toggleMixer" } | { type: "tab"; tab: "sound" | "steps" } | { type: "close" } | { type: "prune"; tracks: string[] }`.
  - `dockReducer(s: DockState, a: DockAction): DockState`:
    - `openTrack` on the open track toggles the dock closed.
    - `openTrack` on another track opens it, keeping the current tab.
    - The default tab is `"sound"`.
    - `prune` closes the dock when the docked track is not in `tracks`.
  - `dockSection(state: StudioState, selected: number | null, current: number | null): number | null`. It returns `selected` when it is in range, else `current` when that is in range, else null.
- `useStudioKeys` gains `mixer(): void` (M, no modifiers, ignores repeat) and `escape(): void` (Escape). Both go through the existing `ignored()` check, so Esc pressed inside a Radix popover is left to the popover.
- `Dock(props: { song: SongView; dock: Exclude<DockState, null>; section: number | null; automatedIn: number | null; highlights: Map<string, HighlightKind>; send(cmd: Command): Promise<unknown>; fire(cmd: Command): void; onAction(a: DockAction): void })`:
  - A `.glass` region named `"Dock"`, with height `clamp(260px, 40vh, 360px)`. It opens with a 220ms height and opacity transition, which is instant under reduced motion.
  - **Track header.**
    - A chip with a colour dot, the track id and the instrument title.
    - `"<section name> · bars A–B"` (1-based, inclusive).
    - `PatternPicker`, then `"×N"` (`.badge`) when the shown pattern plays in N > 1 sections.
    - For acid only, a `role="tablist"` with "Sound" and "Steps".
    - A close `IconButton` named `"Close dock"`.
  - **Mixer header.** "Mixer" plus the close button.
  - **Body.**
    - An acid track shows the active tab.
    - A drum track shows `SoundTab` and `StepsTab` side by side (`grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]`).
    - The mixer shows `MixerTab`.
    - The body scrolls inside itself, with `HelpLine` at the bottom.
- `PatternPicker(props: { song: SongView; track: string; section: number; onPick(pattern: string | null): void })`:
  - Moves the arrangement cell popover's list here unchanged: the track's patterns plus "None".
  - The trigger is named `` `${track} in section ${section + 1}` ``, the same name the Arrangement popover uses today.
  - It fires `set_section_play`.
- `ChangeNotice` becomes the toast card, with the same props and the same text logic and strings:
  - `role="status"`, named `"Claude's change"` or `"Song file change"`.
  - Fixed at the top-right, below the transport (`top: 72px; right: 16px; width: 360px`). It uses `.glass-solid`, not blur.
  - A Claude notice gets a 1px `agent` border and a soft cyan glow. Its title line is `"Claude"` with a cyan dot.
  - A file notice gets a `line2` border with no cyan. Its title line is `"Song file"` with a hollow ink2 dot.
  - The body is the existing sentence. The actions are the existing buttons, restyled as `IconButton` size sm. Undo, Re-apply, Discard and Dismiss keep their accessible names.
- **`StudioScreen` shell.**
  - `<div className="relative flex h-screen min-w-[1280px] flex-col gap-2 overflow-hidden bg-bg p-2">`.
  - Three `.blob`s behind everything (`aria-hidden`, `pointer-events-none`), using kick, ride and acid at `--blobop`.
  - Then the banners, the transport, the timeline (`flex-1 min-h-0`), the dock when open, and the toast.
  - Dock state comes from `useReducer(dockReducer, null)`.
  - `useEffect` prunes the dock on every `song.tracks` change.
  - The keys:
    - M: `toggleMixer`.
    - Esc: `close`.
    - Clicking a track header: `openTrack`.
  - The dock's section is `dockSection(state, view.selectedSection, currentSection)`.
  - With the dock closed, `HelpLine` sits at the bottom of the timeline.
  - Wrap everything in `HelpProvider`.

- [ ] **Step 1: Write `dock.test.ts`.**

```ts
it("opens, switches and toggles a track", () => {
  let s = dockReducer(null, { type: "openTrack", track: "acid" });
  expect(s).toEqual({ kind: "track", track: "acid", tab: "sound" });
  s = dockReducer(s, { type: "tab", tab: "steps" });
  s = dockReducer(s, { type: "openTrack", track: "kick" });
  expect(s).toEqual({ kind: "track", track: "kick", tab: "steps" });
  expect(dockReducer(s, { type: "openTrack", track: "kick" })).toBeNull();
});
it("toggles the mixer and closes", () => {
  const m = dockReducer(null, { type: "toggleMixer" });
  expect(m).toEqual({ kind: "mixer" });
  expect(dockReducer(m, { type: "toggleMixer" })).toBeNull();
  expect(dockReducer(m, { type: "close" })).toBeNull();
});
it("closes when the docked track is gone", () => {
  const s = dockReducer(null, { type: "openTrack", track: "acid" });
  expect(dockReducer(s, { type: "prune", tracks: ["kick"] })).toBeNull();
  expect(dockReducer(s, { type: "prune", tracks: ["acid", "kick"] })).toBe(s);
});
it("falls back from a stale selected section", () => {
  const st = stateWithSections(3);
  expect(dockSection(st, 1, 0)).toBe(1);
  expect(dockSection(st, 5, 0)).toBe(0);
  expect(dockSection(st, 5, 9)).toBeNull();
  expect(dockSection(st, null, null)).toBeNull();
});
```

- [ ] **Step 2: Write `Dock.test.tsx` and update `StudioScreen.test.tsx`.**
  - Clicking `"Open acid"` shows region `"Dock"` with the tablist and Sound selected. Clicking Steps shows `"303 step sequencer"`.
  - Opening a kick track shows a slider and a drum group together, with no tablist.
  - Pressing M shows the mixer. Pressing M again hides it.
  - Esc closes the dock.
  - With the pattern picker open, Esc closes only the picker: the region `"Dock"` is still there. A second Esc closes it.
  - Picking a pattern fires `set_section_play` for the dock's section.
  - A state update that removes the docked track closes the dock without throwing.
  - `ChangeNotice.test.tsx` keeps every string assertion. It also checks the role `status` and the names `"Claude's change"` and `"Song file change"`.
  - Update `App.test.tsx` lookups that went through `Panel303` or the bottom drum section: open the dock first, then look.
- [ ] **Step 3: Run** `pnpm --filter @tune/desktop test`. Expected: FAIL.
- [ ] **Step 4: Implement.**
  - Delete `Panel303`, `MixPanel` and `HelpStrip`, and the `Mixer` and `DrumSection` code in `StudioScreen`, now that their logic lives in the dock files.
  - Keep `LiveTransport` and `LiveArrangement` as they are, so the render-cost properties hold.
- [ ] **Step 5: Run** `task verify`. Expected: PASS, `renderCost` included.
- [ ] **Step 6: Commit** `feat(desktop): build the studio shell with the module dock and change toast`.

---

### Task 10: Timeline blocks

**Files:**
- Modify: `apps/desktop/src/lib/song.ts`, `apps/desktop/src/lib/song.test.ts`, `packages/ui/src/components/Arrangement.tsx`, `packages/ui/src/components/Arrangement.test.tsx`, `apps/desktop/src/screens/StudioScreen.tsx`

**Interfaces:**
- Produces `patternGlyph(p: PatternView): number[]` in `lib/song.ts`.
  - Drum steps: rest 0, hit 0.6, accent 1.
  - Acid steps: rest 0, note 0.6, accented note 1.
- `ArrangementProps.patterns` becomes `{ id: string; track: string; kind: "acid" | "drum"; glyph: number[] }[]`.
- New prop `laneLabels?: Record<string, string>`, keyed `` `${track}.${param}` `` and holding the param label.
- `onSetPlay` is removed. Pattern choice now lives only in the dock's `PatternPicker` (Task 9).

Each (section, track) cell that plays a pattern becomes a block:
- Inset 3px, radius `rs`.
- The fill is the track colour at 22% and the border at 45%. The selected section's blocks go to 32% fill.
- The glyph spans the block:
  - Drums draw one bar per step, with height = value × 60% of the block.
  - Acid draws one 2px dash per note, with its y from the step index.
- The pattern id sits top-left in 10.5px 700 ink.
- A `.badge` `"×N"` sits top-right when the pattern plays in N > 1 sections.
- Blocks are SVG and are never blurred.
- Clicking a block calls `onSelect(i)` and `onOpenTrack(track)`.
- An empty cell is a faint `line` dashed outline on hover, and clicking it does the same.
- A Claude highlight on `sections[i].play.<track>` or `patterns.<id>.*` gets a cyan 1.5px outline that fades over 2.4s.
- Lanes:
  - Each (track, param) with a lane in any section gets a 22px row under its track. The row header reads `↳ <label>` in `.lbl` (indented), and the lane curve is drawn per section from `holdPoints`, keeping the `lane-overlay` test id.
  - Lanes for track `"mix"` go under a final "Mix" header row with no M/S.

- [ ] **Step 1: Write the tests.**
  - `song.test.ts`: `patternGlyph` for `x...X...` gives `[0.6,0,0,0,1,0,0,0]`. For an acid pattern of 4 steps `C2 . C2! .` it gives `[0.6,0,1,0]`.
  - `Arrangement.test.tsx`:
    - A block named `"<pattern> in <section name>"` is a button. Clicking it calls `onSelect(1)` and `onOpenTrack("kick")`.
    - `"×2"` shows for a pattern used twice.
    - No cell popover exists.
    - A lane row reads `↳` plus the label.
    - Remove the popover tests: their behaviour moved to `PatternPicker` in Task 9.
- [ ] **Step 2: Run** `pnpm -r test song Arrangement`. Expected: FAIL.
- [ ] **Step 3: Implement.** In `StudioScreen`:
  - Build `patterns` with `patternGlyph`, memoised on `song.patterns`.
  - Build `laneLabels` from the track params, the strip params and the mix params.
  - Wire `onOpenTrack` to also select the section.
- [ ] **Step 4: Run** `task verify`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): draw timeline blocks with pattern glyphs, link badges and lane rows`.

---

### Task 11: Cleanup, docs and visual check

**Files:**
- Modify: `packages/ui/src/theme/tokens.css`, `packages/ui/src/tailwind.preset.ts`, `apps/desktop/src/styles/app.css`, `apps/desktop/src/screens/Banners.tsx`, `apps/desktop/src/screens/OpenSongScreen.tsx`, `docs/superpowers/specs/2026-10-21-studio-design-handover.md`, `CLAUDE.md` (only if a line there describes the old UI layout)

- [ ] **Step 1: Restyle the remaining screens.**
  - `Banners`: a full-width strip above the transport with radius `rs`, `glass-solid`, and the kick colour for errors. There is no cyan.
  - `OpenSongScreen`: a centred `.glass` card on the blob background, listing songs as rows.
  - Keep all copy and accessible names.
- [ ] **Step 2: Remove the aliases and dead styles.**
  - Delete the Task 1 alias tokens, their preset entries, `font-label` and `font-value`, and the `.faceplate` and `.screw` CSS.
  - Run `rg -n "chassis|faceplate|silkscreen|\bled\b|\bslide\b|text-claude|bg-claude|font-label|font-value|Barlow" packages apps -g '!node_modules'`. Expected: no hits, apart from the domain word "slide" in acid step code and copy. Replace each class hit with its new name.
- [ ] **Step 3: Record the picks in the handover.** In §4, add a "Picked (2026-10-24)" column with the producer's picks table from this plan. Add one line saying that Plan 6 built look A on the existing features, and that the rest of the design waits for Phases 2–5.
- [ ] **Step 4: Run** `task verify`. Expected: PASS.
- [ ] **Step 5: Do the visual check.** Do not commit its output.
  - Run `task ui:build`, then `tune serve songs/first-acid/song.toml --device null --port 4580` in the background.
  - Use the agent-browser skill at 1440×900 and at 1280×720.
  - Screenshot to `scratch/plan6-*.png`. Check: no page scroll (`agent-browser eval "document.scrollingElement.scrollHeight <= innerHeight"` prints `true`), at most 3 elements with a computed `backdrop-filter` other than `none`, and cyan only on Claude marks.
  - Do this with the dock closed, then on acid Sound, acid Steps, kick and the Mixer.
  - Compare each screenshot against the matching board.
  - Fix differences in the owning component, with a commit per concern.
  - Stop the server.
- [ ] **Step 6: Commit** `refactor(ui): drop the faceplate theme and record the design picks`.
