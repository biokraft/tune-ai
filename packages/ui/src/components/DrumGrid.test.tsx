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

describe("DrumGrid emphasis", () => {
  it("marks only the emphasised row and dims the others", () => {
    render(
      <DrumGrid
        rows={[
          {
            pattern: "k4",
            label: "kick",
            hits: ["hit"],
            emphasis: true,
            color: "rgb(var(--kick))",
          },
          { pattern: "h", label: "hat", hits: ["hit"], color: "rgb(var(--hat))" },
        ]}
        playingStep={null}
        onSet={() => {}}
      />,
    );
    expect(screen.getByRole("group", { name: "kick" })).toHaveAttribute("data-emphasis", "true");
    const hat = screen.getByRole("group", { name: "hat" });
    expect(hat).not.toHaveAttribute("data-emphasis");
    expect(hat.className).toContain("opacity-[.55]");
  });
  it("fills hits at 55% and accents fully in the row colour", () => {
    render(
      <DrumGrid
        rows={[{ pattern: "k4", label: "kick", hits: ["hit", "accent"], color: "rgb(255 0 0)" }]}
        playingStep={null}
        onSet={() => {}}
      />,
    );
    const hit = screen.getByRole("button", { name: "kick step 1: hit" });
    const acc = screen.getByRole("button", { name: "kick step 2: accent" });
    const fill = (b: HTMLElement) => b.querySelector<HTMLElement>("[data-fill]")!;
    expect(fill(hit).style.opacity).toBe("0.55");
    expect(fill(hit).style.backgroundColor).toBe("rgb(255, 0, 0)");
    expect(fill(acc).style.opacity).toBe("1");
    expect(fill(acc).style.backgroundColor).toBe("rgb(255, 0, 0)");
  });
});
