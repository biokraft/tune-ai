import type { SectionView } from "@tune/ui";
import type { PatternView, SongView, StudioState } from "../client/protocol";
import type { HighlightKind, StudioView } from "../store/studioStore";

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

/** Name of the section whose bars equal the half-open `loop`, else null. */
export function loopSectionName(
  sections: SectionView[],
  loop: [number, number] | null,
): string | null {
  if (loop === null) return null;
  const s = sections.find((x) => x.start_bar === loop[0] && x.start_bar + x.bars === loop[1]);
  return s?.name ?? null;
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

/**
 * The clips `track` plays in part of section `i`, or from mid-clip, when the section plays no one
 * clip for it (`play[track]` is unset): the distinct clip ids of the placements that overlap the
 * section, in time order. Empty when the track is silent there or plays one clip over it.
 */
export function partialClipsIn(song: SongView, i: number, track: string): string[] {
  const s = song.sections[i];
  if (!s || s.play[track] !== undefined) return [];
  const end = s.start_bar + s.bars;
  const placements = song.tracks.find((t) => t.id === track)?.placements ?? [];
  const clips = placements
    .filter((p) => p.start_bar < end && s.start_bar < p.start_bar + p.bars)
    .map((p) => p.clip);
  return [...new Set(clips)];
}

/** The pattern `track` plays in section `i`, if any. */
export function patternIn(song: SongView, i: number | null, track: string): PatternView | null {
  if (i === null) return null;
  const id = song.sections[i]?.play[track];
  return song.patterns.find((p) => p.id === id) ?? null;
}

/**
 * A pattern's shape for its timeline block, one value per step: a rest is 0, a hit or a note 0.6
 * and an accented one 1. A note clip has `16 * bars` values; each note marks the step it starts in.
 */
export function patternGlyph(p: PatternView): number[] {
  if (p.kind === "notes") {
    const glyph = new Array<number>(STEPS_PER_BAR * p.bars).fill(0);
    for (const n of p.notes) {
      const i = Math.floor(n.start);
      if (i >= 0 && i < glyph.length) glyph[i] = Math.max(glyph[i]!, n.vel >= 110 ? 1 : 0.6);
    }
    return glyph;
  }
  if (p.kind === "drum") return p.hits.map((h) => (h === "accent" ? 1 : h === "hit" ? 0.6 : 0));
  return p.steps.map((s) => (s.note === null ? 0 : s.accent ? 1 : 0.6));
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

/** The step indices of `pattern` highlighted in `highlights`, each with who changed it. */
export function highlightedSteps(
  highlights: ReadonlyMap<string, HighlightKind>,
  pattern: string,
): Map<number, HighlightKind> {
  const prefix = `clips.${pattern}.steps[`;
  const out = new Map<number, HighlightKind>();
  for (const [key, kind] of highlights) {
    if (key.startsWith(prefix)) out.set(Number(key.slice(prefix.length, -1)), kind);
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

/**
 * Why `track` gets no duck knob: the duck source would duck itself, and without a source nothing
 * ducks. Validation refuses both, so a drag would be heard but its release never written.
 */
export function whyNoDuck(source: string | null, track: string): string | null {
  if (source === null) return "No kick";
  return source === track ? "Duck source" : null;
}
