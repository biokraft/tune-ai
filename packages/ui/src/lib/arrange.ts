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

/** Zero-based bar and beat in, one-based "033:2" out. */
export function formatPosition(bar: number, beat: number): string {
  return `${String(bar + 1).padStart(3, "0")}:${beat + 1}`;
}
