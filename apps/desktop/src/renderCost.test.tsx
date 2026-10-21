import { act, fireEvent, render, screen } from "@testing-library/react";
import App from "./App";
import { FakeClient, stateEvent } from "./client/fakeClient";
import { createStudioStore } from "./store/studioStore";

// Counts renders of the Knobs the 303 panel draws: if Panel303 re-renders, all 12 do.
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

describe("playhead ticks", () => {
  it("move the LEDs and the position without re-rendering the 303 panel", () => {
    const client = new FakeClient();
    render(<App store={createStudioStore(client)} />);
    act(() => client.emit(stateEvent((s) => (s.playing = true))));
    fireEvent.click(screen.getByTestId("section-1"));
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
    // Section 1 starts at bar 2 (step 32); a1 has 8 steps. Entering the section may re-render
    // the panel once: knob lane badges follow the section under the playhead.
    tick(32);
    knobRenders.count = 0;

    for (let step = 33; step <= 42; step++) tick(step);

    expect(knobRenders.count).toBe(0);
    const playing = document.querySelector('[data-step][data-playing="true"]');
    expect(playing).toHaveAttribute("data-step", "2");
    expect(screen.getByLabelText("Position")).toHaveTextContent("003:3");
    expect(screen.getByTestId("playhead")).toHaveAttribute("x1", String((42 / 16) * 12));
  });
});
