import { describe, expect, it } from "vitest";
import { goldenState } from "../client/fakeClient";
import { describeChange, describeTarget } from "./describeChange";

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
  it("describes a note clip's notes and length", () => {
    expect(change("clips.lead.notes", [], [{ start: 0 }])).toBe("Changed the notes of lead");
    expect(change("clips.lead.bars", 1, 2)).toBe("Changed the length of lead");
  });
});

describe("describeChange on v2 songs", () => {
  it("names the upgrade of the song file", () => {
    expect(change("song.version", 1, 2)).toBe(
      "Upgraded the song file to format version 2 (comments kept)",
    );
  });
  it("names going back to an older format as a restore, not an upgrade", () => {
    expect(change("song.version", 2, 1)).toBe("Restored the song file to format version 1");
  });
  it("names a distortion mode change by its device id", () => {
    expect(change("tracks.acid.dist.mode", "soft", "hard")).toBe("acid distortion soft → hard");
    const dist = song.tracks.find((t) => t.id === "acid")!.fx.find((f) => f.id === "dist")!;
    const two = {
      ...song,
      tracks: song.tracks.map((t) =>
        t.id === "acid" ? { ...t, fx: [...t.fx, { ...dist, id: "d2", params: [] }] } : t,
      ),
    };
    expect(
      describeChange({ path: "tracks.acid.d2.mode", old: "soft", new: "hard" } as never, two),
    ).toBe("acid distortion d2 soft → hard");
    // A mode on no distortion keeps the plain form.
    expect(change("tracks.acid.d2.mode", "soft", "hard")).toBe("acid d2.mode soft → hard");
  });
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

describe("describeTarget", () => {
  const k = (target: string, by: "human" | "claude" | "external" | null = null) =>
    ({ target, item: null, reason: "changed_later", by, message: null }) as never;

  it("labels a leaf as describeChange does, without the span", () => {
    expect(describeTarget(k("tracks.acid.acid303.cutoff", "human"), song)).toBe("Cutoff (by you)");
    expect(describeTarget(k("tracks.kick.send.delay"), song)).toBe("kick Delay");
    expect(describeTarget(k("song.bpm"), song)).toBe("Tempo");
    expect(describeTarget(k("tracks.hat.pan"), song)).toBe("hat pan");
    expect(describeTarget(k("tracks.hat.mute"), song)).toBe("hat mute");
  });
  it("names a track and its devices", () => {
    expect(describeTarget(k("tracks.acid"), song)).toBe("track acid");
    expect(describeTarget(k("tracks.acid.instrument"), song)).toBe("acid's instrument");
    expect(describeTarget(k("tracks.acid.fx"), song)).toBe("acid's effects");
    expect(describeTarget(k("tracks.acid.play"), song)).toBe("a clip on acid");
  });
  it("names a clip, a lane and the sections", () => {
    expect(describeTarget(k("clips.a1"), song)).toBe("clip a1");
    expect(describeTarget(k("automation.acid.acid303.cutoff"), song)).toBe(
      "acid303.cutoff automation on acid",
    );
    expect(describeTarget(k("automation.mix.reverb.size"), song)).toBe(
      "mix reverb.size automation",
    );
    expect(describeTarget(k("sections"), song)).toBe("the sections");
  });
  it("says who changed it later", () => {
    expect(describeTarget(k("sections", "human"), song)).toBe("the sections (by you)");
    expect(describeTarget(k("sections", "claude"), song)).toBe("the sections (another run)");
    expect(describeTarget(k("sections", "external"), song)).toBe("the sections");
    expect(describeTarget(k("sections", null), song)).toBe("the sections");
  });
});
