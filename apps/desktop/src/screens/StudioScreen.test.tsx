import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "../App";
import { FakeClient, goldenExternal, stateEvent } from "../client/fakeClient";
import { createStudioStore } from "../store/studioStore";

function render303(state = stateEvent()) {
  const client = new FakeClient();
  render(<App store={createStudioStore(client)} />);
  act(() => client.emit(state));
  return client;
}

/** The studio with the mixer docked. */
function setup(state = stateEvent()) {
  const client = render303(state);
  fireEvent.click(screen.getByRole("button", { name: "Mixer (M)" }));
  return client;
}

const strip = (track: string) => screen.getByRole("group", { name: `Track ${track}` });
const params = (client: FakeClient) => client.named("set_param").map((c) => c.args);

describe("StudioScreen mixer strips", () => {
  it("a send knob drag sends set_param with its own track and param id", async () => {
    const client = setup();
    const knob = within(strip("hat")).getByRole("slider", { name: "Reverb" });
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 280, pointerId: 1 });
    await waitFor(() => expect(params(client)).toHaveLength(1));
    expect(params(client)[0]).toMatchObject({
      track: "hat",
      param: "send.reverb",
      commit: false,
    });
    fireEvent.pointerUp(knob, { clientY: 280, pointerId: 1 });
    expect(params(client).map((p) => p.commit)).toEqual([false, true]);
    expect(params(client)[1]).toMatchObject({ track: "hat", param: "send.reverb" });
  });

  it("a keyboard step on a duck knob commits on its track", () => {
    const client = setup();
    fireEvent.keyDown(within(strip("acid")).getByRole("slider", { name: "Duck" }), {
      key: "ArrowUp",
    });
    expect(params(client)).toEqual([
      { track: "acid", param: "duck.depth", value: expect.any(Number), commit: true },
    ]);
  });

  it("offers no duck knob on the duck source's own strip", () => {
    setup();
    const duck = (track: string) => within(strip(track)).queryByRole("slider", { name: "Duck" });
    expect(duck("kick")).toBeNull();
    expect(within(strip("kick")).getByText("Duck source")).toBeInTheDocument();
    expect(duck("acid")).toBeInTheDocument();
  });

  it("offers no duck knob anywhere without a duck source", () => {
    setup(stateEvent((s) => (s.song!.mix.duck_source = null)));
    for (const t of ["kick", "acid"]) {
      expect(within(strip(t)).queryByRole("slider", { name: "Duck" })).toBeNull();
      expect(within(strip(t)).getByText("No kick")).toBeInTheDocument();
    }
  });

  it("rings the send knob of the track Claude changed, and no other", () => {
    const client = setup();
    act(() =>
      client.emit({
        ...structuredClone(goldenExternal),
        changes: [{ path: "tracks.kick.send.delay", old: -60, new: -18 }],
      }),
    );
    const ring = (track: string) =>
      within(strip(track)).getByRole("slider", { name: "Delay" }).getAttribute("data-highlight");
    expect(ring("kick")).toBe("claude");
    expect(ring("acid")).toBeNull();
  });

  it("badges a send knob that a lane automates in the sounding section", () => {
    setup(
      stateEvent((s) => {
        s.song!.sections[0]!.lanes.push({
          track: "kick",
          param: "send.delay",
          points: [[0, 0.5]],
        });
      }),
    );
    const badge = (track: string) =>
      within(within(strip(track)).getByRole("slider", { name: "Delay" })).queryByTestId(
        "knob-lane",
      );
    expect(badge("kick")).toBeInTheDocument();
    expect(badge("acid")).toBeNull();
  });

  it("shows the help of a focused send knob in the dock's help line", () => {
    setup();
    act(() => within(strip("kick")).getByRole("slider", { name: "Delay" }).focus());
    const dock = screen.getByRole("region", { name: "Dock" });
    expect(
      within(dock).getByText(
        "How much of this track goes to the shared dotted-eighth delay. -60 is off.",
      ),
    ).toBeInTheDocument();
  });
});

describe("StudioScreen shell", () => {
  it("fills the window without scrolling it", () => {
    render303();
    const shell = screen.getByRole("toolbar", { name: "Transport" }).parentElement!;
    expect(shell).toHaveClass("h-screen", "overflow-hidden", "min-w-[1280px]");
    expect(shell.querySelectorAll('.blob[aria-hidden="true"]')).toHaveLength(3);
  });

  it("stacks the transport, the timeline, the dock and the toast", () => {
    render303();
    fireEvent.click(screen.getByRole("button", { name: "Open acid" }));
    const shell = screen.getByRole("toolbar", { name: "Transport" }).parentElement!;
    const order = [
      screen.getByRole("toolbar", { name: "Transport" }),
      screen.getByRole("region", { name: "Song arrangement" }),
      screen.getByRole("region", { name: "Dock" }),
      screen.getByRole("status", { name: "Song file change" }),
    ];
    expect(order.map((el) => el.parentElement)).toEqual(order.map(() => shell));
    const children = [...shell.children];
    const at = order.map((el) => children.indexOf(el));
    expect(at).toEqual([...at].sort((a, b) => a - b));
    expect(screen.getByRole("region", { name: "Song arrangement" })).toHaveClass(
      "min-h-0",
      "flex-1",
    );
  });

  it("uses blur only on the transport, the timeline and the dock", () => {
    render303();
    fireEvent.click(screen.getByRole("button", { name: "Open acid" }));
    expect(document.querySelectorAll(".glass")).toHaveLength(3);
    expect(screen.getByRole("status", { name: "Song file change" })).toHaveClass("glass-solid");
  });
});
