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
  };
  render(
    <TransportBar
      playing={false}
      bpm={145}
      bar={32}
      beat={1}
      loopOn={false}
      canLoop={true}
      canUndo={true}
      canRedo={false}
      songName="first-acid"
      {...fns}
      {...extra}
    />,
  );
  return fns;
}

describe("TransportBar", () => {
  it("loop disabled without selection", () => {
    setup({ canLoop: false });
    expect(screen.getByRole("button", { name: "Loop selected section" })).toBeDisabled();
  });
  it("Play click calls onPlay; shows position", () => {
    const f = setup();
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(f.onPlay).toHaveBeenCalled();
    expect(screen.getByLabelText("Position")).toHaveTextContent("033:2");
  });
  it("undo and redo respect their flags", () => {
    const f = setup();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(f.onUndo).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Redo" })).toBeDisabled();
  });
  it("renders the notice", () => {
    setup({ notice: "Claude changed 2 things" });
    expect(screen.getByRole("status")).toHaveTextContent("Claude changed 2 things");
  });
});
