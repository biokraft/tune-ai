import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { goldenState, stateEvent } from "../client/fakeClient";
import type { Command, SongView } from "../client/protocol";
import { Panel303 } from "./Panel303";

function setup(song: SongView = goldenState.song!, result: unknown = null, automatedIn = 1) {
  const sent: Command[] = [];
  const send = vi.fn((c: Command) => {
    sent.push(c);
    return Promise.resolve(result);
  });
  const utils = render(
    <Panel303
      song={song}
      section={1}
      automatedIn={automatedIn}
      highlights={new Map()}
      send={send}
    />,
  );
  return { sent, ...utils };
}

describe("Panel303", () => {
  it("shows the help of the hovered knob", () => {
    setup();
    expect(screen.getByText("Hover or focus a knob to learn what it does.")).toBeInTheDocument();
    fireEvent.pointerEnter(screen.getByRole("slider", { name: "Resonance" }));
    expect(screen.getByText(/A whistling peak at the cutoff/)).toBeInTheDocument();
    fireEvent.pointerLeave(screen.getByRole("slider", { name: "Resonance" }));
    expect(screen.getByText("Hover or focus a knob to learn what it does.")).toBeInTheDocument();
  });

  it("shows the help of the focused knob", () => {
    setup();
    act(() => screen.getByRole("slider", { name: "Decay" }).focus());
    expect(screen.getByText(/How fast the filter closes/)).toBeInTheDocument();
  });

  it("a keyboard step commits at once", () => {
    const { sent } = setup();
    fireEvent.keyDown(screen.getByRole("slider", { name: "Accent" }), { key: "ArrowUp" });
    expect(sent).toEqual([
      {
        cmd: "set_param",
        args: { track: "acid", param: "acid303.accent", value: expect.any(Number), commit: true },
      },
    ]);
  });

  it("switches the distortion mode", () => {
    const { sent } = setup();
    fireEvent.click(screen.getByRole("radio", { name: "Overdrive" }));
    expect(sent).toEqual([{ cmd: "set_fx_mode", args: { track: "acid", mode: "overdrive" } }]);
  });

  it("says when a release on an automated knob was not written", async () => {
    setup(goldenState.song!, { automated: true });
    fireEvent.keyDown(screen.getByRole("slider", { name: "Cutoff" }), { key: "ArrowUp" });
    expect(
      await screen.findByText(
        "Cutoff is automated where the playhead is, so the change was not saved. Edit its lane in song.toml.",
      ),
    ).toBeInTheDocument();
  });

  it("marks knobs automated where a release would land, not in the shown section", () => {
    const { unmount } = setup(goldenState.song!, null, 0);
    const cutoff = () => screen.getByRole("slider", { name: "Cutoff" });
    expect(within(cutoff()).queryByTestId("knob-lane")).toBeNull();
    unmount();
    setup(goldenState.song!, null, 1);
    expect(within(cutoff()).getByTestId("knob-lane")).toBeInTheDocument();
  });

  it("offers a tab per 303 track", () => {
    const song = stateEvent((s) => {
      const acid = structuredClone(s.song!.tracks[1]!);
      s.song!.tracks.push({ ...acid, id: "acid2" });
    }).song!;
    const { sent } = setup(song);
    const tabs = screen.getByRole("tablist", { name: "303 tracks" });
    fireEvent.click(within(tabs).getByRole("tab", { name: "acid2" }));
    expect(screen.getByRole("region", { name: "303 acid2" })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("slider", { name: "Cutoff" }), { key: "ArrowUp" });
    expect(sent[0]).toMatchObject({ args: { track: "acid2" } });
  });
});
