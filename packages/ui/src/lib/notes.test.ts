import { describe, expect, it } from "vitest";
import { nextHit, noteName } from "./notes";

describe("noteName", () => {
  it("uses sharps and scientific pitch", () => {
    expect(noteName(36)).toBe("C2");
    // the brief says C#3, but the Rust Display gives C#4 (and C#3 is 49)
    expect(noteName(61)).toBe("C#4");
    expect(noteName(49)).toBe("C#3");
    expect(noteName(0)).toBe("C-1");
    expect(noteName(127)).toBe("G9");
  });
});

describe("nextHit", () => {
  it("cycles rest, hit, accent", () => {
    expect(nextHit("rest")).toBe("hit");
    expect(nextHit("hit")).toBe("accent");
    expect(nextHit("accent")).toBe("rest");
  });
});
