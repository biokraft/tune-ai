import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AcidStepSequencer, type AcidStep } from "./AcidStepSequencer";

const steps: AcidStep[] = Array.from({ length: 16 }, (_, i) => ({
  note: i === 1 ? null : 36,
  accent: false,
  slide: false,
}));

function setup(extra = {}) {
  const onChange = vi.fn();
  render(<AcidStepSequencer steps={steps} playingStep={null} onChange={onChange} {...extra} />);
  return { onChange };
}

describe("AcidStepSequencer", () => {
  it("accent toggle", async () => {
    const { onChange } = setup();
    await userEvent.click(screen.getAllByRole("button", { name: "Step 1 accent" })[0]!);
    expect(onChange).toHaveBeenCalledWith(0, { note: 36, accent: true, slide: false });
  });

  it("slide toggle reports aria-pressed", async () => {
    const { onChange } = setup();
    const b = screen.getByRole("button", { name: "Step 1 slide" });
    expect(b).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(b);
    expect(onChange).toHaveBeenCalledWith(0, { note: 36, accent: false, slide: true });
  });

  it("arrow up raises a semitone", () => {
    const { onChange } = setup();
    fireEvent.keyDown(screen.getByRole("button", { name: "Step 1 note" }), { key: "ArrowUp" });
    expect(onChange).toHaveBeenCalledWith(0, { note: 37, accent: false, slide: false });
  });

  it("shift arrow raises an octave and clamps", () => {
    const { onChange } = setup();
    const n = screen.getByRole("button", { name: "Step 1 note" });
    fireEvent.keyDown(n, { key: "ArrowUp", shiftKey: true });
    expect(onChange).toHaveBeenLastCalledWith(0, { note: 48, accent: false, slide: false });
    fireEvent.keyDown(n, { key: "ArrowDown", shiftKey: true });
    expect(onChange).toHaveBeenLastCalledWith(0, { note: 24, accent: false, slide: false });
  });

  it("clamps to 0..127", () => {
    const onChange = vi.fn();
    const edge = [
      { note: 5, accent: false, slide: false },
      { note: 125, accent: false, slide: false },
    ];
    render(<AcidStepSequencer steps={edge} playingStep={null} onChange={onChange} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Step 1 note" }), {
      key: "ArrowDown",
      shiftKey: true,
    });
    expect(onChange).toHaveBeenLastCalledWith(0, { note: 0, accent: false, slide: false });
    fireEvent.keyDown(screen.getByRole("button", { name: "Step 2 note" }), {
      key: "ArrowUp",
      shiftKey: true,
    });
    expect(onChange).toHaveBeenLastCalledWith(1, { note: 127, accent: false, slide: false });
  });

  it("a and s keys toggle flags", () => {
    const { onChange } = setup();
    const n = screen.getByRole("button", { name: "Step 1 note" });
    fireEvent.keyDown(n, { key: "a" });
    expect(onChange).toHaveBeenLastCalledWith(0, { note: 36, accent: true, slide: false });
    fireEvent.keyDown(n, { key: "s" });
    expect(onChange).toHaveBeenLastCalledWith(0, { note: 36, accent: false, slide: true });
  });

  it("piano picks a note", async () => {
    const { onChange } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Step 1 note" }));
    const piano = await screen.findByRole("dialog");
    // opens on the step's octave (C2 = 36 -> octave 2); pick the same octave explicitly
    const oct = within(piano).getByLabelText("Octave");
    expect(oct).toHaveTextContent("2");
    await userEvent.click(within(piano).getByRole("button", { name: "D#" }));
    expect(onChange).toHaveBeenCalledWith(0, { note: 39, accent: false, slide: false });
  });

  it("piano octave buttons move the octave", async () => {
    const { onChange } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Step 1 note" }));
    const piano = await screen.findByRole("dialog");
    await userEvent.click(within(piano).getByRole("button", { name: "Octave up" }));
    await userEvent.click(within(piano).getByRole("button", { name: "C" }));
    expect(onChange).toHaveBeenCalledWith(0, { note: 48, accent: false, slide: false });
  });

  it("piano rest key rests the step", async () => {
    const { onChange } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Step 1 note" }));
    const piano = await screen.findByRole("dialog");
    await userEvent.click(within(piano).getByRole("button", { name: "Rest" }));
    expect(onChange).toHaveBeenCalledWith(0, { note: null, accent: false, slide: false });
  });

  it("rest toggle round trip", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <AcidStepSequencer steps={steps} playingStep={null} onChange={onChange} />,
    );
    fireEvent.keyDown(screen.getByRole("button", { name: "Step 1 note" }), { key: "r" });
    expect(onChange).toHaveBeenLastCalledWith(0, { note: null, accent: false, slide: false });
    const rested = steps.map((s, i) => (i === 0 ? { ...s, note: null } : s));
    rerender(<AcidStepSequencer steps={rested} playingStep={null} onChange={onChange} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Step 1 note" }), { key: "Delete" });
    expect(onChange).toHaveBeenLastCalledWith(0, { note: 36, accent: false, slide: false });
  });

  it("leaving a rest with no history uses C2", () => {
    const { onChange } = setup();
    fireEvent.keyDown(screen.getByRole("button", { name: "Step 2 note" }), { key: "r" });
    expect(onChange).toHaveBeenCalledWith(1, { note: 36, accent: false, slide: false });
  });

  it("arrow left and right move focus", () => {
    setup();
    const n1 = screen.getByRole("button", { name: "Step 1 note" });
    n1.focus();
    fireEvent.keyDown(n1, { key: "ArrowRight" });
    expect(screen.getByRole("button", { name: "Step 2 note" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("button", { name: "Step 2 note" }), { key: "ArrowLeft" });
    expect(n1).toHaveFocus();
  });

  it("playing step is lit", () => {
    const { container } = render(
      <AcidStepSequencer steps={steps} playingStep={3} onChange={() => {}} />,
    );
    const cols = container.querySelectorAll("[data-step]");
    expect(cols[3]).toHaveAttribute("data-playing", "true");
    expect(cols[2]).toHaveAttribute("data-playing", "false");
  });

  it("marks highlighted columns", () => {
    const { container } = render(
      <AcidStepSequencer
        steps={steps}
        playingStep={null}
        highlighted={new Set([2])}
        onChange={() => {}}
      />,
    );
    expect(container.querySelectorAll("[data-step]")[2]).toHaveAttribute(
      "data-highlight",
      "claude",
    );
  });
});
