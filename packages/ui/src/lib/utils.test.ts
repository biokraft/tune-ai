import { describe, expect, it } from "vitest";
import { cn } from "./utils";

describe("cn", () => {
  it("keeps the solid outline beside its width, as Tailwind 3 reads them", () => {
    const ring =
      "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play";
    expect(cn(ring).split(" ")).toEqual(ring.split(" "));
  });

  it("still lets a later class override an earlier one", () => {
    expect(cn("px-1 text-xs", "px-2")).toBe("text-xs px-2");
  });
});
