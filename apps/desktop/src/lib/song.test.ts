import { stateEvent } from "../client/fakeClient";
import type { StudioState } from "../client/protocol";
import type { StudioView } from "../store/studioStore";
import { currentSection, highlightedSteps, loopSectionName, patternGlyph } from "./song";

function view(patch: Partial<StudioView> = {}, edit: (s: StudioState) => void = () => {}) {
  const { event: _, ...state } = stateEvent();
  edit(state);
  return {
    state,
    playhead: null,
    highlights: new Map(),
    notice: null,
    runUndo: null,
    noticeUndoable: false,
    selectedSection: null,
    connection: "open",
    message: null,
    ...patch,
  } satisfies StudioView;
}

describe("currentSection", () => {
  // The golden song: section 0 (`a`) starts at bar 0, section 1 (`b`) follows it.

  it("stopped with nothing selected, is the section under the cursor", () => {
    expect(currentSection(view())).toBe(0);
    expect(currentSection(view({}, (s) => (s.cursor_bar = 3)))).toBe(1);
  });

  it("while playing follows the playhead", () => {
    const playing = view({ playhead: { step: 4, bar: 0, beat: 1 } }, (s) => {
      s.playing = true;
      s.cursor_bar = 3;
    });
    expect(currentSection(playing)).toBe(0);
  });

  it("a selected section always wins", () => {
    expect(currentSection(view({ selectedSection: 1 }))).toBe(1);
  });
});

describe("loopSectionName", () => {
  const sections = [
    { name: "intro", start_bar: 0, bars: 16, play: {}, lanes: [] },
    { name: "drop", start_bar: 80, bars: 16, play: {}, lanes: [] },
  ];
  it("names the section whose bars equal the loop", () => {
    expect(loopSectionName(sections, [80, 96])).toBe("drop");
  });
  it("is null for a partial or missing loop", () => {
    expect(loopSectionName(sections, [80, 88])).toBeNull();
    expect(loopSectionName(sections, [0, 32])).toBeNull();
    expect(loopSectionName(sections, null)).toBeNull();
  });
});

describe("patternGlyph", () => {
  it("draws a drum pattern's rests, hits and accents", () => {
    const hits = [..."x...X..."].map((c) => (c === "x" ? "hit" : c === "X" ? "accent" : "rest"));
    expect(
      patternGlyph({
        kind: "drum",
        id: "k",
        hits: hits as ("rest" | "hit" | "accent")[],
      }),
    ).toEqual([0.6, 0, 0, 0, 1, 0, 0, 0]);
  });

  it("draws an acid pattern's rests, notes and accented notes", () => {
    // C2 . C2! .
    const c2 = { note: 36, accent: false, slide: false };
    const rest = { note: null, accent: false, slide: false };
    expect(
      patternGlyph({
        kind: "acid",
        id: "a",
        steps: [c2, rest, { ...c2, accent: true }, rest],
      }),
    ).toEqual([0.6, 0, 1, 0]);
  });

  it("a note clip's glyph marks its note starts", () => {
    const glyph = patternGlyph({
      kind: "notes",
      id: "lead",
      bars: 1,
      notes: [
        { start: 0, pitch: 48, len: 3, vel: 100 },
        { start: 2.5, pitch: 51, len: 2, vel: 120 },
        { start: 15.9, pitch: 48, len: 0.1, vel: 100 },
      ],
    });
    expect(glyph).toHaveLength(16);
    expect(glyph.map((v, i) => [i, v]).filter(([, v]) => v !== 0)).toEqual([
      [0, 0.6],
      [2, 1],
      [15, 0.6],
    ]);
  });

  it("keeps the larger value where two notes start in one step", () => {
    const glyph = patternGlyph({
      kind: "notes",
      id: "lead",
      bars: 2,
      notes: [
        { start: 4, pitch: 48, len: 1, vel: 127 },
        { start: 4.5, pitch: 55, len: 1, vel: 100 },
      ],
    });
    expect(glyph).toHaveLength(32);
    expect(glyph[4]).toBe(1);
  });
});

describe("highlightedSteps", () => {
  it("keeps who changed each step of the pattern", () => {
    const h = new Map([
      ["clips.a1.steps[2]", "claude" as const],
      ["clips.a1.steps[5]", "external" as const],
      ["clips.a10.steps[1]", "claude" as const],
      ["clips.a1.length", "external" as const],
    ]);
    expect(highlightedSteps(h, "a1")).toEqual(
      new Map([
        [2, "claude"],
        [5, "external"],
      ]),
    );
  });
});
