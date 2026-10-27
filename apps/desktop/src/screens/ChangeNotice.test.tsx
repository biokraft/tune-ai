import { fireEvent, render, screen, within } from "@testing-library/react";
import { goldenState } from "../client/fakeClient";
import type { ExternalChange, RunUndo } from "../client/protocol";
import { ChangeNotice } from "./ChangeNotice";
import type { Notice } from "../store/studioStore";

const song = goldenState.song!;

function notice(patch: Partial<Notice>): Notice {
  return {
    source: "claude",
    run: null,
    changes: [],
    replaced: false,
    merged: [],
    offered: [],
    errors: [],
    ...patch,
  };
}

function setup(
  n: Notice | null,
  offered = n?.offered ?? [],
  canUndo = true,
  runUndo: RunUndo | null = null,
) {
  const onUndo = vi.fn();
  const onUndoRun = vi.fn();
  const onResolve = vi.fn();
  const onDismiss = vi.fn();
  render(
    <ChangeNotice
      notice={n}
      offered={offered}
      canUndo={canUndo}
      runUndo={runUndo}
      song={song}
      onUndo={onUndo}
      onUndoRun={onUndoRun}
      onResolve={onResolve}
      onDismiss={onDismiss}
    />,
  );
  return { onUndo, onUndoRun, onResolve, onDismiss };
}

const CUTOFF = { path: "tracks.acid.acid303.cutoff", old: 500, new: 820 };
const runUndo = (patch: Partial<RunUndo>): RunUndo => ({
  run: "r1",
  title: null,
  reverted: [],
  kept: [],
  already: [],
  ...patch,
});
const reverted = (n: number) =>
  Array.from({ length: n }, (_, i) => ({ target: `tracks.acid.acid303.p${i}`, item: null }));

describe("ChangeNotice runs", () => {
  it("shows the run title", () => {
    setup(notice({ changes: [CUTOFF], run: { id: "r1", title: "open the filter" } }));
    const card = screen.getByRole("status", { name: "Claude's change" });
    expect(within(card).getByText("Claude")).toBeInTheDocument();
    expect(within(card).getByText("· open the filter", { exact: false })).toHaveClass("text-ink2");
  });

  it("Undo this run sends undo_run", () => {
    const { onUndoRun, onUndo } = setup(
      notice({ changes: [CUTOFF], run: { id: "r1", title: null } }),
    );
    expect(screen.getByRole("button", { name: "Undo Claude's change" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Undo this run" }));
    expect(onUndoRun).toHaveBeenCalledWith("r1");
    expect(onUndo).not.toHaveBeenCalled();
  });

  it("offers no Undo this run without a run", () => {
    setup(notice({ changes: [CUTOFF] }));
    expect(screen.queryByRole("button", { name: "Undo this run" })).toBeNull();
  });

  it("reports what the run undo kept", () => {
    setup(
      notice({ changes: [CUTOFF], run: { id: "r1", title: null } }),
      [],
      true,
      runUndo({
        reverted: reverted(3),
        kept: [
          {
            target: "tracks.acid.acid303.cutoff",
            item: null,
            reason: "changed_later",
            by: "human",
            message: null,
          },
        ],
      }),
    );
    expect(
      screen.getByText("Undid this run: 3 changes. Kept 1 change made since: Cutoff (by you)."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Undo this run" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Undo Claude's change" })).toBeNull();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
  });

  it("reports a run undo that kept nothing", () => {
    setup(
      notice({ changes: [CUTOFF], run: { id: "r1", title: null } }),
      [],
      true,
      runUndo({ reverted: reverted(1) }),
    );
    expect(screen.getByText("Undid this run: 1 change.")).toBeInTheDocument();
  });

  it("says when the run was already undone", () => {
    setup(notice({ changes: [CUTOFF], run: { id: "r1", title: null } }), [], true, runUndo({}));
    expect(screen.getByText("Nothing to undo: this run is already undone.")).toBeInTheDocument();
  });
});

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
          { path: "clips.k4.steps", old: "x... x...", new: "x... X..." },
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

  it("is a status card named for Claude's change, titled Claude", () => {
    setup(notice({ changes: [{ path: "tracks.acid.acid303.resonance", old: 70, new: 90 }] }));
    const card = screen.getByRole("status", { name: "Claude's change" });
    expect(within(card).getByText("Claude")).toBeInTheDocument();
    expect(card).toHaveAttribute("data-source", "claude");
  });

  it("is a status card named for a song file change, titled Song file", () => {
    setup(notice({ source: "external", changes: [{ path: "song.bpm", old: 120, new: 125 }] }));
    const card = screen.getByRole("status", { name: "Song file change" });
    expect(within(card).getByText("Song file")).toBeInTheDocument();
    expect(card).toHaveAttribute("data-source", "file");
  });

  it("names a replaced file and a bare re-apply offer as song file changes", () => {
    const { unmount } = render(
      <ChangeNotice
        notice={notice({ replaced: true })}
        offered={[]}
        canUndo
        runUndo={null}
        song={song}
        onUndo={() => {}}
        onUndoRun={() => {}}
        onResolve={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByRole("status", { name: "Song file change" })).toBeInTheDocument();
    unmount();
    setup(null, [{ path: "tracks.acid.acid303.cutoff", old: 820, new: 500 }]);
    expect(screen.getByRole("status", { name: "Song file change" })).toBeInTheDocument();
  });

  it("Dismiss calls onDismiss", () => {
    const { onDismiss } = setup(notice({ changes: [{ path: "song.bpm", old: 120, new: 125 }] }));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("renders nothing when there is nothing to say", () => {
    const { container } = render(
      <ChangeNotice
        notice={null}
        offered={[]}
        canUndo
        runUndo={null}
        song={song}
        onUndo={() => {}}
        onUndoRun={() => {}}
        onResolve={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
