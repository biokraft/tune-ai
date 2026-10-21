import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { SectionView } from "../lib/arrange";
import { Arrangement } from "./Arrangement";

const sec = (name: string, start_bar: number, bars: number, extra = {}): SectionView => ({
  name,
  start_bar,
  bars,
  play: {},
  lanes: [],
  ...extra,
});

function setup(sections: SectionView[], extra: Record<string, unknown> = {}) {
  const fns = {
    onSelect: vi.fn(),
    onSeek: vi.fn(),
    onMove: vi.fn(),
    onResize: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
    onSetPlay: vi.fn(),
  };
  const utils = render(
    <div>
      <button>outside</button>
      <Arrangement
        sections={sections}
        tracks={[
          { id: "acid", instrument: "acid303" },
          { id: "drums", instrument: "drums" },
        ]}
        patterns={[
          { id: "a1", track: "acid" },
          { id: "a2", track: "acid" },
          { id: "d1", track: "drums" },
        ]}
        playhead={0}
        selected={null}
        loop={null}
        {...fns}
        {...extra}
      />
    </div>,
  );
  return { ...fns, ...utils };
}

const block = (i: number) => screen.getByTestId(`section-${i}`);

describe("Arrangement", () => {
  it("blocks sized by bars", () => {
    setup([sec("intro", 0, 16)]);
    expect(block(0)).toHaveAttribute("width", "192");
  });

  it("drag body reorders", () => {
    const { onMove } = setup([sec("a", 0, 16), sec("b", 16, 16)]);
    fireEvent.pointerDown(block(1), { clientX: 200, pointerId: 1 });
    fireEvent.pointerMove(block(1), { clientX: 13, pointerId: 1 });
    fireEvent.pointerUp(block(1), { clientX: 13, pointerId: 1 });
    expect(onMove).toHaveBeenCalledWith(1, 0);
  });

  it("a reorder selects the moved section and ignores the trailing click", () => {
    const { onMove, onSelect, onSeek } = setup([sec("a", 0, 16), sec("b", 16, 16)]);
    fireEvent.pointerDown(block(1), { clientX: 200, pointerId: 1 });
    fireEvent.pointerMove(block(1), { clientX: 13, pointerId: 1 });
    fireEvent.pointerUp(block(1), { clientX: 13, pointerId: 1 });
    expect(onMove).toHaveBeenCalledWith(1, 0);
    expect(onSelect).toHaveBeenCalledWith(0);
    onSelect.mockClear();
    fireEvent.click(block(1));
    fireEvent.doubleClick(block(1));
    expect(onSelect).not.toHaveBeenCalled();
    expect(onSeek).not.toHaveBeenCalled();
  });

  it("body jitter does not move", () => {
    const { onMove } = setup([sec("a", 0, 16), sec("b", 16, 16)]);
    fireEvent.pointerDown(block(1), { clientX: 200, pointerId: 1 });
    fireEvent.pointerMove(block(1), { clientX: 203, pointerId: 1 });
    fireEvent.pointerUp(block(1), { clientX: 203, pointerId: 1 });
    expect(onMove).not.toHaveBeenCalled();
  });

  it("edge drag snaps to 8, Alt is fine", () => {
    const { onResize } = setup([sec("a", 0, 16)]);
    const h = screen.getByTestId("resize-0");
    fireEvent.pointerDown(h, { clientX: 192, pointerId: 1 });
    fireEvent.pointerMove(h, { clientX: 252, pointerId: 1 });
    fireEvent.pointerUp(h, { clientX: 252, pointerId: 1 });
    expect(onResize).toHaveBeenLastCalledWith(0, 24);
    fireEvent.pointerDown(h, { clientX: 192, pointerId: 1 });
    fireEvent.pointerMove(h, { clientX: 252, pointerId: 1, altKey: true });
    fireEvent.pointerUp(h, { clientX: 252, pointerId: 1, altKey: true });
    expect(onResize).toHaveBeenLastCalledWith(0, 21);
  });

  it("tiny edge jitter does nothing", () => {
    const { onResize } = setup([sec("a", 0, 16)]);
    const h = screen.getByTestId("resize-0");
    fireEvent.pointerDown(h, { clientX: 192, pointerId: 1 });
    fireEvent.pointerMove(h, { clientX: 194, pointerId: 1 });
    fireEvent.pointerUp(h, { clientX: 194, pointerId: 1 });
    expect(onResize).not.toHaveBeenCalled();
  });

  it("click selects, double click seeks", () => {
    const { onSelect, onSeek } = setup([sec("a", 0, 16), sec("b", 16, 16)]);
    fireEvent.click(block(1));
    expect(onSelect).toHaveBeenCalledWith(1);
    fireEvent.doubleClick(block(1));
    expect(onSeek).toHaveBeenCalledWith(16);
  });

  it("cell picks a pattern, None clears", async () => {
    const user = userEvent.setup();
    const { onSetPlay } = setup([sec("a", 0, 16)]);
    await user.click(screen.getByRole("button", { name: "acid in section 1" }));
    await user.click(screen.getByRole("button", { name: "a1" }));
    expect(onSetPlay).toHaveBeenCalledWith(0, "acid", "a1");
    await user.click(screen.getByRole("button", { name: "acid in section 1" }));
    expect(screen.queryByRole("button", { name: "d1" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "None" }));
    expect(onSetPlay).toHaveBeenLastCalledWith(0, "acid", null);
  });

  it("cell shows the playing pattern", () => {
    setup([sec("a", 0, 16, { play: { acid: "a2" } })]);
    expect(screen.getByRole("button", { name: "acid in section 1" })).toHaveTextContent("a2");
  });

  it("lanes are drawn", () => {
    const lanes = [
      {
        track: "acid",
        param: "cutoff",
        points: [
          [0, 0],
          [8, 1],
        ] as [number, number][],
      },
      {
        track: "acid",
        param: "res",
        points: [
          [0, 0.5],
          [16, 0.5],
        ] as [number, number][],
      },
    ];
    const { container } = setup([sec("a", 0, 16, { lanes })]);
    expect(container.querySelectorAll("polyline")).toHaveLength(2);
  });

  it("lane curves stay visible above compact cells", () => {
    setup([sec("a", 0, 16)]);
    const cell = screen.getByRole("button", { name: "acid in section 1" });
    expect(parseFloat(cell.style.maxWidth)).toBeLessThan(16 * 12);
    const overlay = screen.getByTestId("lane-overlay");
    expect(overlay).toHaveClass("pointer-events-none");
    expect(overlay).toHaveClass("z-10");
  });

  it("pattern cells fit the whole pattern name, up to the section width", () => {
    setup([sec("a", 0, 16, { play: { acid: "acid_main" } })]);
    const cell = screen.getByRole("button", { name: "acid in section 1" });
    expect(cell).toHaveTextContent("acid_main");
    expect(cell).toHaveAttribute("title", "acid_main");
    expect(cell.style.width).toBe("");
    expect(cell.style.maxWidth).toBe(`${16 * 12 - 4}px`);
  });

  it("labels each row with the track id and instrument", () => {
    setup([sec("a", 0, 16)]);
    const heads = screen.getAllByRole("rowheader");
    expect(heads).toHaveLength(2);
    expect(heads[0]).toHaveTextContent("acid");
    expect(heads[0]).toHaveTextContent("acid303");
    expect(heads[1]).toHaveTextContent("drums");
    expect(screen.getByRole("rowgroup", { name: "Tracks" })).toHaveClass("sticky", "left-0");
  });

  it("lanes hold flat to the section edges and single points span the section", () => {
    const lanes = [
      {
        track: "acid",
        param: "cutoff",
        points: [
          [1, 0.2],
          [2, 0.8],
        ] as [number, number][],
      },
      { track: "acid", param: "res", points: [[2, 0.5]] as [number, number][] },
    ];
    const { container } = setup([sec("a", 16, 4, { lanes })]);
    const polys = container.querySelectorAll("polyline");
    expect(polys).toHaveLength(2);
    for (const poly of polys) {
      const xs = poly
        .getAttribute("points")!
        .split(" ")
        .map((pt) => parseFloat(pt.split(",")[0]!));
      expect(xs[0]).toBe(16 * 12);
      expect(xs[xs.length - 1]).toBe(20 * 12);
    }
  });

  it("playhead x", () => {
    setup([sec("a", 0, 16)], { playhead: 4.5 });
    expect(screen.getByTestId("playhead")).toHaveAttribute("x1", "54");
  });

  it("ruler ticks every 8 bars, one-based", () => {
    setup([sec("a", 0, 16)]);
    expect(screen.getByText("9")).toBeInTheDocument();
  });

  it("highlighted paths ring the block", () => {
    setup([sec("a", 0, 16), sec("b", 16, 16)], { highlighted: new Set(["sections[1].bars"]) });
    expect(block(0)).not.toHaveAttribute("data-highlight");
    expect(block(1)).toHaveAttribute("data-highlight", "claude");
  });

  it("keys act on the selected section, only inside the arrangement", async () => {
    const user = userEvent.setup();
    const { onDelete, onDuplicate, onMove, getByRole } = setup(
      [sec("a", 0, 16), sec("b", 16, 16)],
      { selected: 0 },
    );
    const region = getByRole("group", { name: "Arrangement" });
    region.focus();
    await user.keyboard("{Delete}");
    expect(onDelete).toHaveBeenCalledWith(0);
    await user.keyboard("{Meta>}d{/Meta}");
    expect(onDuplicate).toHaveBeenCalledWith(0);
    await user.keyboard("{Alt>}{ArrowRight}{/Alt}");
    expect(onMove).toHaveBeenCalledWith(0, 1);
    onDelete.mockClear();
    screen.getByRole("button", { name: "outside" }).focus();
    await user.keyboard("{Delete}");
    expect(onDelete).not.toHaveBeenCalled();
  });
});
