import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "./App";
import { FakeClient, goldenExternal, stateEvent } from "./client/fakeClient";
import type { StateEvent } from "./client/protocol";
import { createStudioStore } from "./store/studioStore";

function setup(state: StateEvent | null = stateEvent()) {
  const client = new FakeClient();
  const store = createStudioStore(client);
  render(<App store={store} />);
  if (state) act(() => client.emit(state));
  return { client, store };
}

const press = (init: KeyboardEventInit, target: EventTarget = document.body) =>
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }),
    );
  });

describe("App", () => {
  it("shows the 303 panel's knobs", () => {
    setup();
    const panel = screen.getByRole("region", { name: "303 acid" });
    for (const name of ["Tuning", "Cutoff", "Resonance", "Env mod", "Decay", "Accent"]) {
      expect(within(panel).getByRole("slider", { name })).toBeInTheDocument();
    }
    for (const name of ["Waveform", "Slide time", "Volume", "Drive", "Mix", "Output"]) {
      expect(within(panel).getByRole("slider", { name })).toBeInTheDocument();
    }
    expect(within(panel).getByRole("radio", { name: "Soft" })).toBeChecked();
  });

  it("keeps the transport and its change notice in view while the page scrolls", () => {
    setup();
    const header = screen.getByRole("button", { name: "Play" }).closest("header");
    expect(header).toHaveClass("sticky", "top-0", "bg-chassis");
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

  it("Cmd+Z undoes, Shift+Cmd+Z redoes", () => {
    const { client } = setup();
    press({ key: "z", metaKey: true });
    press({ key: "z", metaKey: true, shiftKey: true });
    expect(client.calls.map((c) => c.cmd)).toEqual(["undo", "redo"]);
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

  it("ignores shortcuts in text fields, popovers and handled events", () => {
    const { client } = setup();
    const input = document.createElement("input");
    document.body.append(input);
    press({ key: " ", code: "Space" }, input);
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
    expect(client.calls).toEqual([]);
    input.remove();
    pop.remove();
  });

  it("shows the selected section's acid pattern, or says it plays none", () => {
    setup();
    fireEvent.click(screen.getByTestId("section-0"));
    expect(
      screen.getByText("This section plays no 303 pattern. Pick one in the arrangement."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("section-1"));
    const seq = screen.getByRole("group", { name: "303 step sequencer" });
    expect(within(seq).getByRole("button", { name: "Step 1 note" })).toHaveTextContent("C2");
    fireEvent.click(within(seq).getByRole("button", { name: "Step 2 accent" }));
  });

  it("the drum grid shows the selected section's drum patterns", () => {
    const { client } = setup();
    fireEvent.click(screen.getByTestId("section-0"));
    const drums = screen.getByRole("region", { name: "Drums" });
    expect(within(drums).getByRole("group", { name: "kick" })).toBeInTheDocument();
    expect(within(drums).getByRole("group", { name: "hat" })).toBeInTheDocument();
    fireEvent.click(within(drums).getByRole("button", { name: "kick step 2: rest" }));
    expect(client.named("set_drum_step")).toEqual([
      { cmd: "set_drum_step", args: { pattern: "k4", index: 1, hit: "hit" } },
    ]);
  });

  it("highlights a knob Claude changed", () => {
    const { client } = setup();
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
