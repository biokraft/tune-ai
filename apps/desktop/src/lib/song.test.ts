import { stateEvent } from "../client/fakeClient";
import type { StudioState } from "../client/protocol";
import type { StudioView } from "../store/studioStore";
import { acidSection, currentSection } from "./song";

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

describe("acidSection", () => {
  // The golden song: section 0 (`a`) has no acid, section 1 (`b`) plays a1.

  it("stopped with nothing selected, falls back to the first section the 303 plays in", () => {
    const v = view();
    expect(currentSection(v)).toBe(0);
    expect(acidSection(v)).toBe(1);
  });

  it("keeps the cursor's section when the 303 plays there", () => {
    expect(acidSection(view({}, (s) => (s.cursor_bar = 3)))).toBe(1);
  });

  it("while playing follows the playhead, even through a section without the 303", () => {
    const playing = view({ playhead: { step: 4, bar: 0, beat: 1 } }, (s) => (s.playing = true));
    expect(acidSection(playing)).toBe(0);
  });

  it("a selected section always wins", () => {
    expect(acidSection(view({ selectedSection: 0 }))).toBe(0);
  });

  it("keeps the current section when the 303 plays nowhere", () => {
    const v = view({}, (s) => {
      for (const sec of s.song!.sections) delete sec.play.acid;
    });
    expect(acidSection(v)).toBe(0);
  });
});
