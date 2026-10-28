import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "./App";
import { FakeClient, goldenExternal, migratedStateEvent, stateEvent } from "./client/fakeClient";
import type { StateEvent } from "./client/protocol";
import { createStudioStore } from "./store/studioStore";

function setup(state: StateEvent | null = stateEvent()) {
  const client = new FakeClient();
  const store = createStudioStore(client);
  render(<App store={store} />);
  if (state) act(() => client.emit(state));
  return { client, store };
}

const openDock = (track: string) => {
  fireEvent.click(screen.getByRole("button", { name: `Open ${track}` }));
  return screen.getByRole("region", { name: "Dock" });
};

const press = (init: KeyboardEventInit, target: EventTarget = document.body) =>
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }),
    );
  });

describe("App", () => {
  it("shows the 303's knobs in the dock", () => {
    setup();
    const panel = openDock("acid");
    for (const name of ["Tuning", "Cutoff", "Resonance", "Env mod", "Decay", "Accent"]) {
      expect(within(panel).getByRole("slider", { name })).toBeInTheDocument();
    }
    for (const name of ["Waveform", "Slide time", "Volume", "Drive", "Mix", "Output"]) {
      expect(within(panel).getByRole("slider", { name })).toBeInTheDocument();
    }
    expect(within(panel).getByRole("radio", { name: "Soft" })).toBeChecked();
  });

  it("keeps the transport and the change notice in view: the window never scrolls", () => {
    setup();
    const shell = screen.getByRole("region", { name: "Song arrangement" }).parentElement;
    expect(shell).toHaveClass("h-screen", "overflow-hidden");
    const toast = screen.getByRole("status", { name: "Song file change" });
    expect(toast).toHaveClass("absolute", "top-full");
    expect(toast.parentElement).toContainElement(
      screen.getByRole("toolbar", { name: "Transport" }),
    );
  });

  it("Space calls play, and stop while playing", () => {
    const { client } = setup();
    press({ key: " ", code: "Space" });
    expect(client.named("play")).toEqual([{ cmd: "play", args: { from_bar: null } }]);
    act(() => client.emit(stateEvent((s) => (s.playing = true))));
    press({ key: " ", code: "Space" });
    expect(client.named("stop")).toHaveLength(1);
  });

  it("Space on a focused button plays instead of pressing the button", () => {
    const { client } = setup();
    const undo = screen.getByRole("button", { name: "Undo" });
    undo.focus();
    const e = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      undo.dispatchEvent(e);
    });
    expect(e.defaultPrevented).toBe(true);
    expect(client.named("play")).toHaveLength(1);
    expect(client.named("undo")).toHaveLength(0);
  });

  it("dragging Cutoff sends live values, then one commit on release", async () => {
    const { client } = setup();
    openDock("acid");
    const knob = screen.getByRole("slider", { name: "Cutoff" });
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 290, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 280, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 270, pointerId: 1 });
    const params = () => client.named("set_param").map((c) => c.args);
    // Three moves in one frame send one live value: the last.
    await waitFor(() => expect(params()).toHaveLength(1));
    expect(params()[0]).toMatchObject({ track: "acid", param: "acid303.cutoff", commit: false });
    fireEvent.pointerUp(knob, { clientY: 270, pointerId: 1 });
    expect(params().map((p) => p.commit)).toEqual([false, true]);
    expect(params()[1]!.value).toBe(params()[0]!.value);
    // The knob keeps showing the released value until the studio's state catches up.
    expect(knob).not.toHaveAttribute("aria-valuenow", "500");
  });

  it("Re-apply after a replaced file resolves the offered edits", () => {
    const { client } = setup(stateEvent((s) => (s.offered = [])));
    act(() => client.emit({ ...structuredClone(goldenExternal), replaced: true }));
    expect(screen.getByText("The song file was replaced. Re-apply 1 unsaved edit?")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Re-apply" }));
    expect(client.named("resolve_offered")).toEqual([
      { cmd: "resolve_offered", args: { reapply: true } },
    ]);
  });

  it("a null path shows the song list", async () => {
    const client = new FakeClient();
    client.results.list_songs = ["songs/first-acid/song.toml", "songs/mini/song.toml"];
    render(<App store={createStudioStore(client)} />);
    act(() =>
      client.emit(
        stateEvent((s) => {
          s.path = null;
          s.song = null;
        }),
      ),
    );
    const song = await screen.findByRole("button", { name: /first-acid/ });
    expect(screen.getByRole("button", { name: /mini/ })).toBeInTheDocument();
    fireEvent.click(song);
    expect(client.named("open_song")).toEqual([
      { cmd: "open_song", args: { path: "songs/first-acid/song.toml" } },
    ]);
  });

  it("the open-song screen shows the TuneAI lockup", async () => {
    const client = new FakeClient();
    client.results.list_songs = ["songs/mini/song.toml"];
    render(<App store={createStudioStore(client)} />);
    act(() => client.emit(stateEvent((s) => (s.path = null))));
    await screen.findByRole("button", { name: /mini/ });
    const main = screen.getByRole("main");
    expect(main).toHaveTextContent("TuneAI");
    expect(screen.getByText("AI")).toHaveClass("text-agent");
    expect(main.querySelector('svg[viewBox="0 0 120 120"]')).not.toBeNull();
  });

  it("an empty song list says how to start one", async () => {
    const client = new FakeClient();
    client.results.list_songs = [];
    render(<App store={createStudioStore(client)} />);
    act(() => client.emit(stateEvent((s) => (s.path = null))));
    expect(
      await screen.findByText("No songs yet. Ask Claude to draft one in songs/<name>/song.toml."),
    ).toBeInTheDocument();
  });

  it("shows file errors with their positions", () => {
    setup(
      stateEvent(
        (s) =>
          (s.file_errors = [
            { line: 12, column: 5, message: "expected a value" },
            { line: 30, column: 1, message: "unknown pattern `a9`" },
          ]),
      ),
    );
    const banner = screen.getByRole("alert");
    expect(banner).toHaveTextContent(
      "song.toml has 2 errors. Still playing the last good version.",
    );
    expect(within(banner).getByText("12:5 expected a value")).toBeInTheDocument();
    expect(within(banner).getByText("30:1 unknown pattern `a9`")).toBeInTheDocument();
    // A long list scrolls inside the banner rather than pushing the dock off-screen.
    expect(within(banner).getByRole("list")).toHaveClass("max-h-[30vh]", "overflow-auto");
  });

  it("shows file errors instead of the song list when the open song does not parse", () => {
    const client = new FakeClient();
    client.results.list_songs = ["songs/mini/song.toml"];
    render(<App store={createStudioStore(client)} />);
    act(() =>
      client.emit(
        stateEvent((s) => {
          s.song = null;
          s.file_errors = [
            { line: 12, column: 5, message: "expected a value" },
            { line: null, column: null, message: "no sections" },
          ];
        }),
      ),
    );
    const banner = screen.getByRole("alert");
    expect(banner).toHaveTextContent("song.toml has 2 errors, so it cannot be opened.");
    expect(within(banner).getByText("12:5 expected a value")).toBeInTheDocument();
    expect(within(banner).getByText("no sections")).toBeInTheDocument();
    expect(screen.queryByText("Open a song")).toBeNull();
    expect(client.named("list_songs")).toHaveLength(0);
  });

  it("tells the producer to restart tune serve when the connection is lost", () => {
    const { client } = setup(stateEvent((s) => (s.file_errors = [])));
    act(() => client.loseConnection());
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Lost connection to the studio. Restart tune serve, then reload this page.",
    );
  });

  it("says a song was upgraded on open, with an Undo that reverts it", () => {
    const { client } = setup(
      migratedStateEvent((s) => {
        s.offered = [];
        s.file_errors = [];
      }),
    );
    const notice = screen.getByRole("status", { name: "Song file change" });
    expect(notice).toHaveTextContent(
      "The song file changed 1 thing: Upgraded the song file to format version 2 (comments kept)",
    );
    fireEvent.click(within(notice).getByRole("button", { name: "Undo the change on disk" }));
    expect(client.named("undo")).toHaveLength(1);
    expect(screen.queryByRole("status", { name: "Song file change" })).toBeNull();
  });

  it("shows an error from the studio, such as a migration it could not save", () => {
    const { client } = setup(stateEvent());
    const message =
      "the song was upgraded to the new file format in memory but not saved: cannot write song.toml";
    act(() => client.emit({ event: "error", message }));
    const banner = screen.getByText(message).closest('[role="status"]')!;
    fireEvent.click(within(banner as HTMLElement).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText(message)).toBeNull();
  });

  it("Cmd+Z undoes, Shift+Cmd+Z redoes", () => {
    const { client } = setup();
    press({ key: "z", metaKey: true });
    press({ key: "z", metaKey: true, shiftKey: true });
    expect(client.sent().map((c) => c.cmd)).toEqual(["undo", "redo"]);
  });

  it("undo from the transport or keyboard clears Claude's notice", () => {
    const { client } = setup();
    const claude = () =>
      act(() =>
        client.emit({
          ...structuredClone(goldenExternal),
          offered: [],
          merged: [],
          changes: [{ path: "tracks.acid.acid303.resonance", old: 70, new: 90 }],
        }),
      );
    const notice = () => screen.queryByText(/^Claude changed/);
    claude();
    expect(notice()).toBeInTheDocument();
    press({ key: "z", metaKey: true });
    expect(notice()).toBeNull();
    claude();
    // Two buttons undo, each with its own name: the transport's ↶ and the notice's.
    expect(screen.getByRole("button", { name: "Undo Claude's change" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(notice()).toBeNull();
    expect(client.named("undo")).toHaveLength(2);
  });

  it("L loops the selected section, and again clears the loop", () => {
    const { client } = setup(stateEvent((s) => (s.loop_bars = null)));
    press({ key: "l" });
    expect(client.named("set_loop")).toHaveLength(0);
    fireEvent.click(screen.getByTestId("section-1"));
    press({ key: "l" });
    expect(client.named("set_loop")).toEqual([{ cmd: "set_loop", args: { bars: [2, 4] } }]);
    act(() => client.emit(stateEvent((s) => (s.loop_bars = [2, 4]))));
    press({ key: "l" });
    expect(client.named("set_loop")[1]).toEqual({ cmd: "set_loop", args: { bars: null } });
  });

  it("a click on the ruler seeks there", () => {
    const { client } = setup();
    const ruler = screen.getByTestId("ruler");
    fireEvent.pointerDown(ruler, { clientX: 3 * 12 + 2, pointerId: 1 });
    fireEvent.pointerUp(ruler, { clientX: 3 * 12 + 2, pointerId: 1 });
    expect(client.named("seek_bar")).toEqual([{ cmd: "seek_bar", args: { bar: 3 } }]);
    expect(client.named("set_loop")).toHaveLength(0);
  });

  it("a drag on the ruler loops that range and jumps to its start, without playing", () => {
    const { client } = setup(stateEvent((s) => (s.loop_bars = null)));
    const ruler = screen.getByTestId("ruler");
    fireEvent.pointerDown(ruler, { clientX: 1 * 12 + 2, pointerId: 1 });
    fireEvent.pointerMove(ruler, { clientX: 2 * 12 + 6, pointerId: 1 });
    fireEvent.pointerUp(ruler, { clientX: 2 * 12 + 6, pointerId: 1 });
    expect(client.sent()).toEqual([
      { cmd: "set_loop", args: { bars: [1, 3] } },
      { cmd: "seek_bar", args: { bar: 1 } },
    ]);
  });

  it("ignores shortcuts in text fields, popovers and handled events", () => {
    const { client } = setup();
    const input = document.createElement("input");
    document.body.append(input);
    press({ key: " ", code: "Space" }, input);
    press({ key: "m" }, input);
    expect(screen.queryByRole("region", { name: "Dock" })).toBeNull();
    const pop = document.createElement("div");
    pop.setAttribute("data-radix-popper-content-wrapper", "");
    const inner = document.createElement("button");
    pop.append(inner);
    document.body.append(pop);
    press({ key: "z", metaKey: true }, inner);
    const handled = new KeyboardEvent("keydown", { key: "l", bubbles: true, cancelable: true });
    handled.preventDefault();
    act(() => {
      document.body.dispatchEvent(handled);
    });
    expect(client.sent()).toEqual([]);
    input.remove();
    pop.remove();
  });

  it("docks the 303 on the section at the cursor, and Steps offers a pattern pick there", () => {
    // Stopped at bar 1, nothing selected: section `a` has no 303.
    setup();
    const dock = openDock("acid");
    fireEvent.click(within(dock).getByRole("tab", { name: "Steps" }));
    expect(within(dock).getByText("a · bars 1–2")).toBeInTheDocument();
    expect(
      within(dock).getByText("acid plays nothing in a. Pick a pattern above."),
    ).toBeInTheDocument();
    expect(within(dock).getByRole("button", { name: "acid in section 1" })).toHaveTextContent("—");
  });

  it("while playing, the 303 sequencer follows the playhead into a section without the 303", () => {
    const { client } = setup(stateEvent((s) => (s.playing = true)));
    const dock = openDock("acid");
    fireEvent.click(within(dock).getByRole("tab", { name: "Steps" }));
    act(() => client.emit({ event: "playhead", step: 36, bar: 2, beat: 1, playing: true }));
    expect(within(dock).getByRole("group", { name: "303 step sequencer" })).toBeInTheDocument();
    act(() => client.emit({ event: "playhead", step: 4, bar: 0, beat: 1, playing: true }));
    expect(
      within(dock).getByText("acid plays nothing in a. Pick a pattern above."),
    ).toBeInTheDocument();
  });

  it("shows the selected section's acid pattern, or says it plays none", () => {
    setup();
    const dock = openDock("acid");
    fireEvent.click(within(dock).getByRole("tab", { name: "Steps" }));
    fireEvent.click(screen.getByTestId("section-0"));
    expect(
      within(dock).getByText("acid plays nothing in a. Pick a pattern above."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("section-1"));
    const seq = within(dock).getByRole("group", { name: "303 step sequencer" });
    expect(within(seq).getByRole("button", { name: "Step 1 note" })).toHaveTextContent("C2");
    fireEvent.click(within(seq).getByRole("button", { name: "Step 2 accent" }));
  });

  it("the drum grid shows the selected section's drum patterns", () => {
    const { client } = setup();
    fireEvent.click(screen.getByTestId("section-0"));
    const drums = openDock("kick");
    expect(within(drums).getByRole("group", { name: "kick" })).toBeInTheDocument();
    expect(within(drums).getByRole("group", { name: "hat" })).toBeInTheDocument();
    fireEvent.click(within(drums).getByRole("button", { name: "kick step 2: rest" }));
    expect(client.named("set_drum_step")).toEqual([
      { cmd: "set_drum_step", args: { pattern: "k4", index: 1, hit: "hit" } },
    ]);
  });

  it("highlights a knob Claude changed", () => {
    const { client } = setup();
    openDock("acid");
    act(() =>
      client.emit({
        ...structuredClone(goldenExternal),
        changes: [{ path: "tracks.acid.acid303.resonance", old: 70, new: 90 }],
      }),
    );
    expect(screen.getByRole("slider", { name: "Resonance" })).toHaveAttribute(
      "data-highlight",
      "claude",
    );
  });
});
