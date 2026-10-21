import { render, screen } from "@testing-library/react";
import { goldenState } from "../client/fakeClient";
import { HelpStrip } from "./HelpStrip";

const cutoff = goldenState.song!.tracks[1]!.params[2]!;

describe("HelpStrip", () => {
  it("says what to do when no knob is in focus", () => {
    render(<HelpStrip spec={null} />);
    expect(screen.getByText("Hover or focus a knob to learn what it does.")).toBeInTheDocument();
  });

  it("shows the knob's label and help", () => {
    render(<HelpStrip spec={cutoff} />);
    expect(screen.getByText("Cutoff")).toBeInTheDocument();
    expect(screen.getByText(cutoff.help)).toBeInTheDocument();
  });

  it("a note wins over the help", () => {
    render(<HelpStrip spec={cutoff} note="Cutoff is automated here." />);
    expect(screen.getByText("Cutoff is automated here.")).toBeInTheDocument();
  });
});
