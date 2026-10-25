import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TransportBar, type TransportBarProps } from "./TransportBar";

function setup(extra: Partial<TransportBarProps> = {}) {
  const fns = {
    onPlay: vi.fn(),
    onStop: vi.fn(),
    onToggleLoop: vi.fn(),
    onUndo: vi.fn(),
    onRedo: vi.fn(),
    onToggleMixer: vi.fn(),
  };
  render(
    <TransportBar
      playing={false}
      bpm={145}
      bar={2}
      beat={2}
      loopOn={false}
      canLoop={true}
      canUndo={true}
      canRedo={false}
      songName="first-acid"
      totalBars={144}
      loopBars={null}
      loopName={null}
      mixerOpen={false}
      {...fns}
      {...extra}
    />,
  );
  return fns;
}

describe("TransportBar", () => {
  it("loop disabled without selection", () => {
    setup({ canLoop: false });
    expect(screen.getByRole("button", { name: "Loop (L)" })).toBeDisabled();
  });
  it("loop button reflects and toggles the loop", () => {
    const f = setup({ loopOn: true });
    const loop = screen.getByRole("button", { name: "Loop (L)" });
    expect(loop).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(loop);
    expect(f.onToggleLoop).toHaveBeenCalled();
  });
  it("Play click calls onPlay; shows position, time and tempo", () => {
    const f = setup();
    fireEvent.click(screen.getByRole("button", { name: "Play (space)" }));
    expect(f.onPlay).toHaveBeenCalled();
    expect(screen.getByLabelText("Position")).toHaveTextContent("3.3");
    expect(screen.getByLabelText("Time")).toHaveTextContent("0:04.1");
    expect(screen.getByLabelText("BPM")).toHaveTextContent("145");
    expect(screen.getByText("145 BPM · 144 bars")).toBeInTheDocument();
  });
  it("names the button Pause while playing", () => {
    setup({ playing: true });
    expect(screen.getByRole("button", { name: "Pause (space)" })).toBeInTheDocument();
  });
  it("Pause click while playing calls onStop, not onPlay", () => {
    const f = setup({ playing: true });
    fireEvent.click(screen.getByRole("button", { name: "Pause (space)" }));
    expect(f.onStop).toHaveBeenCalledTimes(1);
    expect(f.onPlay).not.toHaveBeenCalled();
  });
  it("Stop calls onStop", () => {
    const f = setup();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(f.onStop).toHaveBeenCalled();
  });
  it("shows the loop range one-based inclusive with its section name", () => {
    setup({ loopBars: [80, 96], loopName: "drop" });
    expect(screen.getByLabelText("Loop range")).toHaveTextContent("81 – 96");
    expect(screen.getByLabelText("Loop range")).toHaveTextContent("drop");
  });
  it("hides the loop range without a loop", () => {
    setup();
    expect(screen.queryByLabelText("Loop range")).toBeNull();
  });
  it("undo and redo respect their flags", () => {
    const f = setup();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(f.onUndo).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Redo" })).toBeDisabled();
  });
  it("Mixer toggles through onToggleMixer", () => {
    const f = setup({ mixerOpen: true });
    const mixer = screen.getByRole("button", { name: "Mixer (M)" });
    expect(mixer).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(mixer);
    expect(f.onToggleMixer).toHaveBeenCalled();
  });
  it("has no notice slot", () => {
    setup();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
