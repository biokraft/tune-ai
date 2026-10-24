import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { IconButton } from "./IconButton";

describe("IconButton", () => {
  it("is named by label and calls onClick", () => {
    const onClick = vi.fn();
    render(
      <IconButton label="Undo" onClick={onClick}>
        <svg />
      </IconButton>,
    );
    const b = screen.getByRole("button", { name: "Undo" });
    expect(b).not.toHaveAttribute("aria-pressed");
    fireEvent.click(b);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("sets aria-pressed when pressed is defined", () => {
    const { rerender } = render(
      <IconButton label="Loop" pressed onClick={() => {}}>
        <svg />
      </IconButton>,
    );
    expect(screen.getByRole("button", { name: "Loop" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Loop" })).toHaveClass(
      "bg-ink/[.12]",
      "border-line2",
    );
    rerender(
      <IconButton label="Loop" pressed={false} onClick={() => {}}>
        <svg />
      </IconButton>,
    );
    expect(screen.getByRole("button", { name: "Loop" })).toHaveAttribute("aria-pressed", "false");
  });

  it("renders the kbd chip", () => {
    render(
      <IconButton label="Play" kbd="Space" onClick={() => {}}>
        <svg />
      </IconButton>,
    );
    expect(screen.getByText("Space")).toHaveClass("kbd");
  });

  it("does not click when disabled", () => {
    const onClick = vi.fn();
    render(
      <IconButton label="Undo" disabled onClick={onClick}>
        <svg />
      </IconButton>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(onClick).not.toHaveBeenCalled();
  });
});
