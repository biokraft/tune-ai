import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../../App";
import { FakeClient, stateEvent } from "../../client/fakeClient";
import type { StateEvent } from "../../client/protocol";
import { createStudioStore } from "../../store/studioStore";

function setup(state: StateEvent = stateEvent()) {
  const client = new FakeClient();
  render(<App store={createStudioStore(client)} />);
  act(() => client.emit(state));
  return client;
}

const press = (init: KeyboardEventInit, target: EventTarget = document.body) =>
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }),
    );
  });

const dock = () => screen.getByRole("region", { name: "Dock" });
const noDock = () => expect(screen.queryByRole("region", { name: "Dock" })).toBeNull();
const open = (track: string) =>
  fireEvent.click(screen.getByRole("button", { name: `Open ${track}` }));

describe("Dock", () => {
  it("is closed until a track header opens it", () => {
    setup();
    noDock();
  });

  it("opens the acid track on Sound, and Steps shows the 303 sequencer", () => {
    setup();
    fireEvent.click(screen.getByTestId("section-1"));
    open("acid");
    const tabs = within(dock()).getByRole("tablist");
    expect(within(tabs).getByRole("tab", { name: "Sound" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(dock()).getByRole("slider", { name: "Cutoff" })).toBeInTheDocument();
    expect(within(dock()).queryByRole("group", { name: "303 step sequencer" })).toBeNull();
    fireEvent.click(within(tabs).getByRole("tab", { name: "Steps" }));
    expect(within(tabs).getByRole("tab", { name: "Steps" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(dock()).getByRole("group", { name: "303 step sequencer" })).toBeInTheDocument();
  });

  it("names the track, its section and bars in the header", () => {
    setup();
    fireEvent.click(screen.getByTestId("section-1"));
    open("acid");
    const chip = within(dock()).getByTestId("dock-track");
    expect(chip).toHaveTextContent("acid");
    expect(chip).toHaveTextContent("Acid 303");
    expect(within(dock()).getByText("b · bars 3–4")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open acid" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("badges a pattern that plays in more than one section", () => {
    setup();
    open("kick");
    expect(within(dock()).getByText("×2")).toBeInTheDocument();
    open("acid");
    fireEvent.click(screen.getByTestId("section-1"));
    expect(within(dock()).queryByText(/^×/)).toBeNull();
  });

  it("shows a drum track's sound and steps together, with no tabs", () => {
    setup();
    open("kick");
    expect(within(dock()).getAllByRole("slider").length).toBeGreaterThan(0);
    expect(within(dock()).getByRole("group", { name: "kick" })).toBeInTheDocument();
    expect(within(dock()).queryByRole("tablist")).toBeNull();
  });

  it("the open track's header closes it again, another track swaps in", () => {
    setup();
    open("acid");
    open("kick");
    expect(within(dock()).getByRole("group", { name: "kick" })).toBeInTheDocument();
    open("kick");
    noDock();
  });

  it("M shows the mixer and M again hides it", () => {
    setup();
    press({ key: "m" });
    expect(within(dock()).getByRole("group", { name: "Track hat" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Mixer (M)" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    press({ key: "m", repeat: true });
    expect(dock()).toBeInTheDocument();
    press({ key: "m", metaKey: true });
    expect(dock()).toBeInTheDocument();
    press({ key: "m" });
    noDock();
  });

  it("the transport's Mixer button toggles the mixer", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Mixer (M)" }));
    expect(within(dock()).getByRole("group", { name: "Track kick" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Mixer (M)" }));
    noDock();
  });

  it("Esc closes the dock, and so does its close button", () => {
    setup();
    open("acid");
    press({ key: "Escape" });
    noDock();
    open("acid");
    fireEvent.click(within(dock()).getByRole("button", { name: "Close dock" }));
    noDock();
  });

  it("with the pattern picker open, Esc closes only the picker; a second Esc the dock", async () => {
    const user = userEvent.setup();
    setup();
    open("acid");
    await user.click(within(dock()).getByRole("button", { name: "acid in section 1" }));
    expect(screen.getByRole("button", { name: "None" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "None" })).toBeNull();
    expect(dock()).toBeInTheDocument();
    await user.keyboard("{Escape}");
    noDock();
  });

  it("Esc on the picker's trigger while it is open closes only the picker", async () => {
    const user = userEvent.setup();
    setup();
    open("acid");
    const trigger = within(dock()).getByRole("button", { name: "acid in section 1" });
    await user.click(trigger);
    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "None" })).toBeNull();
    expect(dock()).toBeInTheDocument();
  });

  it("picking a pattern fires set_section_play for the dock's section", async () => {
    const user = userEvent.setup();
    const client = setup();
    fireEvent.click(screen.getByTestId("section-1"));
    open("acid");
    const trigger = within(dock()).getByRole("button", { name: "acid in section 2" });
    expect(trigger).toHaveTextContent("a1");
    await user.click(trigger);
    expect(screen.getByRole("button", { name: "a1" })).toHaveAttribute("aria-current", "true");
    await user.click(screen.getByRole("button", { name: "None" }));
    expect(client.named("set_section_play")).toEqual([
      { cmd: "set_section_play", args: { section: 1, track: "acid", pattern: null } },
    ]);
    expect(screen.queryByRole("button", { name: "None" })).toBeNull();
  });

  it("closes without throwing when a state update removes the docked track", () => {
    const client = setup();
    open("hat");
    expect(dock()).toBeInTheDocument();
    act(() =>
      client.emit(
        stateEvent((s) => {
          s.song!.tracks = s.song!.tracks.filter((t) => t.id !== "hat");
          for (const sec of s.song!.sections) delete sec.play.hat;
        }),
      ),
    );
    noDock();
  });

  it("puts the help line in the dock, or under the timeline when the dock is closed", () => {
    setup();
    const hint = "Hover or focus a knob to learn what it does.";
    const timeline = screen.getByRole("region", { name: "Song arrangement" });
    expect(within(timeline).getByText(hint)).toBeInTheDocument();
    open("acid");
    expect(within(timeline).queryByText(hint)).toBeNull();
    act(() => within(dock()).getByRole("slider", { name: "Cutoff" }).focus());
    expect(dock().querySelector('[aria-live="polite"]')).toHaveTextContent(/^Cutoff/);
  });
});
