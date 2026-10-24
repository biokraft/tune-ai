import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MuteSolo } from "./MuteSolo";

function setup(extra = {}) {
  const onMute = vi.fn();
  const onSolo = vi.fn();
  render(
    <MuteSolo track="acid" mute={false} solo={false} onMute={onMute} onSolo={onSolo} {...extra} />,
  );
  return { onMute, onSolo };
}

describe("MuteSolo", () => {
  it("exposes aria-pressed on both buttons under the names TrackStrip uses", () => {
    setup({ mute: true });
    expect(screen.getByRole("button", { name: "M" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "S" })).toHaveAttribute("aria-pressed", "false");
  });

  it("M calls onMute(true) when not muted", () => {
    const { onMute, onSolo } = setup();
    fireEvent.click(screen.getByRole("button", { name: "M" }));
    expect(onMute).toHaveBeenCalledWith(true);
    expect(onSolo).not.toHaveBeenCalled();
  });

  it("M calls onMute(false) when muted, S toggles solo", () => {
    const { onMute, onSolo } = setup({ mute: true, solo: true });
    fireEvent.click(screen.getByRole("button", { name: "M" }));
    expect(onMute).toHaveBeenCalledWith(false);
    fireEvent.click(screen.getByRole("button", { name: "S" }));
    expect(onSolo).toHaveBeenCalledWith(false);
  });
});
