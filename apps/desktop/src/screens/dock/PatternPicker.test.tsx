import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
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

  it("shows a section the track plays in part as partial, not silent", () => {
    const onPick = vi.fn();
    // The acid plays a1 from bar 2 of the clip over bars 2–3: section `a` shows no clip for it.
    const loose: SongView = {
      ...song,
      tracks: song.tracks.map((t) =>
        t.id === "acid"
          ? {
              ...t,
              placements: [{ start_bar: 1, bars: 2, clip: "a1", offset: 1, in_section: null }],
            }
          : t,
      ),
      sections: song.sections.map((s) => {
        const { acid: _, ...play } = s.play;
        return { ...s, play };
      }),
    };
    render(<PatternPicker song={loose} track="acid" section={0} onPick={onPick} />);
    const trigger = screen.getByRole("button", { name: "acid in section 1" });
    expect(trigger).toHaveTextContent("partial: a1");
    expect(trigger).not.toHaveTextContent("—");
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(onPick).toHaveBeenCalledWith(null);
  });

  it("shows mixed for parts of two clips, and — where the track is silent", () => {
    const two: SongView = {
      ...song,
      tracks: song.tracks.map((t) =>
        t.id === "kick"
          ? {
              ...t,
              placements: [
                { start_bar: 0, bars: 1, clip: "k4", offset: 0, in_section: null },
                { start_bar: 1, bars: 1, clip: "k8", offset: 0, in_section: null },
              ],
            }
          : t,
      ),
      sections: song.sections.map((s) => {
        const { kick: _, ...play } = s.play;
        return { ...s, play };
      }),
    };
    const { unmount } = render(
      <PatternPicker song={two} track="kick" section={0} onPick={() => {}} />,
    );
    expect(screen.getByRole("button", { name: "kick in section 1" })).toHaveTextContent("mixed");
    unmount();
    render(<PatternPicker song={two} track="kick" section={1} onPick={() => {}} />);
    expect(screen.getByRole("button", { name: "kick in section 2" })).toHaveTextContent("—");
  });
});
