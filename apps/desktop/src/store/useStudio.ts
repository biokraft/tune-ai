import { createContext, useContext, useRef, useSyncExternalStore } from "react";
import { playheadStep } from "../lib/song";
import type { StudioStore, StudioView } from "./studioStore";

/**
 * Reads one slice of the store. The component re-renders only when `isEqual` says the slice
 * changed, so the 30 Hz playhead reaches only the components that read it.
 */
export function useStudioSelector<T>(
  store: StudioStore,
  select: (v: StudioView) => T,
  isEqual: (a: T, b: T) => boolean = Object.is,
): T {
  const last = useRef<{ value: T } | null>(null);
  const getSnapshot = () => {
    const next = select(store.get());
    if (last.current && isEqual(last.current.value, next)) return last.current.value;
    last.current = { value: next };
    return next;
  };
  return useSyncExternalStore(store.subscribe, getSnapshot);
}

/** Equal views apart from the playhead. */
export function sameButPlayhead(a: StudioView, b: StudioView): boolean {
  return (Object.keys(a) as (keyof StudioView)[]).every((k) => k === "playhead" || a[k] === b[k]);
}

/** The store, for the few leaf components that follow the playhead. */
export const StudioStoreContext = createContext<StudioStore | null>(null);

const noStore: StudioStore = {
  get: () => ({
    state: null,
    playhead: null,
    highlights: new Map(),
    notice: null,
    noticeUndoable: false,
    selectedSection: null,
    connection: "open",
    message: null,
  }),
  subscribe: () => () => {},
  send: () => Promise.resolve(null),
  select: () => {},
  dismissNotice: () => {},
  dismissMessage: () => {},
};

/** The playhead in 16ths from the song start while playing, else null (no store: null). */
export function usePlayheadStep(): number | null {
  const store = useContext(StudioStoreContext) ?? noStore;
  return useStudioSelector(store, playheadStep);
}

/** The playhead's 0-based bar and beat while playing, else null. */
export function usePlayheadPosition(): { bar: number; beat: number } | null {
  const store = useContext(StudioStoreContext) ?? noStore;
  return useStudioSelector(
    store,
    (v) =>
      playheadStep(v) !== null && v.playhead
        ? { bar: v.playhead.bar, beat: v.playhead.beat }
        : null,
    (a, b) => a === b || (a !== null && b !== null && a.bar === b.bar && a.beat === b.beat),
  );
}
