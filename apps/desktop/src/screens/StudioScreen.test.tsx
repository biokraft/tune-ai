import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "../App";
import { FakeClient, goldenExternal, stateEvent } from "../client/fakeClient";
import { createStudioStore } from "../store/studioStore";

function setup() {
  const client = new FakeClient();
  render(<App store={createStudioStore(client)} />);
  act(() => client.emit(stateEvent()));
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
    const client = new FakeClient();
    render(<App store={createStudioStore(client)} />);
    act(() =>
      client.emit(
        stateEvent((s) => {
          s.song!.sections[0]!.lanes.push({
            track: "kick",
            param: "send.delay",
            points: [[0, 0.5]],
          });
        }),
      ),
    );
    const badge = (track: string) =>
      within(within(strip(track)).getByRole("slider", { name: "Delay" })).queryByTestId(
        "knob-lane",
      );
    expect(badge("kick")).toBeInTheDocument();
    expect(badge("acid")).toBeNull();
  });

  it("shows the help of a focused send knob in the mixer's help line", () => {
    setup();
    act(() => within(strip("kick")).getByRole("slider", { name: "Delay" }).focus());
    const mixer = screen.getByRole("complementary", { name: "Mixer" });
    expect(
      within(mixer).getByText(
        "How much of this track goes to the shared dotted-eighth delay. -60 is off.",
      ),
    ).toBeInTheDocument();
  });
});
