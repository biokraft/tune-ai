import { describe, expect, it } from "vitest";
import {
  dropIndex,
  fitPxPerBar,
  gridBars,
  HEADER_W,
  holdPoints,
  laneValue,
  loosePieces,
  MAX_PX_PER_BAR,
  MIN_PX_PER_BAR,
  rulerStep,
  formatPosition,
  formatTime,
  snapBars,
  type SectionView,
} from "./arrange";

const sec = (start_bar: number, bars: number): SectionView => ({
  name: "s",
  start_bar,
  bars,
  play: {},
  lanes: [],
});

describe("arrange", () => {
  it("snaps to 8 bars, or 1 when fine", () => {
    expect(snapBars(21, false)).toBe(24);
    expect(snapBars(3, false)).toBe(8);
    expect(snapBars(21, true)).toBe(21);
    expect(snapBars(0, true)).toBe(1);
  });
  it("formats positions one-based, unpadded", () => {
    expect(formatPosition(87, 2)).toBe("88.3");
    expect(formatPosition(0, 0)).toBe("1.1");
  });
  it("formats elapsed time as m:ss.d", () => {
    // (87*4 + 2) * 60 / 145 = 144.83 s
    expect(formatTime(87, 2, 145)).toBe("2:24.8");
    expect(formatTime(0, 0, 145)).toBe("0:00.0");
  });
  it("drops by section midpoints", () => {
    const s = [sec(0, 16), sec(16, 16), sec(32, 32)];
    expect(dropIndex(s, 10)).toBe(0);
    expect(dropIndex(s, 300)).toBe(2);
    expect(dropIndex(s, 5000)).toBe(3);
  });
  it("drops by section midpoints at the drawn bar width", () => {
    const s = [sec(0, 16), sec(16, 16), sec(32, 32)];
    // Midpoints at 20 px per bar: 160, 480, 960; at the default 12: 96, 288, 576.
    expect(dropIndex(s, 300, 20)).toBe(1);
    expect(dropIndex(s, 300)).toBe(2);
    expect(dropIndex(s, 500, 20)).toBe(2);
  });
  it("fits the song to the width beside the track headers, within limits", () => {
    expect(HEADER_W).toBe(168);
    expect(MIN_PX_PER_BAR).toBe(6);
    expect(MAX_PX_PER_BAR).toBe(40);
    expect(fitPxPerBar(1168, 100)).toBe(10);
    expect(fitPxPerBar(1168, 4)).toBe(40);
    expect(fitPxPerBar(1168, 1000)).toBe(6);
    expect(fitPxPerBar(1168, 0)).toBe(40);
  });
  it("falls back to 12 px per bar without a measured width", () => {
    expect(fitPxPerBar(0, 144)).toBe(12);
    expect(fitPxPerBar(-5, 144)).toBe(12);
  });
  it("labels the ruler every 4 bars from 16 px per bar, else every 8 or 16, 40 px apart", () => {
    expect(rulerStep(40)).toBe(4);
    expect(rulerStep(16)).toBe(4);
    expect(rulerStep(12)).toBe(8);
    expect(rulerStep(6)).toBe(8);
    expect(rulerStep(4)).toBe(16);
  });
  it("holds lane points flat to the edges of the stretch's part", () => {
    expect(
      holdPoints(
        [
          [1, 0.2],
          [2, 0.8],
        ],
        0,
        4,
      ),
    ).toEqual([
      [0, 0.2],
      [1, 0.2],
      [2, 0.8],
      [4, 0.8],
    ]);
    expect(holdPoints([[2, 0.5]], 0, 4)).toEqual([
      [0, 0.5],
      [2, 0.5],
      [4, 0.5],
    ]);
    expect(holdPoints([], 0, 4)).toEqual([]);
  });
  it("cuts points outside the part at the value the engine plays there", () => {
    // A ramp from bar -1 to 9 seen over bars 0 to 4.
    const ramp: [number, number][] = [
      [-1, 0],
      [9, 1],
    ];
    expect(holdPoints(ramp, 0, 4)).toEqual([
      [0, expect.closeTo(0.1)],
      [4, expect.closeTo(0.5)],
    ]);
    // A stretch on the second half of a 4-bar section.
    expect(
      holdPoints(
        [
          [2, 0.1],
          [4, 0.8],
        ],
        2,
        4,
      ),
    ).toEqual([
      [2, 0.1],
      [4, 0.8],
    ]);
    // A jump at the part's start plays its later value.
    expect(
      holdPoints(
        [
          [1, 0],
          [1, 1],
          [3, 0],
        ],
        1,
        3,
      ),
    ).toEqual([
      [1, 1],
      [3, 0],
    ]);
  });
  it("reads a lane like the engine", () => {
    const pts: [number, number][] = [
      [0, 0.2],
      [2, 0.6],
      [2, 1],
    ];
    expect(laneValue(pts, -1)).toBe(0.2);
    expect(laneValue(pts, 1)).toBeCloseTo(0.4);
    expect(laneValue(pts, 2)).toBe(1);
    expect(laneValue(pts, 5)).toBe(1);
  });
});

describe("loosePieces", () => {
  const sections = [sec(0, 16), sec(16, 16), sec(32, 8)];
  const at = (start_bar: number, bars: number, in_section: number | null) => ({
    start_bar,
    bars,
    clip: "a1",
    offset: 0,
    in_section,
  });
  it("is the whole placement when no section shows it", () => {
    expect(loosePieces(at(18, 4, null), sections)).toEqual([[18, 22]]);
    expect(loosePieces(at(8, 16, null), sections)).toEqual([[8, 24]]);
  });
  it("is nothing for a placement its section shows whole", () => {
    expect(loosePieces(at(0, 16, 0), sections)).toEqual([]);
    expect(loosePieces(at(16, 16, 1), sections)).toEqual([]);
  });
  it("is what runs past the end of the section that shows it", () => {
    expect(loosePieces(at(0, 20, 0), sections)).toEqual([[16, 20]]);
    // A later section plays it mid-clip, so it shows none of it.
    expect(loosePieces(at(0, 32, 0), sections)).toEqual([[16, 32]]);
    expect(loosePieces(at(16, 24, 1), sections)).toEqual([[32, 40]]);
  });
});

describe("gridBars", () => {
  it("labels every step bars from 0, short of the end", () => {
    expect(gridBars(32, 8)).toEqual([0, 8, 16, 24]);
    expect(gridBars(33, 8)).toEqual([0, 8, 16, 24, 32]);
    expect(gridBars(0, 8)).toEqual([]);
  });
});
