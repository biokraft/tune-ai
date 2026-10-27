import { FakeClient, goldenExternal, migratedStateEvent, stateEvent } from "../client/fakeClient";
import type { ExternalChangeEvent } from "../client/protocol";
import { createStudioStore } from "./studioStore";

function external(patch: Partial<ExternalChangeEvent>): ExternalChangeEvent {
  return { ...structuredClone(goldenExternal), ...patch };
}

describe("createStudioStore", () => {
  afterEach(() => vi.useRealTimers());

  it("state replaces snapshot", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    expect(store.get().state).toBeNull();
    let notified = 0;
    store.subscribe(() => (notified += 1));

    const first = stateEvent();
    client.emit(first);
    expect(store.get().state?.song?.name).toBe("mini");

    const second = stateEvent((s) => {
      s.playing = true;
      s.song!.name = "other";
    });
    client.emit(second);
    expect(store.get().state?.playing).toBe(true);
    expect(store.get().state?.song?.name).toBe("other");
    expect(store.get().state).not.toHaveProperty("event");
    expect(notified).toBe(2);
  });

  it("asks for the state at start, for a transport that dropped the first state event", async () => {
    const client = new FakeClient();
    // The host emitted the first state before the webview listened (Tauri does not buffer).
    client.emit(stateEvent());
    const { event: _, ...state } = stateEvent();
    client.results.get_state = state;
    const store = createStudioStore(client);
    expect(client.named("get_state")).toHaveLength(1);
    await vi.waitFor(() => expect(store.get().state?.song?.name).toBe("mini"));
    expect(store.get().state).toEqual(state);
  });

  it("a state event that arrives before the get_state result wins", async () => {
    const client = new FakeClient();
    let answer!: (v: unknown) => void;
    client.results.get_state = new Promise((resolve) => (answer = resolve));
    const store = createStudioStore(client);
    client.emit(stateEvent((s) => (s.cursor_bar = 3)));
    const { event: _, ...older } = stateEvent();
    answer(older);
    await Promise.resolve();
    await Promise.resolve();
    expect(store.get().state?.cursor_bar).toBe(3);
  });

  it("external change highlights for 4 s", () => {
    vi.useFakeTimers();
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    client.emit(goldenExternal);
    expect(store.get().notice?.changes[0]?.path).toBe("sections[1].bars");
    expect(store.get().highlights.get("sections[1].bars")).toBe("claude");
    vi.advanceTimersByTime(3900);
    expect(store.get().highlights.has("sections[1].bars")).toBe(true);
    vi.advanceTimersByTime(200);
    expect(store.get().highlights.has("sections[1].bars")).toBe(false);
  });

  it("a newer change to the same path restarts its 4 s", () => {
    vi.useFakeTimers();
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(goldenExternal);
    vi.advanceTimersByTime(3000);
    client.emit(external({ source: "external" }));
    vi.advanceTimersByTime(3000);
    expect(store.get().highlights.get("sections[1].bars")).toBe("external");
    vi.advanceTimersByTime(1100);
    expect(store.get().highlights.size).toBe(0);
  });

  it("expands `clips.<c>.steps` into per-step keys", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(
      external({
        changes: [{ path: "clips.k4.steps", old: "x... x...", new: "x... X..." }],
      }),
    );
    const keys = [...store.get().highlights.keys()];
    expect(keys).toEqual(["clips.k4.steps[4]"]);
  });

  it("an applied edit from a human sets nothing", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    const before = store.get();
    client.emit({
      event: "applied",
      source: "human",
      changes: [{ path: "tracks.acid.acid303.cutoff", old: 500, new: 820 }],
      run: null,
    });
    expect(store.get()).toBe(before);
  });

  it("an applied change from Claude raises the notice with its run", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    const changes = [{ path: "tracks.acid.acid303.cutoff", old: 500, new: 820 }];
    client.emit({
      event: "applied",
      source: "claude",
      changes,
      run: { id: "r1", title: "open the filter" },
    });
    expect(store.get().notice).toEqual({
      source: "claude",
      changes,
      run: { id: "r1", title: "open the filter" },
      replaced: false,
      merged: [],
      offered: [],
      errors: [],
    });
    expect(store.get().notice?.run?.id).toBe("r1");
    expect(store.get().noticeUndoable).toBe(true);
    expect(store.get().highlights.get("tracks.acid.acid303.cutoff")).toBe("claude");
  });

  it("an external change has no run", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    client.emit(external({ merged: [] }));
    expect(store.get().notice?.run).toBeNull();
  });

  it("a human edit after it ends its Undo but keeps Undo this run", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    client.emit({
      event: "applied",
      source: "claude",
      changes: [{ path: "tracks.acid.acid303.cutoff", old: 500, new: 820 }],
      run: { id: "r1", title: null },
    });
    client.emit({
      event: "applied",
      source: "human",
      changes: [{ path: "tracks.acid.acid303.resonance", old: 70, new: 90 }],
      run: null,
    });
    expect(store.get().noticeUndoable).toBe(false);
    expect(store.get().notice?.run?.id).toBe("r1");
  });

  it("undoRun sends undo_run and keeps the answer for the toast, until the notice goes", async () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    client.emit({
      event: "applied",
      source: "claude",
      changes: [{ path: "tracks.acid.acid303.cutoff", old: 500, new: 820 }],
      run: { id: "r1", title: null },
    });
    const answer = { run: "r1", title: null, reverted: [], kept: [], already: [] };
    client.results.undo_run = answer;
    await store.undoRun("r1");
    expect(client.named("undo_run")).toEqual([{ cmd: "undo_run", args: { run: "r1" } }]);
    expect(store.get().runUndo).toEqual(answer);
    store.dismissNotice();
    expect(store.get().runUndo).toBeNull();
  });

  it("a later human edit keeps the notice but takes back its Undo", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    client.emit(external({ merged: [] }));
    expect(store.get().noticeUndoable).toBe(true);
    client.emit({
      event: "applied",
      source: "human",
      changes: [{ path: "tracks.acid.acid303.cutoff", old: 500, new: 820 }],
      run: null,
    });
    // Undo now reverts the producer's edit, not the notice's change.
    expect(store.get().notice).not.toBeNull();
    expect(store.get().noticeUndoable).toBe(false);
    client.emit(external({ merged: [] }));
    expect(store.get().noticeUndoable).toBe(true);
  });

  it("a change that merged pending edits is not undoable from the notice", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    // The merged edits were written on top as their own undo step.
    client.emit(goldenExternal);
    expect(goldenExternal.merged.length).toBeGreaterThan(0);
    expect(store.get().notice).not.toBeNull();
    expect(store.get().noticeUndoable).toBe(false);
  });

  it("an unreadable file sets no notice", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(
      external({
        changes: [],
        merged: [],
        offered: [],
        errors: [{ line: 3, column: 1, message: "expected a value" }],
      }),
    );
    expect(store.get().notice).toBeNull();
  });

  it("shows the migration notice once, with undo", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    const opened = (path: string, from: number | null) =>
      (from === null ? stateEvent : migratedStateEvent)((s) => {
        s.path = path;
      });
    client.emit(opened("songs/a/song.toml", 1));
    expect(store.get().notice).toEqual({
      source: "external",
      run: null,
      changes: [{ path: "song.version", old: 1, new: 2 }],
      replaced: false,
      merged: [],
      offered: [],
      errors: [],
    });
    expect(store.get().noticeUndoable).toBe(true);
    // The same song's next states do not show it again, even once dismissed.
    store.dismissNotice();
    client.emit(opened("songs/a/song.toml", 1));
    expect(store.get().notice).toBeNull();
    // Shown again, the notice loses its Undo once one undo no longer reverts the migration.
    client.emit(opened("songs/b/song.toml", 1));
    expect(store.get().notice?.changes[0]?.path).toBe("song.version");
    client.emit(opened("songs/b/song.toml", null));
    expect(store.get().notice).not.toBeNull();
    expect(store.get().noticeUndoable).toBe(false);
    // A song opened without a migration shows none.
    store.dismissNotice();
    client.emit(opened("songs/c/song.toml", null));
    expect(store.get().notice).toBeNull();
  });

  it("a migration turning null leaves another notice's Undo alone", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(migratedStateEvent());
    client.emit(external({ merged: [] }));
    expect(store.get().notice?.source).toBe("claude");
    client.emit(stateEvent((s) => (s.migrated_from = null)));
    expect(store.get().noticeUndoable).toBe(true);
  });

  it("shows an error event, such as a migration that could not be saved", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    const message =
      "the song was upgraded to the new file format in memory but not saved: cannot write song.toml";
    client.emit({ event: "error", message });
    expect(store.get().message).toBe(message);
  });

  it("playhead events set the playhead", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit({ event: "playhead", step: 37, bar: 2, beat: 1, playing: true });
    expect(store.get().playhead).toEqual({ step: 37, bar: 2, beat: 1 });
  });

  it("a playhead event that changes nothing notifies no one", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    const tick = { event: "playhead", step: 37, bar: 2, beat: 1, playing: true } as const;
    client.emit(tick);
    const before = store.get();
    let notified = 0;
    store.subscribe(() => (notified += 1));
    client.emit({ ...tick });
    client.emit({ ...tick });
    expect(notified).toBe(0);
    expect(store.get()).toBe(before);
    client.emit({ ...tick, step: 38 });
    expect(notified).toBe(1);
  });

  it("select clamps to the song's sections", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    client.emit(stateEvent());
    store.select(1);
    expect(store.get().selectedSection).toBe(1);
    client.emit(stateEvent((s) => s.song!.sections.pop()));
    expect(store.get().selectedSection).toBe(0);
    store.select(null);
    expect(store.get().selectedSection).toBeNull();
  });

  it("send forwards to the client and keeps a failure's message", async () => {
    const client = new FakeClient();
    client.results.undo = new Error("nothing to undo");
    const store = createStudioStore(client);
    await expect(store.send({ cmd: "undo" })).rejects.toThrow("nothing to undo");
    expect(client.sent()).toEqual([{ cmd: "undo" }]);
    expect(store.get().message).toBe("nothing to undo");
  });

  it("a lost connection shows as lost", () => {
    const client = new FakeClient();
    const store = createStudioStore(client);
    expect(store.get().connection).toBe("open");
    client.loseConnection();
    expect(store.get().connection).toBe("lost");
  });
});
