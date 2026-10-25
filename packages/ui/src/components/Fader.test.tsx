import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Fader } from "./Fader";

function setup(value = 0, extra = {}) {
  const onChange = vi.fn();
  const onCommit = vi.fn();
  render(
    <Fader
      label="Gain"
      value={value}
      min={-60}
      max={12}
      defaultValue={0}
      unit="dB"
      onChange={onChange}
      onCommit={onCommit}
      {...extra}
    />,
  );
  return { onChange, onCommit, fader: screen.getByRole("slider", { name: "Gain" }) };
}

describe("Fader", () => {
  it("exposes vertical slider semantics", () => {
    const { fader } = setup();
    expect(fader).toHaveAttribute("aria-orientation", "vertical");
    expect(fader).toHaveAttribute("aria-valuemin", "-60");
    expect(fader).toHaveAttribute("aria-valuemax", "12");
    expect(fader).toHaveAttribute("aria-valuenow", "0");
    expect(fader).toHaveAttribute("aria-valuetext", "0.0 dB");
  });

  it("marks 0 dB on the slot", () => {
    setup(-12);
    // -60..12 dB: 0 dB sits 60/72 of the way up.
    expect(screen.getByTestId("fader-zero").style.bottom).toBe(`${(60 / 72) * 100}%`);
  });

  it("formats a negative value", () => {
    const { fader } = setup(-3);
    expect(fader).toHaveAttribute("aria-valuetext", "-3.0 dB");
  });

  it("steps 0.5 with the arrows and commits each time", () => {
    const { fader, onCommit } = setup();
    fireEvent.keyDown(fader, { key: "ArrowUp" });
    expect(onCommit).toHaveBeenLastCalledWith(0.5);
    fireEvent.keyDown(fader, { key: "ArrowDown" });
    expect(onCommit).toHaveBeenLastCalledWith(-0.5);
  });

  it("steps 3 with the page keys", () => {
    const { fader, onCommit } = setup();
    fireEvent.keyDown(fader, { key: "PageDown" });
    expect(onCommit).toHaveBeenLastCalledWith(-3);
    fireEvent.keyDown(fader, { key: "PageUp" });
    expect(onCommit).toHaveBeenLastCalledWith(3);
  });

  it("clamps to the range", () => {
    const { fader, onCommit } = setup(12);
    fireEvent.keyDown(fader, { key: "ArrowUp" });
    expect(onCommit).toHaveBeenLastCalledWith(12);
    fireEvent.keyDown(fader, { key: "PageDown" });
    expect(onCommit).toHaveBeenLastCalledWith(9);
  });

  it("commits the default on double-click", () => {
    const { fader, onCommit } = setup(-20);
    fireEvent.doubleClick(fader);
    expect(onCommit).toHaveBeenCalledWith(0);
  });

  it("drags 160px across the full range, Shift ten times finer", () => {
    const { fader, onChange, onCommit } = setup(-60);
    fireEvent.pointerDown(fader, { clientY: 200, button: 0, pointerId: 1 });
    fireEvent.pointerMove(fader, { clientY: 120, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(-24);
    fireEvent.pointerMove(fader, { clientY: 40, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(12);
    fireEvent.pointerUp(fader, { clientY: 40, pointerId: 1 });
    expect(onCommit).toHaveBeenCalledWith(12);
  });

  it("drags finely with Shift", () => {
    const { fader, onChange } = setup(-60);
    fireEvent.pointerDown(fader, { clientY: 200, button: 0, pointerId: 1, shiftKey: true });
    fireEvent.pointerMove(fader, { clientY: 40, pointerId: 1, shiftKey: true });
    expect(onChange).toHaveBeenLastCalledWith(-60 + 72 * 0.1);
  });

  it("does not commit a click without movement", () => {
    const { fader, onCommit } = setup();
    fireEvent.pointerDown(fader, { clientY: 100, button: 0, pointerId: 1 });
    fireEvent.pointerUp(fader, { clientY: 100, pointerId: 1 });
    expect(onCommit).not.toHaveBeenCalled();
  });
});
