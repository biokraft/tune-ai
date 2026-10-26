import { describe, expect, it } from "vitest";
import { goldenState } from "../client/fakeClient";
import { describeChange } from "./describeChange";

const song = goldenState.song!;
const change = (path: string, old: unknown, next: unknown) =>
  describeChange({ path, old, new: next } as never, song);

describe("describeChange on the mixer", () => {
  it("names a send knob with its unit", () => {
    expect(change("tracks.kick.send.delay", -60, -18)).toBe("kick Delay -60 → -18 dB");
    expect(change("tracks.acid.send.delay", -60, -18)).toBe("acid Delay -60 → -18 dB");
    expect(change("tracks.acid.duck.depth", 0, 6)).toBe("acid Duck 0 → 6 dB");
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
  it("has no sentence for a bare mix path, which the studio never sends", () => {
    expect(change("mix", null, {})).toBe("mix");
  });
});

describe("describeChange on clips", () => {
  it("describes a clip step change by its `clips.` path", () => {
    expect(change("clips.k4.steps", "x... x...", "x... X...")).toBe("k4 pattern (1 step)");
    expect(change("clips.a1.steps", null, "C2 C3")).toBe("Pattern a1 added");
  });
});

describe("describeChange on v2 songs", () => {
  it("says what a track plays in a section changed when it is not one clip", () => {
    const placements = [{ at: 1, clip: "a1", bars: 1 }];
    expect(change("sections[1].play.acid", "a1", placements)).toBe("Changed what acid plays in b");
    expect(change("sections[0].play.kick", placements, null)).toBe("Changed what kick plays in a");
    // One clip on both sides still reads as before.
    expect(change("sections[1].play.acid", "a1", "a2")).toBe("b acid a1 → a2");
  });
  it("says the devices on a track changed, not that it was removed", () => {
    const devices = { instrument: "acid303", fx: ["dist"] };
    expect(change("tracks.acid", devices, { ...devices, fx: ["dist", "eq"] })).toBe(
      "Changed the devices on acid",
    );
    expect(change("tracks.acid", null, devices)).toBe("Track acid added");
    expect(change("tracks.acid", devices, null)).toBe("Track acid removed");
  });
});
