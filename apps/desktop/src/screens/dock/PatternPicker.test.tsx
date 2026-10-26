import { fireEvent, render, screen } from "@testing-library/react";
import { goldenState } from "../../client/fakeClient";
import type { SongView } from "../../client/protocol";
import { PatternPicker } from "./PatternPicker";

const golden = goldenState.song! as SongView;

/** The golden song reduced to two clips: `k4` (drum) and `a1` (acid). */
const song: SongView = {
  ...golden,
  patterns: golden.patterns.filter((p) => p.id === "k4" || p.id === "a1"),
};

describe("PatternPicker", () => {
  it("lists only the clips the track can play", () => {
    render(<PatternPicker song={song} track="kick" section={0} onPick={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "kick in section 1" }));
    expect(screen.getByRole("button", { name: "k4" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "None" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "a1" })).toBeNull();
  });

  it("offers an acid clip to the acid track", () => {
    render(<PatternPicker song={song} track="acid" section={0} onPick={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "acid in section 1" }));
    expect(screen.getByRole("button", { name: "a1" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "k4" })).toBeNull();
  });
});
