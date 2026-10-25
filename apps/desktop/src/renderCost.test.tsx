import { act, fireEvent, render, screen } from "@testing-library/react";
import App from "./App";
import { FakeClient, stateEvent } from "./client/fakeClient";
import { createStudioStore } from "./store/studioStore";

// Counts Knob renders: if the dock's Sound tab re-renders, every knob in it does.
const knobRenders = vi.hoisted(() => ({ count: 0 }));
vi.mock("@tune/ui", async (importOriginal) => {
  const ui = await importOriginal<typeof import("@tune/ui")>();
  return {
    ...ui,
    Knob: (props: Parameters<typeof ui.Knob>[0]) => {
      knobRenders.count += 1;
      return ui.Knob(props);
    },
  };
});

function setup(track: string) {
  const client = new FakeClient();
  render(<App store={createStudioStore(client)} />);
  act(() => client.emit(stateEvent((s) => (s.playing = true))));
  fireEvent.click(screen.getByTestId("section-1"));
  fireEvent.click(screen.getByRole("button", { name: `Open ${track}` }));
  const tick = (step: number) =>
    act(() =>
      client.emit({
        event: "playhead",
        step,
        bar: Math.floor(step / 16),
        beat: Math.floor((step % 16) / 4),
        playing: true,
      }),
    );
  // Section 1 starts at bar 2 (step 32). Entering the section may re-render the dock once: knob
  // lane badges follow the section under the playhead.
  tick(32);
  knobRenders.count = 0;
  for (let step = 33; step <= 42; step++) tick(step);
  return tick;
}

describe("playhead ticks", () => {
  it("move the position and the playhead without re-rendering the docked 303's knobs", () => {
    setup("acid");
    expect(knobRenders.count).toBe(0);
    expect(screen.getByLabelText("Position")).toHaveTextContent("3.3");
    expect(screen.getByTestId("playhead")).toHaveAttribute("x1", String((42 / 16) * 12));
    // a1 has 8 steps: step 42 is its step 2.
    fireEvent.click(screen.getByRole("tab", { name: "Steps" }));
    const playing = document.querySelector('[data-step][data-playing="true"]');
    expect(playing).toHaveAttribute("data-step", "2");
  });

  it("chase the docked drum grid without re-rendering the drum voice's knobs", () => {
    const tick = setup("kick");
    expect(knobRenders.count).toBe(0);
    const playing = () =>
      screen.getByRole("group", { name: "kick" }).querySelectorAll('[data-playing="true"]');
    expect(playing()).toHaveLength(1);
    tick(43);
    expect(knobRenders.count).toBe(0);
    expect(playing()).toHaveLength(1);
  });
});
