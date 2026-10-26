/** One automation stretch as one section shows it. */
export type LaneView = {
  track: string;
  param: string;
  /** `[bar offset from the section start, normalized value]`; may lie outside the section. */
  points: [number, number][];
  /** The stretch's part of the section, in bars from its start: `0 <= from < to <= bars`. */
  from: number;
  to: number;
};

export type SectionView = {
  name: string;
  start_bar: number;
  bars: number;
  /**
   * Track id to clip id, for each track that plays one clip over the whole section: one placement
   * that starts at the section start from the clip's first step (offset 0).
   */
  play: Record<string, string>;
  /** The stretches that overlap the section. */
  lanes: LaneView[];
};

/** One clip placed on a track. */
export type PlacementView = {
  /** 0-based. */
  start_bar: number;
  bars: number;
  clip: string;
  /** Bars into the clip at which the placement starts playing it. */
  offset: number;
  /** The section whose `play` shows this placement, or null when none does. */
  in_section: number | null;
};

/**
 * The bars of placement `p` that no section's block shows, as half-open `[start, end)` ranges: all
 * of it when no section shows it, else what runs past the end of the one section that does. A
 * section shows a placement only when it starts there from the clip's first step, so the part of a
 * placement inside any later section is loose.
 */
export function loosePieces(p: PlacementView, sections: SectionView[]): [number, number][] {
  const start = p.start_bar;
  const end = p.start_bar + p.bars;
  const shown = p.in_section === null ? undefined : sections[p.in_section];
  if (!shown) return [[start, end]];
  const from = shown.start_bar + shown.bars;
  return from < end ? [[from, end]] : [];
}

/** Bar width when nothing is measured (jsdom, first paint). */
export const PX_PER_BAR = 12;
/** Fit-to-width limits: long songs scroll below the floor, short ones stop growing at the cap. */
export const MIN_PX_PER_BAR = 6;
export const MAX_PX_PER_BAR = 40;
/** The timeline's sticky track-header column. */
export const HEADER_W = 168;

/** Bar width that fits `totalBars` beside the track headers; a width of 0 or less gives `PX_PER_BAR`. */
export function fitPxPerBar(width: number, totalBars: number): number {
  if (!(width > 0)) return PX_PER_BAR;
  const fit = (width - HEADER_W) / Math.max(totalBars, 1);
  return Math.min(MAX_PX_PER_BAR, Math.max(MIN_PX_PER_BAR, fit));
}

/** Bars between ruler labels: 4 from 16 px per bar, else 8 or 16, keeping labels 40 px apart. */
export function rulerStep(pxPerBar: number): number {
  if (pxPerBar >= 16) return 4;
  for (const step of [8, 16]) if (step * pxPerBar >= 40) return step;
  return Math.ceil(40 / pxPerBar / 16) * 16;
}

/** Bars with a ruler label (and a grid line), from 0 up to but not including `bars`. */
export function gridBars(bars: number, step: number): number[] {
  const out: number[] = [];
  for (let b = 0; b < bars - 1e-9; b += step) out.push(b);
  return out;
}

/** Resize snap: 8 bars (minimum 8), or 1 bar (minimum 1) when `fine`. */
export function snapBars(bars: number, fine: boolean): number {
  return fine ? Math.max(1, Math.round(bars)) : Math.max(8, Math.round(bars / 8) * 8);
}

/** Index a dragged block lands at: the number of section midpoints left of `x`. */
export function dropIndex(sections: SectionView[], x: number, pxPerBar = PX_PER_BAR): number {
  let i = 0;
  for (const s of sections) {
    const mid = (s.start_bar + s.bars / 2) * pxPerBar;
    if (mid < x) i += 1;
  }
  return i;
}

/** Zero-based bar and beat in, one-based "88.3" out. */
export function formatPosition(bar: number, beat: number): string {
  return `${bar + 1}.${beat + 1}`;
}

/** Elapsed time at a zero-based bar and beat (4/4) as "m:ss.d". */
export function formatTime(bar: number, beat: number, bpm: number): string {
  const tenths = Math.floor(((bar * 4 + beat) * 60 * 10) / bpm + 1e-9);
  const m = Math.floor(tenths / 600);
  const s = Math.floor((tenths % 600) / 10);
  return `${m}:${String(s).padStart(2, "0")}.${tenths % 10}`;
}

/**
 * The lane's value at bar `x` of `points` (bars never decreasing), as the engine reads it: the
 * first point's value before it, linear between points, the last point's value after it, and the
 * later point where two share a bar.
 */
export function laneValue(points: [number, number][], x: number): number {
  let i = 0;
  while (i < points.length && points[i]![0] <= x) i += 1;
  if (i === 0) return points[0]![1];
  if (i === points.length) return points[i - 1]![1];
  const [b0, v0] = points[i - 1]!;
  const [b1, v1] = points[i]!;
  return v0 + ((v1 - v0) * (x - b0)) / (b1 - b0);
}

/**
 * Lane points as drawn over a stretch's part of a section, `[from, to]` in bars from the section
 * start: the value at `from`, the points strictly inside, then the value at `to`. Before the
 * first point and after the last the value holds flat, like the engine, so a single point is a
 * flat line across the part.
 */
export function holdPoints(
  points: [number, number][],
  from: number,
  to: number,
): [number, number][] {
  if (points.length === 0 || !(to > from)) return [];
  const inside = points.filter(([x]) => x > from && x < to);
  return [[from, laneValue(points, from)], ...inside, [to, laneValue(points, to)]];
}
