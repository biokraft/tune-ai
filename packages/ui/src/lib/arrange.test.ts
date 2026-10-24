import { describe, expect, it } from "vitest";
import {
  dropIndex,
  holdPoints,
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
  it("holds lane points flat to the section edges", () => {
    expect(
      holdPoints(
        [
          [1, 0.2],
          [2, 0.8],
        ],
        4,
      ),
    ).toEqual([
      [0, 0.2],
      [1, 0.2],
      [2, 0.8],
      [4, 0.8],
    ]);
    expect(holdPoints([[2, 0.5]], 4)).toEqual([
      [0, 0.5],
      [2, 0.5],
      [4, 0.5],
    ]);
    expect(
      holdPoints(
        [
          [-1, 0],
          [9, 1],
        ],
        4,
      ),
    ).toEqual([
      [0, 0],
      [4, 1],
    ]);
    expect(holdPoints([], 4)).toEqual([]);
  });
});
