import type { Command, ExternalChange, StudioEvent, StudioState } from "../client/protocol";
import { LOST_CONNECTION, type StudioClient } from "../client/StudioClient";
import { changedSteps } from "./changedSteps";

/** How long a change from outside stays highlighted. */
export const HIGHLIGHT_MS = 4000;

export type HighlightKind = "claude" | "external";

export type StudioView = {
  state: StudioState | null;
  playhead: { step: number; bar: number; beat: number } | null;
  /**
   * Edit paths changed from outside in the last 4 s. `patterns.<p>.steps` is expanded to one
   * key per changed step: `patterns.<p>.steps[<i>]`.
   */
  highlights: Map<string, HighlightKind>;
  /** The last change on disk, until dismissed. */
  notice: ExternalChange | null;
  selectedSection: number | null;
  connection: "open" | "lost";
  /** The last failure (a rejected call or an `error` event), until dismissed. */
  message: string | null;
};

export type StudioStore = {
  get(): StudioView;
  subscribe(l: () => void): () => void;
  send(cmd: Command): Promise<unknown>;
  select(i: number | null): void;
  dismissNotice(): void;
  dismissMessage(): void;
};

const STEPS = /^patterns\.(.+)\.steps$/;

/** The highlight keys for one change. */
function highlightKeys(path: string, old: unknown, next: unknown): string[] {
  if (STEPS.test(path) && typeof old === "string" && typeof next === "string") {
    return changedSteps(old, next).map((i) => `${path}[${i}]`);
  }
  return [path];
}

/** The studio as the UI sees it, fed by one client's events; read via useSyncExternalStore. */
export function createStudioStore(client: StudioClient): StudioStore {
  let view: StudioView = {
    state: null,
    playhead: null,
    highlights: new Map(),
    notice: null,
    selectedSection: null,
    connection: "open",
    message: null,
  };
  const listeners = new Set<() => void>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  const set = (patch: Partial<StudioView>) => {
    view = { ...view, ...patch };
    for (const l of [...listeners]) l();
  };

  const clampSelection = (state: StudioState, sel: number | null): number | null => {
    const n = state.song?.sections.length ?? 0;
    if (sel === null || n === 0) return null;
    return Math.min(sel, n - 1);
  };

  const highlight = (keys: string[], kind: HighlightKind) => {
    if (keys.length === 0) return;
    const next = new Map(view.highlights);
    for (const key of keys) {
      next.set(key, kind);
      clearTimeout(timers.get(key));
      timers.set(
        key,
        setTimeout(() => {
          timers.delete(key);
          const after = new Map(view.highlights);
          after.delete(key);
          set({ highlights: after });
        }, HIGHLIGHT_MS),
      );
    }
    set({ highlights: next });
  };

  const onEvent = (e: StudioEvent) => {
    switch (e.event) {
      case "state": {
        const { event: _, ...state } = e;
        set({ state, selectedSection: clampSelection(state, view.selectedSection) });
        break;
      }
      case "playhead":
        set({ playhead: { step: e.step, bar: e.bar, beat: e.beat } });
        break;
      case "applied":
        // A client's own edit is already in the next state; only outside sources highlight.
        if (e.source !== "human") {
          highlight(
            e.changes.flatMap((c) => highlightKeys(c.path, c.old, c.new)),
            e.source,
          );
        }
        break;
      case "external_change": {
        const { event: _, ...change } = e;
        // An unreadable file shows in the file error banner (from `state`), not as a notice.
        if (change.errors.length > 0) break;
        const kind: HighlightKind = change.source === "claude" ? "claude" : "external";
        highlight(
          change.changes.flatMap((c) => highlightKeys(c.path, c.old, c.new)),
          kind,
        );
        const worth = change.changes.length > 0 || change.offered.length > 0 || change.replaced;
        if (worth) set({ notice: change });
        break;
      }
      case "error":
        set({ message: e.message });
        break;
    }
  };

  client.subscribe(onEvent);
  client.onLost?.(() => set({ connection: "lost" }));

  return {
    get: () => view,
    subscribe(l) {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    send(cmd) {
      const p = client.call(cmd);
      p.catch((err: unknown) => {
        const message = err instanceof Error ? err.message : String(err);
        // The lost-connection banner already says this.
        if (message !== LOST_CONNECTION) set({ message });
      });
      return p;
    },
    select(i) {
      set({ selectedSection: view.state ? clampSelection(view.state, i) : i });
    },
    dismissNotice() {
      if (view.notice) set({ notice: null });
    },
    dismissMessage() {
      if (view.message) set({ message: null });
    },
  };
}
