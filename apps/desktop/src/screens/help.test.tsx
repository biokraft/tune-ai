import { act, render, screen } from "@testing-library/react";
import { goldenState } from "../client/fakeClient";
import { HelpLine, HelpProvider, useHelp } from "./help";

const cutoff = goldenState.song!.tracks[1]!.params[2]!;

function Driver({ spec, note }: { spec?: typeof cutoff | null; note?: string | null }) {
  const help = useHelp();
  return (
    <>
      <button onClick={() => help.show(spec ?? null)}>show</button>
      <button onClick={() => help.note(note ?? null)}>note</button>
    </>
  );
}

function setup(props: { spec?: typeof cutoff | null; note?: string | null } = {}) {
  render(
    <HelpProvider>
      <Driver {...props} />
      <HelpLine />
    </HelpProvider>,
  );
  const click = (name: string) =>
    act(() => {
      screen.getByRole("button", { name }).click();
    });
  return { click };
}

describe("HelpLine", () => {
  it("says what to do when no knob is in focus", () => {
    setup();
    expect(screen.getByText("Hover or focus a knob to learn what it does.")).toBeInTheDocument();
  });

  it("is a polite live region", () => {
    setup();
    expect(
      screen.getByText("Hover or focus a knob to learn what it does.").closest("[aria-live]"),
    ).toHaveAttribute("aria-live", "polite");
  });

  it("shows the knob's label and help", () => {
    const { click } = setup({ spec: cutoff });
    click("show");
    expect(screen.getByText("Cutoff")).toBeInTheDocument();
    expect(screen.getByText(cutoff.help)).toBeInTheDocument();
  });

  it("a note wins over the help", () => {
    const { click } = setup({ spec: cutoff, note: "Cutoff is automated here." });
    click("show");
    click("note");
    expect(screen.getByText("Cutoff is automated here.")).toBeInTheDocument();
    expect(screen.queryByText(cutoff.help)).toBeNull();
  });

  it("a note fades after a few seconds", () => {
    vi.useFakeTimers();
    try {
      const { click } = setup({ note: "Cutoff is automated here." });
      click("note");
      expect(screen.getByText("Cutoff is automated here.")).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(6100);
      });
      expect(screen.queryByText("Cutoff is automated here.")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
