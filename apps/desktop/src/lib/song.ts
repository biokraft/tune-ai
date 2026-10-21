import type { SectionView } from "@tune/ui";
import type { PatternView, SongView, StudioState } from "../client/protocol";
import type { StudioView } from "../store/studioStore";

/** 16ths per bar. */
export const STEPS_PER_BAR = 16;

/** "Env Mod" → "Env mod": UI labels are sentence case. */
export function sentenceCase(label: string): string {
  return label.charAt(0).toUpperCase() + label.slice(1).toLowerCase();
}

/** Index of the section holding 0-based `bar`, or null past the end. */
export function sectionAtBar(sections: SectionView[], bar: number): number | null {
  const i = sections.findIndex((s) => bar >= s.start_bar && bar < s.start_bar + s.bars);
  return i < 0 ? null : i;
}

/** Whether the studio is sounding and the playhead is known. */
export function playheadStep(view: StudioView): number | null {
  return view.state?.playing && view.playhead ? view.playhead.step : null;
}

/** The section the editors show: the selected one, else the one under the playhead or cursor. */
export function currentSection(view: StudioView): number | null {
  const song = view.state?.song;
  if (!song || song.sections.length === 0) return null;
  if (view.selectedSection !== null) return view.selectedSection;
  const step = playheadStep(view);
  const bar = step !== null ? Math.floor(step / STEPS_PER_BAR) : view.state!.cursor_bar;
  return sectionAtBar(song.sections, bar) ?? 0;
}

/** The pattern `track` plays in section `i`, if any. */
export function patternIn(song: SongView, i: number | null, track: string): PatternView | null {
  if (i === null) return null;
  const id = song.sections[i]?.play[track];
  return song.patterns.find((p) => p.id === id) ?? null;
}

/**
 * Pattern-relative index of the sounding step, when the playhead is inside section `i`.
 * The engine restarts every pattern at its section start.
 */
export function stepInPattern(
  song: SongView,
  i: number | null,
  step: number | null,
  length: number,
): number | null {
  if (i === null || step === null || length === 0) return null;
  const s = song.sections[i];
  if (!s) return null;
  const offset = step - s.start_bar * STEPS_PER_BAR;
  if (offset < 0 || offset >= s.bars * STEPS_PER_BAR) return null;
  return offset % length;
}

/** The step indices of `pattern` highlighted in `highlights`. */
export function highlightedSteps(highlights: Map<string, unknown>, pattern: string): Set<number> {
  const prefix = `patterns.${pattern}.steps[`;
  const out = new Set<number>();
  for (const key of highlights.keys()) {
    if (key.startsWith(prefix)) out.add(Number(key.slice(prefix.length, -1)));
  }
  return out;
}

/** The `[start, end)` bars of section `i`. */
export function sectionBars(state: StudioState, i: number): [number, number] | null {
  const s = state.song?.sections[i];
  return s ? [s.start_bar, s.start_bar + s.bars] : null;
}

/** A number as people read it: at most 4 significant digits, no trailing zeros. */
export function formatNumber(x: number): string {
  return String(Number(x.toPrecision(4)));
}
