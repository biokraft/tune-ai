import { fireEvent, render, screen } from "@testing-library/react";
import { goldenState } from "../client/fakeClient";
import type { ExternalChange } from "../client/protocol";
import { ChangeNotice } from "./ChangeNotice";

const song = goldenState.song!;

function notice(patch: Partial<ExternalChange>): ExternalChange {
  return {
    source: "claude",
    changes: [],
    replaced: false,
    merged: [],
    offered: [],
    errors: [],
    ...patch,
  };
}

function setup(n: ExternalChange | null, offered = n?.offered ?? [], canUndo = true) {
  const onUndo = vi.fn();
  const onResolve = vi.fn();
  const onDismiss = vi.fn();
  render(
    <ChangeNotice
      notice={n}
      offered={offered}
      canUndo={canUndo}
      song={song}
      onUndo={onUndo}
      onResolve={onResolve}
      onDismiss={onDismiss}
    />,
  );
  return { onUndo, onResolve, onDismiss };
}

describe("ChangeNotice", () => {
  it("lists what Claude changed, with Undo", () => {
    const { onUndo } = setup(
      notice({
        changes: [
          { path: "tracks.acid.acid303.cutoff", old: 500, new: 820 },
          { path: "tracks.acid.acid303.resonance", old: 70, new: 90 },
        ],
      }),
    );
    expect(
      screen.getByText("Claude changed 2 things: Cutoff 500 → 820 Hz, Resonance 70 → 90%"),
    ).toBeInTheDocument();
    const undo = screen.getByRole("button", { name: "Undo Claude's change" });
    expect(undo).toHaveTextContent("Undo");
    fireEvent.click(undo);
    expect(onUndo).toHaveBeenCalledTimes(1);
  });

  it("drops Undo once it would revert something else", () => {
    setup(
      notice({ changes: [{ path: "tracks.acid.acid303.resonance", old: 70, new: 90 }] }),
      [],
      false,
    );
    expect(screen.getByText("Claude changed 1 thing: Resonance 70 → 90%")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Undo Claude's change" })).toBeNull();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
  });

  it("names other kinds of change in plain words", () => {
    setup(
      notice({
        source: "external",
        changes: [
          { path: "sections[1].bars", old: 2, new: 4 },
          { path: "tracks.hat.pan", old: 0.3, new: 0.4 },
          { path: "patterns.k4.steps", old: "x... x...", new: "x... X..." },
          { path: "song.bpm", old: 120, new: 125 },
        ],
      }),
    );
    expect(
      screen.getByText(
        "The song file changed 4 things: b length 2 → 4 bars, hat pan 0.3 → 0.4, k4 pattern (1 step), and 1 more",
      ),
    ).toBeInTheDocument();
  });

  it("offers to re-apply edits a replaced file dropped", () => {
    const { onResolve } = setup(
      notice({
        replaced: true,
        offered: [{ path: "tracks.acid.acid303.cutoff", old: 820, new: 500 }],
      }),
    );
    expect(screen.getByText("The song file was replaced. Re-apply 1 unsaved edit?")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(onResolve).toHaveBeenCalledWith(false);
    fireEvent.click(screen.getByRole("button", { name: "Re-apply" }));
    expect(onResolve).toHaveBeenLastCalledWith(true);
  });

  it("offers overridden edits even without a notice", () => {
    setup(null, [{ path: "tracks.acid.acid303.cutoff", old: 820, new: 500 }]);
    expect(
      screen.getByText("A change on disk overrode 1 unsaved edit. Re-apply it?"),
    ).toBeInTheDocument();
  });

  it("renders nothing when there is nothing to say", () => {
    const { container } = render(
      <ChangeNotice
        notice={null}
        offered={[]}
        canUndo
        song={song}
        onUndo={() => {}}
        onResolve={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
