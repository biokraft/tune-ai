import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TuneMark } from "./TuneMark";

describe("TuneMark", () => {
  it("is hidden from assistive tech by default", () => {
    const { container } = render(<TuneMark />);
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("is an image named TuneAI when standalone", () => {
    render(<TuneMark standalone />);
    expect(screen.getByRole("img", { name: "TuneAI" })).toBeInTheDocument();
  });

  it("takes its size in px", () => {
    const { container } = render(<TuneMark size={40} />);
    const svg = container.querySelector("svg");
    expect(svg).toHaveAttribute("width", "40");
    expect(svg).toHaveAttribute("height", "40");
  });

  it("gives each instance its own gradient ids", () => {
    const { container } = render(
      <>
        <TuneMark />
        <TuneMark />
      </>,
    );
    const ids = [...container.querySelectorAll("[id]")].map((el) => el.id);
    expect(ids).toHaveLength(4);
    expect(new Set(ids).size).toBe(4);
    for (const el of container.querySelectorAll("rect[fill^='url(']")) {
      const ref = /url\(#(.+)\)/.exec(el.getAttribute("fill") ?? "")?.[1];
      expect(container.querySelector(`[id="${ref}"]`)).not.toBeNull();
    }
  });
});
