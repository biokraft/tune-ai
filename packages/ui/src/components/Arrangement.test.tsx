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
    onLoop: vi.fn(),
    onMove: vi.fn(),
    onResize: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
    onOpenTrack: vi.fn(),
    onShowTrack: vi.fn(),
    onMix: vi.fn(),
  };
  const ui = (more: Record<string, unknown>) => (
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
        {...more}
      />
    </div>
  );
  const utils = render(ui({}));
  /** Re-renders with `more` on top of the first props, as a new playhead tick would. */
  const update = (more: Record<string, unknown>) => utils.rerender(ui(more));
  return { ...fns, ...utils, update };
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

  it("fills the sticky parts only while something scrolls under them", () => {
    setup([sec("a", 0, 16)]);
    const column = screen.getByRole("rowgroup", { name: "Tracks" });
    const scroller = column.parentElement!.parentElement!;
    const filled = (el: Element) => el.className.includes("--solid");
    const band = column.nextElementSibling!.firstElementChild!;
    expect(filled(column)).toBe(false);
    expect(filled(band)).toBe(false);

    scroller.scrollTop = 40;
    fireEvent.scroll(scroller);
    expect(filled(band)).toBe(true);
    expect(filled(column)).toBe(false);

    scroller.scrollTop = 0;
    scroller.scrollLeft = 30;
    fireEvent.scroll(scroller);
    expect(filled(band)).toBe(false);
    expect(filled(column)).toBe(true);
  });

  it("scrolls the lanes without rubber-banding or chaining the scroll to the window", () => {
    setup([sec("a", 0, 16)]);
    const scroller = screen.getByRole("rowgroup", { name: "Tracks" }).parentElement!.parentElement!;
    expect(scroller).toHaveClass("overflow-auto", "overscroll-none");
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
    const ph = screen.getByTestId("playhead");
    expect(ph).toHaveAttribute("data-x", "54");
    // A 2 px line centred on x: its left edge sits 1 px before it.
    expect(ph.style.transform).toBe("translateX(53px)");
  });

  it("ruler ticks every 8 bars, one-based", () => {
    setup([sec("a", 0, 16)]);
    expect(screen.getByText("9")).toBeInTheDocument();
  });

  it("highlighted paths ring the block", () => {
    setup([sec("a", 0, 16), sec("b", 16, 16)], {
      highlighted: new Map([["sections[1].bars", "claude"]]),
    });
    expect(block(0)).not.toHaveAttribute("data-highlight");
    expect(block(1)).toHaveAttribute("data-highlight", "claude");
    const ring = block(1).parentElement!.querySelector('rect[data-highlight="claude"]')!;
    expect(ring).toHaveAttribute("stroke", "rgb(var(--agent))");
  });

  it("marks a section changed on disk with a dashed ink2 outline, not cyan", () => {
    setup([sec("a", 0, 16), sec("b", 16, 16)], {
      highlighted: new Map([["sections[1].bars", "external"]]),
    });
    expect(block(1)).toHaveAttribute("data-highlight", "external");
    const g = block(1).parentElement!;
    expect(g.innerHTML).not.toMatch(/--agent/);
    const mark = g.querySelector('rect[data-highlight="external"]')!;
    expect(mark).toHaveAttribute("stroke", "rgb(var(--ink2))");
    expect(mark).toHaveAttribute("stroke-dasharray");
  });

  it("a Claude change wins over a change on disk in the same section", () => {
    setup([sec("a", 0, 16)], {
      highlighted: new Map([
        ["sections[0].bars", "external"],
        ["sections[0].name", "claude"],
      ]),
    });
    expect(block(0)).toHaveAttribute("data-highlight", "claude");
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
    expect(ph.tagName).toBe("DIV");
    expect(ph).toHaveAttribute("aria-hidden", "true");
    expect(ph).toHaveClass("pointer-events-none", "absolute", "top-0", "left-0", "z-[25]");
    expect(ph.style.width).toBe("2px");
    expect(ph.style.background).toBe("rgb(var(--play))");
    expect(ph.style.boxShadow).toBe("0 0 6px rgb(var(--play) / .8)");
    // Ruler 24 + sections 32 + two 46 px rows.
    expect(ph.style.height).toBe(`${24 + 32 + 2 * 46}px`);
  });

  it("draws the playhead without a CSS filter, which WebKit leaves as fog while it moves", () => {
    setup([sec("a", 0, 16)], { playhead: 2 });
    const ph = screen.getByTestId("playhead");
    expect(ph.style.filter).toBe("");
    expect(ph.getAttribute("style")).not.toMatch(/filter/);
    expect(ph.style.willChange).toBe("transform");
    for (let el: HTMLElement | null = ph; el; el = el.parentElement) {
      expect(el.style.filter).toBe("");
    }
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
    const { onSelect, onOpenTrack, onShowTrack } = setup(
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
    expect(onShowTrack).toHaveBeenCalledWith("kick");
    expect(onOpenTrack).not.toHaveBeenCalled();
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
    const { onSelect, onOpenTrack, onShowTrack } = setup([sec("a", 0, 16), sec("b", 16, 16)]);
    const cell = screen.getByTestId("empty-1-drums");
    expect(cell).toHaveAttribute("aria-hidden", "true");
    expect(cell).toHaveClass("border-dashed", "hover:border-line");
    fireEvent.click(cell);
    expect(onSelect).toHaveBeenCalledWith(1);
    expect(onShowTrack).toHaveBeenCalledWith("drums");
    expect(onOpenTrack).not.toHaveBeenCalled();
  });

  it("rings Claude's play and pattern changes in cyan", () => {
    setup(
      [
        sec("a", 0, 16, { play: { acid: "a1", drums: "d1" } }),
        sec("b", 16, 16, { play: { acid: "a1", drums: "d1" } }),
      ],
      {
        highlighted: new Map([
          ["sections[0].play.drums", "claude"],
          ["patterns.a1.steps[2]", "claude"],
        ]),
      },
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

  it("outlines a block changed on disk dashed in ink2, not cyan", () => {
    setup([sec("a", 0, 16, { play: { acid: "a1", drums: "d1" } })], {
      highlighted: new Map([
        ["sections[0].play.drums", "external"],
        ["patterns.a1.steps[2]", "external"],
      ]),
    });
    for (const name of ["d1 in a", "a1 in a"]) {
      const r = within(blk(name)).getByTestId("block-highlight");
      expect(r).toHaveAttribute("data-highlight", "external");
      expect(r).toHaveAttribute("stroke", "rgb(var(--ink2))");
      expect(r).toHaveAttribute("stroke-dasharray");
      expect(blk(name).innerHTML).not.toMatch(/--agent/);
    }
  });

  it("outlines an empty cell changed on disk dashed in ink2", () => {
    setup([sec("a", 0, 16)], { highlighted: new Map([["sections[0].play.drums", "external"]]) });
    const r = within(screen.getByTestId("empty-0-drums")).getByTestId("block-highlight");
    expect(r).toHaveAttribute("data-highlight", "external");
    expect(r).toHaveAttribute("stroke", "rgb(var(--ink2))");
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
    expect(screen.getByTestId("playhead").style.height).toBe(`${56 + 2 * 46 + 2 * 22}px`);
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

describe("Arrangement ruler", () => {
  // 12 px per bar in jsdom (nothing measures the scroller), over two 16-bar sections.
  const two = () => [sec("a", 0, 16), sec("b", 16, 16)];
  const ruler = () => screen.getByTestId("ruler");
  const at = (bar: number, into = 3) => ({ clientX: bar * 12 + into, pointerId: 1 });
  const cap = () => screen.getByRole("slider", { name: "Playhead" });

  it("a click on the ruler seeks to the bar under the pointer", () => {
    const { onSeek, onLoop } = setup(two());
    expect(ruler()).toHaveClass("cursor-pointer");
    fireEvent.pointerDown(ruler(), at(5));
    fireEvent.pointerUp(ruler(), at(5));
    expect(onSeek).toHaveBeenCalledTimes(1);
    expect(onSeek).toHaveBeenCalledWith(5);
    expect(onLoop).not.toHaveBeenCalled();
  });

  it("a press that moves less than the slop is still a click", () => {
    const { onSeek, onLoop } = setup(two());
    fireEvent.pointerDown(ruler(), at(5, 1));
    fireEvent.pointerMove(ruler(), at(5, 3));
    fireEvent.pointerUp(ruler(), at(5, 3));
    expect(onSeek).toHaveBeenCalledWith(5);
    expect(onLoop).not.toHaveBeenCalled();
  });

  it("only the left button acts", () => {
    const { onSeek, onLoop } = setup(two());
    fireEvent.pointerDown(ruler(), { ...at(5), button: 2 });
    fireEvent.pointerUp(ruler(), { ...at(5), button: 2 });
    expect(onSeek).not.toHaveBeenCalled();
    expect(onLoop).not.toHaveBeenCalled();
  });

  it("measures the pointer from the ruler's own left edge, so a scrolled timeline still hits", () => {
    const { onSeek } = setup(two());
    // Scrolled 120 px: the ruler's left edge sits 120 px left of the viewport's.
    vi.spyOn(ruler(), "getBoundingClientRect").mockReturnValue({
      left: -120,
      top: 0,
      right: 0,
      bottom: 24,
      width: 384,
      height: 24,
      x: -120,
      y: 0,
      toJSON: () => ({}),
    });
    fireEvent.pointerDown(ruler(), { clientX: 3, pointerId: 1 });
    fireEvent.pointerUp(ruler(), { clientX: 3, pointerId: 1 });
    expect(onSeek).toHaveBeenCalledWith(10);
  });

  it("clamps a press past the last bar to the last bar", () => {
    const { onSeek } = setup(two());
    fireEvent.pointerDown(ruler(), at(40));
    fireEvent.pointerUp(ruler(), at(40));
    expect(onSeek).toHaveBeenCalledWith(31);
  });

  it("a drag across bars loops them, end exclusive, and does not seek", () => {
    const { onSeek, onLoop } = setup(two());
    fireEvent.pointerDown(ruler(), at(2));
    fireEvent.pointerMove(ruler(), at(4));
    fireEvent.pointerMove(ruler(), at(5));
    fireEvent.pointerUp(ruler(), at(5));
    expect(onLoop).toHaveBeenCalledTimes(1);
    expect(onLoop).toHaveBeenCalledWith([2, 6]);
    expect(onSeek).not.toHaveBeenCalled();
  });

  it("a right-to-left drag gives the same range", () => {
    const { onSeek, onLoop } = setup(two());
    fireEvent.pointerDown(ruler(), at(5));
    fireEvent.pointerMove(ruler(), at(2));
    fireEvent.pointerUp(ruler(), at(2));
    expect(onLoop).toHaveBeenCalledWith([2, 6]);
    expect(onSeek).not.toHaveBeenCalled();
  });

  it("a drag past the end loops up to the song's last bar", () => {
    const { onLoop } = setup(two());
    fireEvent.pointerDown(ruler(), at(28));
    fireEvent.pointerMove(ruler(), at(50));
    fireEvent.pointerUp(ruler(), at(50));
    expect(onLoop).toHaveBeenCalledWith([28, 32]);
  });

  it("previews the range as a band while dragging, and drops it on release", () => {
    setup(two());
    expect(screen.queryByTestId("loop-preview")).toBeNull();
    fireEvent.pointerDown(ruler(), at(2));
    fireEvent.pointerMove(ruler(), at(5));
    const band = screen.getByTestId("loop-preview");
    expect(band).toHaveAttribute("x", String(2 * 12));
    expect(band).toHaveAttribute("width", String(4 * 12));
    fireEvent.pointerUp(ruler(), at(5));
    expect(screen.queryByTestId("loop-preview")).toBeNull();
  });

  it("a cancelled drag neither loops nor seeks", () => {
    const { onSeek, onLoop } = setup(two());
    fireEvent.pointerDown(ruler(), at(2));
    fireEvent.pointerMove(ruler(), at(5));
    fireEvent.pointerCancel(ruler(), at(5));
    expect(onLoop).not.toHaveBeenCalled();
    expect(onSeek).not.toHaveBeenCalled();
    expect(screen.queryByTestId("loop-preview")).toBeNull();
  });

  it("the playhead's cap is a slider over the song's bars", () => {
    setup(two(), { playhead: 4.5 });
    const c = cap();
    expect(c).toHaveAttribute("aria-valuemin", "1");
    expect(c).toHaveAttribute("aria-valuemax", "32");
    expect(c).toHaveAttribute("aria-valuenow", "5");
    expect(c).toHaveAttribute("tabindex", "0");
    expect(c).toHaveClass("cursor-ew-resize", "pointer-events-auto");
  });

  it("dragging the cap moves the playhead bar by bar, ignores ticks, and seeks on release", () => {
    const { onSeek, onLoop, update } = setup(two(), { playhead: 4.5 });
    fireEvent.pointerDown(cap(), { clientX: 54, pointerId: 1 });
    fireEvent.pointerMove(cap(), { clientX: 100, pointerId: 1 });
    // 100 px is 8.3 bars: the nearest bar line is 8.
    expect(screen.getByTestId("playhead")).toHaveAttribute("data-x", String(8 * 12));
    expect(cap()).toHaveAttribute("aria-valuenow", "9");
    update({ playhead: 5 });
    expect(screen.getByTestId("playhead")).toHaveAttribute("data-x", String(8 * 12));
    fireEvent.pointerUp(cap(), { clientX: 100, pointerId: 1 });
    expect(onSeek).toHaveBeenCalledTimes(1);
    expect(onSeek).toHaveBeenCalledWith(8);
    expect(onLoop).not.toHaveBeenCalled();
    // Released, it follows the ticks again.
    expect(screen.getByTestId("playhead")).toHaveAttribute("data-x", String(5 * 12));
  });

  it("a press on the cap that does not move leaves the playhead where it is", () => {
    const { onSeek, onLoop } = setup(two(), { playhead: 4.5 });
    fireEvent.pointerDown(cap(), { clientX: 54, pointerId: 1 });
    fireEvent.pointerUp(cap(), { clientX: 54, pointerId: 1 });
    expect(onSeek).not.toHaveBeenCalled();
    expect(onLoop).not.toHaveBeenCalled();
  });

  it("arrow keys on the cap step the playhead a bar, clamped, and leave the sections alone", async () => {
    const user = userEvent.setup();
    const { onSeek, onMove, update } = setup(two(), { playhead: 4.5, selected: 0 });
    act(() => cap().focus());
    await user.keyboard("{ArrowRight}");
    expect(onSeek).toHaveBeenLastCalledWith(5);
    await user.keyboard("{ArrowLeft}");
    expect(onSeek).toHaveBeenLastCalledWith(3);
    await user.keyboard("{Alt>}{ArrowRight}{/Alt}");
    expect(onMove).not.toHaveBeenCalled();
    update({ playhead: 0 });
    await user.keyboard("{ArrowLeft}");
    expect(onSeek).toHaveBeenLastCalledWith(0);
    update({ playhead: 31.5 });
    await user.keyboard("{ArrowRight}");
    expect(onSeek).toHaveBeenLastCalledWith(31);
  });
});
