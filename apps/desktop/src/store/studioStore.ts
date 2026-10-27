import type {
  Command,
  ExternalChange,
  RunTag,
  RunUndo,
  StudioEvent,
  StudioState,
} from "../client/protocol";
import { LOST_CONNECTION, type StudioClient } from "../client/StudioClient";
import { changedSteps } from "./changedSteps";

/** How long a change from outside stays highlighted. */
export const HIGHLIGHT_MS = 4000;

/** The toast's change: from the song file (`run` null) or an agent's applied edit with its run. */
export type Notice = ExternalChange & { run: RunTag | null };

export type HighlightKind = "claude" | "external";

export type StudioView = {
  state: StudioState | null;
  playhead: { step: number; bar: number; beat: number } | null;
  /**
   * Edit paths changed from outside in the last 4 s. `clips.<c>.steps` is expanded to one key
   * per changed step: `clips.<c>.steps[<i>]`.
   */
  highlights: Map<string, HighlightKind>;
  /** The last change on disk, until dismissed. */
  notice: Notice | null;
  /** What undoing the notice's run did, until the notice goes. */
  runUndo: RunUndo | null;
  /**
   * Whether one undo still reverts the notice's change: not when it merged pending edits, and no
   * longer once a local edit, undo, redo or re-apply (an `applied` event from a human) lands on
   * top of it.
   */
  noticeUndoable: boolean;
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
  /** Undo every change of an agent run; the answer shows in the toast. */
  undoRun(run: string): Promise<unknown>;
  dismissNotice(): void;
  dismissMessage(): void;
};

const STEPS = /^clips\.(.+)\.steps$/;

/** The highlight keys for one change. */
function highlightKeys(path: string, old: unknown, next: unknown): string[] {
  if (STEPS.test(path) && typeof old === "string" && typeof next === "string") {
    return changedSteps(old, next).map((i) => `${path}[${i}]`);
  }
  return [path];
}

/** The format version the studio writes. */
const FORMAT_VERSION = 2;

/** The notice for a song file upgraded from format `from` on open, as one change on disk. */
function migrationChange(from: number): Notice {
  return {
    source: "external",
    run: null,
    changes: [{ path: "song.version", old: from, new: FORMAT_VERSION }],
    replaced: false,
    merged: [],
    offered: [],
    errors: [],
  };
}

/** The studio as the UI sees it, fed by one client's events; read via useSyncExternalStore. */
export function createStudioStore(client: StudioClient): StudioStore {
  let view: StudioView = {
    state: null,
    playhead: null,
    highlights: new Map(),
    notice: null,
    runUndo: null,
    noticeUndoable: false,
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

  /** The song path the last state named: a different one is a newly opened song. */
  let openPath: string | null = null;
  /** The notice that says the open song was migrated, while it stands. */
  let migrationNotice: Notice | null = null;

  const adopt = (state: StudioState) => {
    const patch: Partial<StudioView> = {
      state,
      selectedSection: clampSelection(state, view.selectedSection),
    };
    const opened = state.path !== openPath;
    openPath = state.path;
    if (opened && state.migrated_from !== null) {
      // Opening upgraded the file: say so once, with an Undo while one undo reverts it.
      migrationNotice = migrationChange(state.migrated_from);
      patch.notice = migrationNotice;
      patch.runUndo = null;
      patch.noticeUndoable = true;
    } else if (
      state.migrated_from === null &&
      migrationNotice !== null &&
      view.notice === migrationNotice
    ) {
      patch.noticeUndoable = false;
    }
    set(patch);
  };

  /** State events seen; a `get_state` result older than one of them is dropped. */
  let stateEvents = 0;

  const onEvent = (e: StudioEvent) => {
    switch (e.event) {
      case "state": {
        const { event: _, ...state } = e;
        stateEvents += 1;
        adopt(state);
        break;
      }
      case "playhead": {
        // 30 events a second, but the step moves only every ~100 ms: skip repeats outright.
        const p = view.playhead;
        if (p && p.step === e.step && p.bar === e.bar && p.beat === e.beat) break;
        set({ playhead: { step: e.step, bar: e.bar, beat: e.beat } });
        break;
      }
      case "applied":
        // A client's own edit is already in the next state; only outside sources highlight.
        if (e.source !== "human") {
          highlight(
            e.changes.flatMap((c) => highlightKeys(c.path, c.old, c.new)),
            e.source,
          );
          if (e.source === "claude") {
            // The same toast an agent's edit on disk raises, plus its run for "Undo this run".
            set({
              notice: {
                source: "claude",
                changes: e.changes,
                run: e.run,
                replaced: false,
                merged: [],
                offered: [],
                errors: [],
              },
              runUndo: null,
              noticeUndoable: true,
            });
          }
        } else if (view.noticeUndoable) {
          // The undo stack now has this edit on top, so Undo would revert it instead.
          set({ noticeUndoable: false });
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
        // Merged pending edits were written on top as their own undo step, so one undo would
        // revert those, not this change.
        if (worth) {
          set({
            notice: { ...change, run: null },
            runUndo: null,
            noticeUndoable: change.merged.length === 0,
          });
        }
        break;
      }
      case "error":
        set({ message: e.message });
        break;
    }
  };

  const report = (err: unknown) => {
    const message = err instanceof Error ? err.message : String(err);
    // The lost-connection banner already says this.
    if (message !== LOST_CONNECTION) set({ message });
  };

  client.subscribe(onEvent);
  client.onLost?.(() => set({ connection: "lost" }));
  // A transport may drop events sent before it listened (Tauri does), so ask for the state
  // too. A state event that arrives first is as new or newer, and wins.
  const seenBefore = stateEvents;
  client.call<StudioState | null>({ cmd: "get_state" }).then((state) => {
    if (state && stateEvents === seenBefore) adopt(state);
  }, report);

  /** Runs whose undo is in flight. */
  const undoing = new Set<string>();

  const send = (cmd: Command) => {
    const p = client.call(cmd);
    p.catch(report);
    return p;
  };

  return {
    get: () => view,
    subscribe(l) {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    send,
    select(i) {
      set({ selectedSection: view.state ? clampSelection(view.state, i) : i });
    },
    async undoRun(run) {
      // A second click before the first answer would only report "nothing to undo" over it.
      if (undoing.has(run)) return null;
      undoing.add(run);
      let result: unknown;
      try {
        result = await send({ cmd: "undo_run", args: { run } });
      } finally {
        undoing.delete(run);
      }
      // Only a toast still about this run takes the answer.
      if (view.notice?.run?.id === run) set({ runUndo: result as RunUndo, noticeUndoable: false });
      return result;
    },
    dismissNotice() {
      if (view.notice) set({ notice: null, runUndo: null, noticeUndoable: false });
    },
    dismissMessage() {
      if (view.message) set({ message: null });
    },
  };
}
