import { fireEvent, render, screen } from "@testing-library/react";
import { goldenState } from "../../client/fakeClient";
import type { Command, SongView } from "../../client/protocol";
import { StepsTab } from "./StepsTab";

const song = goldenState.song! as SongView;

function setup(trackId: string, section: number | null, s: SongView = song) {
  const sent: Command[] = [];
  const fired: Command[] = [];
  render(
    <StepsTab
      song={s}
      track={s.tracks.find((t) => t.id === trackId)!}
      section={section}
      highlights={new Map()}
      fire={(c) => void fired.push(c)}
      send={(c) => {
        sent.push(c);
        return Promise.resolve(null);
      }}
    />,
  );
  return { sent, fired };
}

describe("StepsTab", () => {
  it("shows the 303 sequencer for an acid track and edits accents", () => {
    const { sent } = setup("acid", 1);
    expect(screen.getByRole("group", { name: "303 step sequencer" })).toBeInTheDocument();
    const acc = screen
      .getAllByRole("button", { name: /Step \d+ accent/ })
      .find((b) => !b.hasAttribute("disabled"))!;
    fireEvent.click(acc);
    expect(sent[0]).toMatchObject({ cmd: "set_acid_step", args: { accent: true } });
  });

  it("shows one group per drum track and emphasises the docked one", () => {
    setup("kick", 1);
    expect(screen.getByRole("group", { name: "kick" })).toHaveAttribute("data-emphasis", "true");
    expect(screen.getByRole("group", { name: "hat" })).not.toHaveAttribute("data-emphasis");
    expect(screen.queryByRole("group", { name: "acid" })).toBeNull();
  });

  it("fires set_drum_step from the grid", () => {
    const { fired } = setup("kick", 1);
    fireEvent.click(screen.getByRole("button", { name: "kick step 1: hit" }));
    expect(fired[0]).toMatchObject({ cmd: "set_drum_step" });
  });

  it("says so when the docked track plays nothing in the section", () => {
    setup("acid", 0);
    expect(screen.getByText("acid plays nothing in a. Pick a pattern above.")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "303 step sequencer" })).toBeNull();
  });

  it("says the docked track plays part of a clip in a section that shows none", () => {
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
    setup("acid", 0, loose);
    expect(
      screen.getByText(
        "acid plays part of a1 in a (read only). Pick a pattern above to replace it.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "303 step sequencer" })).toBeNull();
  });

  it("says so when no section is under the playhead", () => {
    setup("kick", null);
    expect(
      screen.getByText("Move the playhead into a section to see its steps."),
    ).toBeInTheDocument();
  });

  it("renders the sequencer in a song without drum tracks", () => {
    const s = structuredClone(song);
    s.tracks = s.tracks.filter((t) => t.id === "acid");
    setup("acid", 1, s);
    expect(screen.getByRole("group", { name: "303 step sequencer" })).toBeInTheDocument();
  });
});
