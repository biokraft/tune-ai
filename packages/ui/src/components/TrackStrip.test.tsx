import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { stripParams } from "../test-fixtures";
import { TrackStrip } from "./TrackStrip";

const track = { id: "acid", gain_db: 0, pan: 0, mute: false, solo: false };

describe("TrackStrip", () => {
  it("M toggles mute", () => {
    const onMix = vi.fn();
    render(<TrackStrip track={track} onMix={onMix} />);
    fireEvent.click(screen.getByRole("button", { name: "M" }));
    expect(onMix).toHaveBeenCalledWith({ mute: true });
  });
  it("S toggles solo and reflects state", () => {
    const onMix = vi.fn();
    render(<TrackStrip track={{ ...track, solo: true }} onMix={onMix} />);
    const s = screen.getByRole("button", { name: "S" });
    expect(s).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(s);
    expect(onMix).toHaveBeenCalledWith({ solo: false });
  });
  it("gain knob commits on release only", () => {
    const onMix = vi.fn();
    render(<TrackStrip track={track} onMix={onMix} />);
    const gain = screen.getByRole("slider", { name: "Gain" });
    fireEvent.pointerDown(gain, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(gain, { clientY: 280, pointerId: 1 });
    expect(onMix).not.toHaveBeenCalled();
    fireEvent.pointerUp(gain, { clientY: 280, pointerId: 1 });
    expect(onMix).toHaveBeenCalledTimes(1);
    expect(onMix.mock.calls[0]![0]).toHaveProperty("gain_db");
  });

  it("renders send knobs when strip given", () => {
    render(<TrackStrip track={track} onMix={vi.fn()} strip={stripParams} />);
    for (const name of ["Delay", "Reverb", "Duck"]) {
      expect(screen.getByRole("slider", { name })).toBeInTheDocument();
    }
    expect(screen.queryByRole("slider", { name: "Duck release" })).toBeNull();
  });
  it("send knob release calls onParam(id, v, true)", () => {
    const onParam = vi.fn();
    render(<TrackStrip track={track} onMix={vi.fn()} strip={stripParams} onParam={onParam} />);
    const send = screen.getByRole("slider", { name: "Delay" });
    fireEvent.pointerDown(send, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(send, { clientY: 280, pointerId: 1 });
    expect(onParam).toHaveBeenLastCalledWith("send.delay", expect.any(Number), false);
    fireEvent.pointerUp(send, { clientY: 280, pointerId: 1 });
    expect(onParam).toHaveBeenLastCalledWith("send.delay", expect.any(Number), true);
  });
  it("no strip, no send knobs", () => {
    render(<TrackStrip track={track} onMix={vi.fn()} />);
    expect(screen.queryByRole("slider", { name: "Delay" })).toBeNull();
    expect(screen.queryByRole("slider", { name: "Duck" })).toBeNull();
  });
});
