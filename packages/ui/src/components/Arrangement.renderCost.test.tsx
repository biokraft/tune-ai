import { render } from "@testing-library/react";
import { Profiler } from "react";
import { describe, expect, it, vi } from "vitest";
import * as arrange from "../lib/arrange";
import type { SectionView } from "../lib/arrange";
import { Arrangement, type ArrangementProps } from "./Arrangement";

// The ruler and the row grid each lay out their bar lines with gridBars: counting its calls
// tells a ruler render from a body render.
vi.mock("../lib/arrange", async (importOriginal) => {
  const real = await importOriginal<typeof import("../lib/arrange")>();
  return { ...real, gridBars: vi.fn(real.gridBars) };
});

const sections: SectionView[] = [
  { name: "a", start_bar: 0, bars: 16, play: {}, lanes: [] },
  { name: "b", start_bar: 16, bars: 16, play: {}, lanes: [] },
];
const tracks = [{ id: "acid", instrument: "acid303", mute: false, solo: false }];
const props: Omit<ArrangementProps, "playhead"> = {
  sections,
  tracks,
  patterns: [],
  selected: null,
  loop: [4, 8],
  openTrack: null,
  onSelect: vi.fn(),
  onSeek: vi.fn(),
  onLoop: vi.fn(),
  onMove: vi.fn(),
  onResize: vi.fn(),
  onDuplicate: vi.fn(),
  onDelete: vi.fn(),
  onOpenTrack: vi.fn(),
  onShowTrack: vi.fn(),
  onMix: vi.fn(),
};

describe("Arrangement render cost", () => {
  it("a playhead tick redraws the row grid but not the memoised ruler", () => {
    const gridBars = vi.mocked(arrange.gridBars);
    let commits = 0;
    const ui = (playhead: number) => (
      <Profiler id="arrangement" onRender={() => (commits += 1)}>
        <Arrangement {...props} playhead={playhead} />
      </Profiler>
    );
    const { rerender } = render(ui(0));
    // Mounting lays out both: the ruler's labels and the rows' grid lines.
    expect(gridBars.mock.calls.length).toBeGreaterThanOrEqual(2);
    gridBars.mockClear();
    commits = 0;
    for (let i = 1; i <= 8; i++) rerender(ui(i / 16));
    expect(commits).toBeGreaterThanOrEqual(8);
    // One call per arrangement render, for the row grid: the ruler never rendered again.
    expect(gridBars).toHaveBeenCalledTimes(commits);
  });
});
