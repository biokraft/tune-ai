import { stateEvent } from "../../client/fakeClient";
import type { StudioState } from "../../client/protocol";
import { dockReducer, dockSection } from "./dockState";

function stateWithSections(n: number): StudioState {
  return stateEvent((s) => {
    const first = s.song!.sections[0]!;
    s.song!.sections = Array.from({ length: n }, (_, i) => ({
      ...structuredClone(first),
      name: `s${i}`,
      start_bar: i * first.bars,
    }));
  });
}

describe("dockReducer", () => {
  it("opens, switches and toggles a track", () => {
    let s = dockReducer(null, { type: "openTrack", track: "acid" });
    expect(s).toEqual({ kind: "track", track: "acid", tab: "sound" });
    s = dockReducer(s, { type: "tab", tab: "steps" });
    s = dockReducer(s, { type: "openTrack", track: "kick" });
    expect(s).toEqual({ kind: "track", track: "kick", tab: "steps" });
    expect(dockReducer(s, { type: "openTrack", track: "kick" })).toBeNull();
  });

  it("shows a track without ever toggling it closed", () => {
    let s = dockReducer(null, { type: "showTrack", track: "acid" });
    expect(s).toEqual({ kind: "track", track: "acid", tab: "sound" });
    s = dockReducer(s, { type: "tab", tab: "steps" });
    expect(dockReducer(s, { type: "showTrack", track: "acid" })).toBe(s);
    expect(dockReducer(s, { type: "showTrack", track: "kick" })).toEqual({
      kind: "track",
      track: "kick",
      tab: "steps",
    });
    const m = dockReducer(null, { type: "toggleMixer" });
    expect(dockReducer(m, { type: "showTrack", track: "acid" })).toEqual({
      kind: "track",
      track: "acid",
      tab: "sound",
    });
  });

  it("toggles the mixer and closes", () => {
    const m = dockReducer(null, { type: "toggleMixer" });
    expect(m).toEqual({ kind: "mixer" });
    expect(dockReducer(m, { type: "toggleMixer" })).toBeNull();
    expect(dockReducer(m, { type: "close" })).toBeNull();
  });

  it("opens a track from the mixer on the Sound tab", () => {
    const m = dockReducer(null, { type: "toggleMixer" });
    expect(dockReducer(m, { type: "openTrack", track: "acid" })).toEqual({
      kind: "track",
      track: "acid",
      tab: "sound",
    });
    const t = dockReducer(null, { type: "openTrack", track: "acid" });
    expect(dockReducer(t, { type: "toggleMixer" })).toEqual({ kind: "mixer" });
  });

  it("closes when the docked track is gone", () => {
    const s = dockReducer(null, { type: "openTrack", track: "acid" });
    expect(dockReducer(s, { type: "prune", tracks: ["kick"] })).toBeNull();
    expect(dockReducer(s, { type: "prune", tracks: ["acid", "kick"] })).toBe(s);
    const m = dockReducer(null, { type: "toggleMixer" });
    expect(dockReducer(m, { type: "prune", tracks: [] })).toBe(m);
  });

  it("keeps the same state when nothing changes", () => {
    const s = dockReducer(null, { type: "openTrack", track: "acid" });
    expect(dockReducer(s, { type: "tab", tab: "sound" })).toBe(s);
    expect(dockReducer(null, { type: "close" })).toBeNull();
    expect(dockReducer(null, { type: "tab", tab: "steps" })).toBeNull();
  });
});

describe("dockSection", () => {
  it("falls back from a stale selected section", () => {
    const st = stateWithSections(3);
    expect(dockSection(st, 1, 0)).toBe(1);
    expect(dockSection(st, 5, 0)).toBe(0);
    expect(dockSection(st, 5, 9)).toBeNull();
    expect(dockSection(st, null, null)).toBeNull();
  });
});
