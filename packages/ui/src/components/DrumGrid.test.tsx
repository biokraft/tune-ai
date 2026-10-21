import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DrumGrid } from "./DrumGrid";

const rows = [{ pattern: "k4", label: "kick", hits: ["rest", "hit", "accent", "rest"] as const }];

function setup(extra = {}) {
  const onSet = vi.fn();
  render(
    <DrumGrid
      rows={rows.map((r) => ({ ...r, hits: [...r.hits] }))}
      playingStep={null}
      onSet={onSet}
      {...extra}
    />,
  );
  return { onSet };
}

describe("DrumGrid", () => {
  it("rest becomes hit", async () => {
    const { onSet } = setup();
    await userEvent.click(screen.getByRole("button", { name: "kick step 1: rest" }));
    expect(onSet).toHaveBeenCalledWith("k4", 0, "hit");
  });
  it("hit becomes accent, accent becomes rest", async () => {
    const { onSet } = setup();
    await userEvent.click(screen.getByRole("button", { name: "kick step 2: hit" }));
    expect(onSet).toHaveBeenLastCalledWith("k4", 1, "accent");
    await userEvent.click(screen.getByRole("button", { name: "kick step 3: accent" }));
    expect(onSet).toHaveBeenLastCalledWith("k4", 2, "rest");
  });
  it("marks the playing step and highlights", () => {
    setup({
      playingStep: 1,
      rows: [{ ...rows[0]!, hits: [...rows[0]!.hits], highlighted: new Set([3]) }],
    });
    expect(screen.getByRole("button", { name: "kick step 2: hit" })).toHaveAttribute(
      "data-playing",
      "true",
    );
    expect(screen.getByRole("button", { name: "kick step 4: rest" })).toHaveAttribute(
      "data-highlight",
      "claude",
    );
  });
});
