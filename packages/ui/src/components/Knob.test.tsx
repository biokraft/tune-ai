import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { formatValue, toNormalized, toPlain } from "../lib/param";
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

  it("gives each knob its own gradient id", () => {
    const { container } = render(
      <>
        <Knob spec={cutoff} value={500} onChange={() => {}} onCommit={() => {}} />
        <Knob spec={cutoff} value={900} onChange={() => {}} onCommit={() => {}} />
      </>,
    );
    const ids = [...container.querySelectorAll("radialGradient")].map((g) => g.id);
    expect(ids).toHaveLength(2);
    expect(new Set(ids).size).toBe(2);
    const fills = [...container.querySelectorAll("circle[fill^='url(']")].map((c) =>
      c.getAttribute("fill"),
    );
    expect(fills).toEqual(ids.map((id) => `url(#${id})`));
  });

  it("does not commit on a click without movement", () => {
    const { knob, onCommit } = setup(0.3);
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerUp(knob, { clientY: 300, pointerId: 1 });
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("double click commits exactly once, with the default", () => {
    const { knob, onCommit } = setup(0.3);
    for (let i = 0; i < 2; i++) {
      fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
      fireEvent.pointerUp(knob, { clientY: 300, pointerId: 1 });
    }
    fireEvent.doubleClick(knob);
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(cutoff.default);
  });

  it("commits the last value when the pointer is cancelled after a move", () => {
    const { knob, onCommit } = setup(0);
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 200, pointerId: 1 });
    fireEvent.pointerCancel(knob, { pointerId: 1 });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(toPlain(cutoff, 0.5));
  });
  it("draws the value arc in the track colour", () => {
    render(
      <Knob
        spec={cutoff}
        value={500}
        onChange={() => {}}
        onCommit={() => {}}
        color="rgb(var(--acid))"
      />,
    );
    expect(screen.getByTestId("knob-arc")).toHaveAttribute("stroke", "rgb(var(--acid))");
  });

  it("marks Claude's edit with an AI badge, and other edits without one", () => {
    const { rerender } = render(
      <Knob spec={cutoff} value={500} onChange={() => {}} onCommit={() => {}} highlight="claude" />,
    );
    expect(screen.getByTestId("knob-highlight")).toBeInTheDocument();
    expect(screen.getByText("AI")).toBeInTheDocument();
    rerender(
      <Knob
        spec={cutoff}
        value={500}
        onChange={() => {}}
        onCommit={() => {}}
        highlight="external"
      />,
    );
    expect(screen.getByTestId("knob-highlight")).toBeInTheDocument();
    expect(screen.queryByText("AI")).not.toBeInTheDocument();
  });

  it("ignores keys and drags when disabled", async () => {
    const onCommit = vi.fn();
    const onChange = vi.fn();
    render(<Knob spec={cutoff} value={500} onChange={onChange} onCommit={onCommit} disabled />);
    const k = screen.getByRole("slider", { name: /cutoff/i });
    expect(k).toHaveAttribute("aria-disabled", "true");
    k.focus();
    await userEvent.keyboard("{ArrowUp}");
    fireEvent.pointerDown(k, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(k, { clientY: 200, pointerId: 1 });
    fireEvent.pointerUp(k, { clientY: 200, pointerId: 1 });
    expect(onCommit).not.toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.queryByTestId("knob-chip")).not.toBeInTheDocument();
  });

  it("shows a value chip while dragging", () => {
    const { knob } = setup(0.5);
    expect(screen.queryByTestId("knob-chip")).not.toBeInTheDocument();
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    expect(screen.getByTestId("knob-chip")).toHaveTextContent(
      formatValue(cutoff, toPlain(cutoff, 0.5)),
    );
    fireEvent.pointerUp(knob, { clientY: 300, pointerId: 1 });
    expect(screen.queryByTestId("knob-chip")).not.toBeInTheDocument();
  });
});
