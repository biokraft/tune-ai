import { describe, expect, it } from "vitest";
import { dropIndex, formatPosition, snapBars, type SectionView } from "./arrange";

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
  it("formats positions one-based", () => {
    expect(formatPosition(32, 1)).toBe("033:2");
    expect(formatPosition(0, 0)).toBe("001:1");
  });
  it("drops by section midpoints", () => {
    const s = [sec(0, 16), sec(16, 16), sec(32, 32)];
    expect(dropIndex(s, 10)).toBe(0);
    expect(dropIndex(s, 300)).toBe(2);
    expect(dropIndex(s, 5000)).toBe(3);
  });
});
