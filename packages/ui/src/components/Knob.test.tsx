import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { toNormalized, toPlain } from "../lib/param";
import { cutoff } from "../test-fixtures";
import { Knob } from "./Knob";

function setup(valueNorm = 0, extra = {}) {
  const onChange = vi.fn();
  const onCommit = vi.fn();
  const onHelp = vi.fn();
  render(
    <Knob
      spec={cutoff}
      value={toPlain(cutoff, valueNorm)}
      onChange={onChange}
      onCommit={onCommit}
      onHelp={onHelp}
      {...extra}
    />,
  );
  return { onChange, onCommit, onHelp, knob: screen.getByRole("slider") };
}

describe("Knob", () => {
  it("exposes slider semantics", () => {
    render(<Knob spec={cutoff} value={500} onChange={() => {}} onCommit={() => {}} />);
    const knob = screen.getByRole("slider", { name: "Cutoff" });
    expect(knob).toHaveAttribute("aria-valuemin", "314");
    expect(knob).toHaveAttribute("aria-valuemax", "2394");
    expect(knob).toHaveAttribute("aria-valuenow", "500");
    expect(knob).toHaveAttribute("aria-valuetext", "500 Hz");
  });

  it("vertical drag maps 200 px to full range", () => {
    const { knob, onChange, onCommit } = setup(0);
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 200, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(toPlain(cutoff, 0.5));
    expect(onCommit).not.toHaveBeenCalled();
    fireEvent.pointerUp(knob, { clientY: 200, pointerId: 1 });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(toPlain(cutoff, 0.5));
  });

  it("shift drag is ten times finer", () => {
    const { knob, onChange } = setup(0);
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1, shiftKey: true });
    fireEvent.pointerMove(knob, { clientY: 200, pointerId: 1, shiftKey: true });
    const last = onChange.mock.lastCall![0] as number;
    expect(toNormalized(cutoff, last)).toBeCloseTo(0.05, 9);
  });

  it("does not jump when shift is pressed mid drag", () => {
    const { knob, onChange } = setup(0);
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 200, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 200, pointerId: 1, shiftKey: true });
    expect(toNormalized(cutoff, onChange.mock.lastCall![0] as number)).toBeCloseTo(0.5, 9);
  });

  it("ignores moves when no drag is active", () => {
    const { knob, onChange } = setup(0);
    fireEvent.pointerMove(knob, { clientY: 100, pointerId: 1 });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("double click resets to default", () => {
    const { knob, onCommit } = setup(0.8);
    fireEvent.doubleClick(knob);
    expect(onCommit).toHaveBeenCalledWith(cutoff.default);
  });

  it("keyboard steps", () => {
    const n = 0.4;
    const { knob, onCommit } = setup(n);
    fireEvent.keyDown(knob, { key: "ArrowUp" });
    expect(onCommit).toHaveBeenLastCalledWith(toPlain(cutoff, n + 0.01));
    fireEvent.keyDown(knob, { key: "ArrowRight", shiftKey: true });
    expect(onCommit).toHaveBeenLastCalledWith(toPlain(cutoff, n + 0.001));
    fireEvent.keyDown(knob, { key: "ArrowDown" });
    expect(onCommit).toHaveBeenLastCalledWith(toPlain(cutoff, n - 0.01));
    fireEvent.keyDown(knob, { key: "PageUp" });
    expect(onCommit).toHaveBeenLastCalledWith(toPlain(cutoff, n + 0.1));
    fireEvent.keyDown(knob, { key: "PageDown" });
    expect(onCommit).toHaveBeenLastCalledWith(toPlain(cutoff, n - 0.1));
    fireEvent.keyDown(knob, { key: "Home" });
    expect(onCommit).toHaveBeenLastCalledWith(cutoff.min);
    fireEvent.keyDown(knob, { key: "End" });
    expect(onCommit).toHaveBeenLastCalledWith(cutoff.max);
  });

  it("clamps at the ends", () => {
    const { knob, onCommit } = setup(1);
    fireEvent.keyDown(knob, { key: "ArrowUp" });
    expect(onCommit).toHaveBeenCalledWith(cutoff.max);
  });

  it("focus reports help", () => {
    const { knob, onHelp } = setup(0.2);
    fireEvent.focus(knob);
    expect(onHelp).toHaveBeenLastCalledWith(cutoff);
    fireEvent.blur(knob);
    expect(onHelp).toHaveBeenLastCalledWith(null);
  });

  it("marks automated and highlighted knobs", () => {
    setup(0.2, { automated: true, highlight: "claude" });
    expect(screen.getByTestId("knob-lane")).toBeInTheDocument();
    expect(screen.getByTestId("knob-highlight")).toBeInTheDocument();
  });
});
