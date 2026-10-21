import { describe, expect, it } from "vitest";
import { goldenState } from "../client/fakeClient";
import { describeChange } from "./describeChange";

const song = goldenState.song!;
const change = (path: string, old: unknown, next: unknown) =>
  describeChange({ path, old, new: next } as never, song);

describe("describeChange on the mixer", () => {
  it("names a send knob with its unit", () => {
    expect(change("tracks.kick.send.delay", -60, -18)).toBe("kick Delay -60 → -18 dB");
    expect(change("tracks.acid.duck.depth", 0, 6)).toBe("Duck 0 → 6 dB");
  });
  it("names a delay param of the mix", () => {
    expect(change("mix.delay.feedback", 35, 45)).toBe("Delay feedback 35 → 45%");
  });
  it("names a master param of the mix", () => {
    expect(change("mix.master.gain", 0, 3)).toBe("Master gain 0 → 3 dB");
  });
  it("names the master mode and the duck source", () => {
    expect(change("mix.master", "clip", "limit")).toBe("Master mode clip → limit");
    expect(change("mix.duck_source", null, "kick")).toBe("Duck source none → kick");
  });
  it("falls back to the path for an unknown mix path", () => {
    expect(change("mix.nonsense", 1, 2)).toBe("mix nonsense 1 → 2");
  });
});
