import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { delayFeedback, delayTime } from "../test-fixtures";
import { FxPanel } from "./FxPanel";

const params = [delayTime, delayFeedback];

describe("FxPanel", () => {
  it("one knob per param", () => {
    render(<FxPanel title="Delay" params={params} onParam={vi.fn()} />);
    expect(screen.getByRole("group", { name: "Delay" })).toBeInTheDocument();
    expect(screen.getAllByRole("slider").map((s) => s.getAttribute("aria-label"))).toEqual([
      "Time",
      "Feedback",
    ]);
  });
  it("inactive note dims the panel", () => {
    render(
      <FxPanel
        title="Delay"
        params={params}
        inactiveNote="No track sends here yet"
        onParam={vi.fn()}
      />,
    );
    expect(screen.getByText("No track sends here yet")).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Delay" })).toHaveAttribute("data-inactive", "true");
  });
  it("is not dimmed without a note", () => {
    render(<FxPanel title="Delay" params={params} onParam={vi.fn()} />);
    expect(screen.getByRole("group", { name: "Delay" })).not.toHaveAttribute("data-inactive");
  });
  it("keyboard step commits", () => {
    const onParam = vi.fn();
    render(<FxPanel title="Delay" params={params} onParam={onParam} />);
    fireEvent.keyDown(screen.getByRole("slider", { name: "Feedback" }), { key: "ArrowUp" });
    expect(onParam).toHaveBeenCalledWith("delay.feedback", expect.any(Number), true);
  });
  it("shows the header slot beside the title", () => {
    render(
      <FxPanel
        title="Distortion"
        params={params}
        onParam={vi.fn()}
        header={<span>mode slot</span>}
      />,
    );
    expect(screen.getByText("mode slot")).toBeInTheDocument();
  });
  it("passes the colour to the knobs", () => {
    render(<FxPanel title="Delay" params={params} onParam={vi.fn()} color="rgb(var(--acid))" />);
    expect(screen.getAllByTestId("knob-arc")[0]!.outerHTML).toContain("--acid");
  });
});
