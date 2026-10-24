import { act, fireEvent, render, screen, within } from "@testing-library/react";
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
    onOpenTrack: vi.fn(),
    onMix: vi.fn(),
  };
  const utils = render(
    <div>
      <button>outside</button>
      <Arrangement
        sections={sections}
        tracks={[
          { id: "acid", instrument: "acid303", mute: false, solo: false },
          { id: "drums", instrument: "drums", mute: false, solo: false },
        ]}
        patterns={[
          { id: "a1", track: "acid", kind: "acid", glyph: [0.6, 0, 1, 0] },
          { id: "a2", track: "acid", kind: "acid", glyph: [1, 1] },
          { id: "d1", track: "drums", kind: "drum", glyph: [0.6, 0, 0, 0, 1, 0, 0, 0] },
        ]}
        playhead={0}
        selected={null}
        loop={null}
        openTrack={null}
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

  it("labels each row with the track id and instrument", () => {
    setup([sec("a", 0, 16)]);
    const heads = screen.getAllByRole("rowheader");
    expect(heads).toHaveLength(2);
    expect(heads[0]).toHaveTextContent("acid");
    expect(heads[0]).toHaveTextContent("Acid 303");
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

  it("names the frame with its section and track counts", () => {
    setup([sec("a", 0, 16), sec("b", 16, 16)]);
    expect(screen.getByText("Arrangement")).toBeInTheDocument();
    expect(screen.getByText("2 sections · 2 tracks")).toBeInTheDocument();
  });

  it("a track header opens its track", () => {
    const { onOpenTrack } = setup([sec("a", 0, 16)]);
    fireEvent.click(screen.getByRole("button", { name: "Open acid" }));
    expect(onOpenTrack).toHaveBeenCalledWith("acid");
  });

  it("the open track's header is pressed", () => {
    setup([sec("a", 0, 16)], { openTrack: "drums" });
    expect(screen.getByRole("button", { name: "Open drums" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Open acid" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("mute and solo in a header mix that track", () => {
    const { onMix, onOpenTrack } = setup([sec("a", 0, 16)]);
    const head = screen.getAllByRole("rowheader")[0]!;
    fireEvent.click(within(head).getByRole("button", { name: "M" }));
    expect(onMix).toHaveBeenLastCalledWith("acid", { mute: true });
    fireEvent.click(within(head).getByRole("button", { name: "S" }));
    expect(onMix).toHaveBeenLastCalledWith("acid", { solo: true });
    expect(onOpenTrack).not.toHaveBeenCalled();
  });

  it("the header shows the track's mute and solo state", () => {
    setup([sec("a", 0, 16)], {
      tracks: [{ id: "acid", instrument: "acid303", mute: true, solo: false }],
    });
    const head = screen.getByRole("rowheader");
    expect(within(head).getByRole("button", { name: "M" })).toHaveAttribute("aria-pressed", "true");
    expect(within(head).getByRole("button", { name: "S" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("the loop is a band over the rows with a bracket in the ruler", () => {
    setup([sec("a", 0, 16), sec("b", 16, 16)], { loop: [16, 32] });
    const bracket = screen.getByTestId("loop-range");
    expect(bracket).toHaveAttribute("x", String(16 * 12));
    expect(bracket).toHaveAttribute("width", String(16 * 12));
    const band = screen.getByTestId("loop-band");
    expect(band).toHaveAttribute("x", String(16 * 12));
    expect(band).toHaveAttribute("width", String(16 * 12));
  });

  it("the playhead glows and spans the ruler and the rows", () => {
    setup([sec("a", 0, 16)], { playhead: 2 });
    const ph = screen.getByTestId("playhead");
    expect(ph).toHaveAttribute("stroke", "rgb(var(--play))");
    expect(ph).toHaveAttribute("y1", "0");
    // Ruler 24 + sections 32 + two 46 px rows.
    expect(ph).toHaveAttribute("y2", String(24 + 32 + 2 * 46));
    expect(ph.closest("svg")!.getAttribute("style")).toContain(
      "drop-shadow(0 0 6px rgb(var(--play) / .8))",
    );
  });

  it("labels the ruler every 8 bars at 12 px per bar", () => {
    setup([sec("a", 0, 32)]);
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText("9")).toBeInTheDocument();
    expect(screen.getByText("17")).toBeInTheDocument();
    expect(screen.queryByText("5")).toBeNull();
  });

  it("Delete on a header or block button leaves the selected section alone", async () => {
    const user = userEvent.setup();
    const { onDelete } = setup([sec("a", 0, 16, { play: { acid: "a1" } })], { selected: 0 });
    const head = screen.getAllByRole("rowheader")[0]!;
    const buttons = [
      screen.getByRole("button", { name: "Open acid" }),
      within(head).getByRole("button", { name: "M" }),
      within(head).getByRole("button", { name: "S" }),
      screen.getByRole("button", { name: "a1 in a" }),
    ];
    for (const b of buttons) {
      b.focus();
      await user.keyboard("{Delete}");
      await user.keyboard("{Backspace}");
    }
    expect(onDelete).not.toHaveBeenCalled();
    screen.getByRole("group", { name: "Arrangement" }).focus();
    await user.keyboard("{Backspace}");
    expect(onDelete).toHaveBeenCalledWith(0);
  });
});

const lane = (
  track: string,
  param: string,
  points: [number, number][] = [
    [0, 0],
    [8, 1],
  ],
) => ({
  track,
  param,
  points,
});

describe("Arrangement blocks", () => {
  const blk = (name: string) => screen.getByRole("button", { name });

  it("a block opens its track on its section", () => {
    const { onSelect, onOpenTrack } = setup(
      [sec("a", 0, 16, { play: { kick: "k4" } }), sec("b", 16, 16, { play: { kick: "k4" } })],
      {
        tracks: [{ id: "kick", instrument: "kick", mute: false, solo: false }],
        patterns: [{ id: "k4", track: "kick", kind: "drum", glyph: [1, 0, 0, 0] }],
      },
    );
    const b = blk("k4 in b");
    expect(b.tagName).toBe("BUTTON");
    fireEvent.click(b);
    expect(onSelect).toHaveBeenCalledWith(1);
    expect(onOpenTrack).toHaveBeenCalledWith("kick");
  });

  it("draws a block in its track's colour, inset 3 px, fuller when its section is selected", () => {
    setup([sec("a", 0, 16, { play: { acid: "a1" } }), sec("b", 16, 16, { play: { acid: "a2" } })], {
      selected: 1,
    });
    const a = blk("a1 in a");
    expect(a.style.left).toBe("3px");
    expect(a.style.top).toBe(`${56 + 3}px`);
    expect(a.style.width).toBe(`${16 * 12 - 6}px`);
    expect(a.style.height).toBe(`${46 - 6}px`);
    expect(a).toHaveTextContent("a1");
    const fill = within(a).getByTestId("block-fill");
    expect(fill).toHaveAttribute("fill", "rgb(var(--acid) / .22)");
    expect(fill).toHaveAttribute("stroke", "rgb(var(--acid) / .45)");
    expect(fill).toHaveAttribute("rx", "9");
    expect(within(blk("a2 in b")).getByTestId("block-fill")).toHaveAttribute(
      "fill",
      "rgb(var(--acid) / .32)",
    );
    expect(blk("a2 in b").style.left).toBe(`${16 * 12 + 3}px`);
  });

  it("draws a bar per drum hit and a dash per acid note", () => {
    setup([sec("a", 0, 16, { play: { acid: "a1", drums: "d1" } })]);
    // Block height 40: an accent is 24 px tall, a hit 14.4 px.
    const bars = within(blk("d1 in a")).getAllByTestId("glyph-step");
    expect(bars.map((b) => Number(b.getAttribute("height")))).toEqual([
      expect.closeTo(14.4, 6),
      expect.closeTo(24, 6),
    ]);
    const dashes = within(blk("a1 in a")).getAllByTestId("glyph-step");
    expect(dashes).toHaveLength(2);
    for (const d of dashes) expect(d).toHaveAttribute("height", "2");
    // The accented note sits higher.
    expect(Number(dashes[1]!.getAttribute("y"))).toBeLessThan(Number(dashes[0]!.getAttribute("y")));
  });

  it("badges a pattern that plays in more than one section", () => {
    setup([
      sec("a", 0, 16, { play: { acid: "a1", drums: "d1" } }),
      sec("b", 16, 16, { play: { acid: "a1" } }),
    ]);
    expect(within(blk("a1 in a")).getByText("×2")).toHaveClass("badge");
    expect(within(blk("a1 in b")).getByText("×2")).toBeInTheDocument();
    expect(within(blk("d1 in a")).queryByText(/×/)).toBeNull();
  });

  it("has no pattern popover: the dock picks patterns", async () => {
    const user = userEvent.setup();
    setup([sec("a", 0, 16, { play: { acid: "a1" } })]);
    expect(screen.queryByRole("button", { name: "acid in section 1" })).toBeNull();
    await user.click(blk("a1 in a"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("button", { name: "a2" })).toBeNull();
    expect(screen.queryByRole("button", { name: "None" })).toBeNull();
  });

  it("an empty cell is a hidden hover outline that opens its track on its section", () => {
    const { onSelect, onOpenTrack } = setup([sec("a", 0, 16), sec("b", 16, 16)]);
    const cell = screen.getByTestId("empty-1-drums");
    expect(cell).toHaveAttribute("aria-hidden", "true");
    expect(cell).toHaveClass("border-dashed", "hover:border-line");
    fireEvent.click(cell);
    expect(onSelect).toHaveBeenCalledWith(1);
    expect(onOpenTrack).toHaveBeenCalledWith("drums");
  });

  it("rings Claude's play and pattern changes in cyan", () => {
    setup(
      [
        sec("a", 0, 16, { play: { acid: "a1", drums: "d1" } }),
        sec("b", 16, 16, { play: { acid: "a1", drums: "d1" } }),
      ],
      { highlighted: new Set(["sections[0].play.drums", "patterns.a1.steps[2]"]) },
    );
    const ring = (name: string) => within(blk(name)).queryByTestId("block-highlight");
    const r = ring("d1 in a")!;
    expect(r.tagName.toLowerCase()).toBe("rect");
    expect(r).toHaveAttribute("data-highlight", "claude");
    expect(r).toHaveAttribute("stroke", "rgb(var(--agent))");
    expect(r).toHaveAttribute("stroke-width", "1.5");
    expect(ring("d1 in b")).toBeNull();
    expect(ring("a1 in a")).toBeInTheDocument();
    expect(ring("a1 in b")).toBeInTheDocument();
  });

  it("gives each automated param a 22 px row under its track", () => {
    const { container } = setup(
      [
        sec("a", 0, 16, { lanes: [lane("acid", "acid303.cutoff")] }),
        sec("b", 16, 16, {
          play: { drums: "d1" },
          lanes: [lane("acid", "acid303.cutoff"), lane("acid", "send.delay")],
        }),
      ],
      { laneLabels: { "acid.acid303.cutoff": "Cutoff" } },
    );
    expect(screen.getByText("↳ Cutoff")).toHaveClass("lbl");
    // A lane without a label falls back to its param id.
    expect(screen.getByText("↳ send.delay")).toBeInTheDocument();
    // Two lane rows push the drums row down.
    expect(blk("d1 in b").style.top).toBe(`${56 + 46 + 2 * 22 + 3}px`);
    expect(screen.getByTestId("playhead")).toHaveAttribute("y2", String(56 + 2 * 46 + 2 * 22));
    // Each curve sits inside its own lane row.
    const polys = [...container.querySelectorAll('[data-testid="lane-overlay"] polyline')];
    expect(polys).toHaveLength(3);
    const ys = (el: Element) =>
      el
        .getAttribute("points")!
        .split(" ")
        .map((pt) => parseFloat(pt.split(",")[1]!));
    const cutoffTop = 56 + 46;
    for (const poly of polys.filter((el) => el.getAttribute("data-lane") === "acid303.cutoff")) {
      for (const y of ys(poly)) {
        expect(y).toBeGreaterThanOrEqual(cutoffTop);
        expect(y).toBeLessThanOrEqual(cutoffTop + 22);
      }
    }
    const delay = polys.find((el) => el.getAttribute("data-lane") === "send.delay")!;
    for (const y of ys(delay)) expect(y).toBeGreaterThanOrEqual(cutoffTop + 22);
  });

  it("puts mix lanes under a final Mix header with no mute or solo", () => {
    setup([sec("a", 0, 16, { lanes: [lane("mix", "reverb.size")] })], {
      laneLabels: { "mix.reverb.size": "Reverb size" },
    });
    const mix = screen.getByTestId("mix-header");
    expect(mix).toHaveTextContent("Mix");
    expect(within(mix).queryAllByRole("button")).toHaveLength(0);
    expect(screen.getByText("↳ Reverb size")).toBeInTheDocument();
    expect(screen.getAllByRole("rowheader")).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "M" })).toHaveLength(2);
  });

  it("draws blocks and drops sections at the fitted bar width", () => {
    let observed: (() => void) | null = null;
    class RO {
      constructor(cb: () => void) {
        observed = cb;
      }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", RO);
    let width = 0;
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get: () => width,
    });
    try {
      const { onMove } = setup([
        sec("a", 0, 25, { play: { acid: "a1" } }),
        sec("b", 25, 25, { play: { acid: "a1" } }),
      ]);
      expect(blk("a1 in b").style.left).toBe(`${25 * 12 + 3}px`);
      // 1178 - 10 right pad - 168 headers = 1000 px over 50 bars: 20 px per bar.
      width = 1178;
      act(() => observed!());
      expect(blk("a1 in b").style.left).toBe(`${25 * 20 + 3}px`);
      expect(blk("a1 in b").style.width).toBe(`${25 * 20 - 6}px`);
      const drag = (dx: number) => {
        fireEvent.pointerDown(block(0), { clientX: 0, pointerId: 1 });
        fireEvent.pointerMove(block(0), { clientX: dx, pointerId: 1 });
        fireEvent.pointerUp(block(0), { clientX: dx, pointerId: 1 });
      };
      // a's own midpoint is at 12.5 bars: 250 px at 20 px per bar (150 px at 12).
      drag(200);
      expect(onMove).not.toHaveBeenCalled();
      drag(300);
      expect(onMove).toHaveBeenCalledWith(0, 1);
    } finally {
      delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
      vi.unstubAllGlobals();
    }
  });
});
