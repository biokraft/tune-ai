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
