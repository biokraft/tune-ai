import type { StudioState } from "../../client/protocol";

export type DockTab = "sound" | "steps";

/** What the module dock under the timeline shows: one track, the mixer, or nothing (closed). */
export type DockState = { kind: "track"; track: string; tab: DockTab } | { kind: "mixer" } | null;

export type DockAction =
  | { type: "openTrack"; track: string }
  | { type: "toggleMixer" }
  | { type: "tab"; tab: DockTab }
  | { type: "close" }
  | { type: "prune"; tracks: string[] };

/**
 * Opening the docked track again closes the dock; opening another keeps the tab. Returns the same
 * state object when nothing changes, so React skips the render.
 */
export function dockReducer(s: DockState, a: DockAction): DockState {
  switch (a.type) {
    case "openTrack":
      if (s?.kind === "track" && s.track === a.track) return null;
      return { kind: "track", track: a.track, tab: s?.kind === "track" ? s.tab : "sound" };
    case "toggleMixer":
      return s?.kind === "mixer" ? null : { kind: "mixer" };
    case "tab":
      return s?.kind === "track" && s.tab !== a.tab ? { ...s, tab: a.tab } : s;
    case "close":
      return null;
    case "prune":
      return s?.kind === "track" && !a.tracks.includes(s.track) ? null : s;
  }
}

/** The section the dock edits: `selected` when it still exists, else `current`, else none. */
export function dockSection(
  state: StudioState,
  selected: number | null,
  current: number | null,
): number | null {
  const n = state.song?.sections.length ?? 0;
  const ok = (i: number | null): i is number => i !== null && i >= 0 && i < n;
  return ok(selected) ? selected : ok(current) ? current : null;
}
