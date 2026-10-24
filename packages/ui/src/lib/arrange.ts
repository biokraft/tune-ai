export type SectionView = {
  name: string;
  start_bar: number;
  bars: number;
  play: Record<string, string>;
  lanes: { track: string; param: string; points: [number, number][] }[];
};

export const PX_PER_BAR = 12;

/** Resize snap: 8 bars (minimum 8), or 1 bar (minimum 1) when `fine`. */
export function snapBars(bars: number, fine: boolean): number {
  return fine ? Math.max(1, Math.round(bars)) : Math.max(8, Math.round(bars / 8) * 8);
}

/** Index a dragged block lands at: the number of section midpoints left of `x`. */
export function dropIndex(sections: SectionView[], x: number): number {
  let i = 0;
  for (const s of sections) {
    const mid = (s.start_bar + s.bars / 2) * PX_PER_BAR;
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
 * Lane points as drawn: x clamped to the section, held flat from the section
 * start to the first point and from the last point to the section end, like
 * the engine. A single point becomes a flat line across the section.
 */
export function holdPoints(points: [number, number][], bars: number): [number, number][] {
  if (points.length === 0) return [];
  const pts = points.map(([x, v]): [number, number] => [Math.min(bars, Math.max(0, x)), v]);
  const first = pts[0]!;
  const last = pts[pts.length - 1]!;
  if (first[0] > 0) pts.unshift([0, first[1]]);
  if (last[0] < bars) pts.push([bars, last[1]]);
  return pts;
}
