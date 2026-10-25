import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { goldenState } from "../../client/fakeClient";
import type { Command } from "../../client/protocol";
import type { HighlightKind } from "../../store/studioStore";
import { HelpLine, HelpProvider } from "../help";
import { BIG, SMALL, SoundTab } from "./SoundTab";

const song = goldenState.song!;
const sentence = (id: string) => {
  const p = song.tracks[1]!.params.find((x) => x.id.endsWith(`.${id}`))!;
  return p.label.charAt(0).toUpperCase() + p.label.slice(1).toLowerCase();
};

function setup(
  trackId = "acid",
  opts: {
    highlights?: Map<string, HighlightKind>;
    automatedIn?: number | null;
    result?: unknown;
  } = {},
) {
  const sent: Command[] = [];
  const fired: Command[] = [];
  const send = vi.fn((c: Command) => {
    sent.push(c);
    return Promise.resolve(opts.result ?? null);
  });
  const fire = vi.fn((c: Command) => void fired.push(c));
  render(
    <HelpProvider>
      <SoundTab
        song={song}
        track={song.tracks.find((t) => t.id === trackId)!}
        automatedIn={opts.automatedIn ?? 1}
        highlights={opts.highlights ?? new Map()}
        send={send}
        fire={fire}
      />
      <HelpLine />
    </HelpProvider>,
  );
  return { sent, fired };
}

const helpLine = () => document.querySelector<HTMLElement>('[aria-live="polite"]')!;

describe("SoundTab", () => {
  it("shows a slider for every 303 knob, the mode switch and the mixer strip", () => {
    setup();
    const names = screen.getAllByRole("slider").map((s) => s.getAttribute("aria-label"));
    for (const id of [...BIG, ...SMALL]) expect(names).toContain(sentence(id));
    expect(screen.getByRole("radiogroup", { name: "Distortion mode" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Mixer strip" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Acid 303" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Distortion" })).toBeInTheDocument();
  });

  it("orders the 303 knobs big first, then small", () => {
    setup();
    const names = within(screen.getByRole("group", { name: "Acid 303" }))
      .getAllByRole("slider")
      .map((s) => s.getAttribute("aria-label"));
    expect(names).toEqual([...BIG, ...SMALL].map(sentence));
  });

  it("picking hard fires set_fx_mode", () => {
    const { fired } = setup();
    fireEvent.click(screen.getByRole("radio", { name: "Hard" }));
    expect(fired).toEqual([{ cmd: "set_fx_mode", args: { track: "acid", mode: "hard" } }]);
  });

  it("a kick shows one slider per param and no mode switch", () => {
    setup("kick");
    const params = within(screen.getByRole("group", { name: "Kick" })).getAllByRole("slider");
    expect(params).toHaveLength(song.tracks[0]!.params.length);
    expect(screen.queryByRole("radiogroup")).toBeNull();
  });

  it("a Claude highlight shows the AI badge on that knob", () => {
    setup("acid", { highlights: new Map([["tracks.acid.acid303.cutoff", "claude"]]) });
    const cutoff = screen.getByRole("slider", { name: "Cutoff" });
    expect(within(cutoff).getByTestId("knob-highlight")).toBeInTheDocument();
    expect(cutoff.textContent).toContain("AI");
    expect(
      within(screen.getByRole("slider", { name: "Decay" })).queryByTestId("knob-highlight"),
    ).toBeNull();
  });

  it("a keyboard step commits at once", () => {
    const { sent } = setup();
    fireEvent.keyDown(screen.getByRole("slider", { name: "Accent" }), { key: "ArrowUp" });
    expect(sent).toEqual([
      {
        cmd: "set_param",
        args: { track: "acid", param: "acid303.accent", value: expect.any(Number), commit: true },
      },
    ]);
  });

  it("mixer strip M fires set_track_mix", () => {
    const { fired } = setup();
    fireEvent.click(
      within(screen.getByRole("region", { name: "Mixer strip" })).getByRole("button", {
        name: "M",
      }),
    );
    expect(fired).toEqual([
      {
        cmd: "set_track_mix",
        args: { track: "acid", gain_db: null, pan: null, mute: true, solo: null },
      },
    ]);
  });

  it("shows the help of the hovered and the focused knob", () => {
    setup();
    expect(screen.getByText("Hover or focus a knob to learn what it does.")).toBeInTheDocument();
    const res = screen.getByRole("slider", { name: "Resonance" });
    fireEvent.pointerEnter(res.parentElement!);
    expect(screen.getByText(/A whistling peak at the cutoff/)).toBeInTheDocument();
    fireEvent.pointerLeave(res.parentElement!);
    act(() => screen.getByRole("slider", { name: "Decay" }).focus());
    expect(screen.getByText(/How fast the filter closes/)).toBeInTheDocument();
  });

  it("says when a release on an automated knob was not written", async () => {
    setup("acid", { result: { automated: true } });
    fireEvent.keyDown(screen.getByRole("slider", { name: "Cutoff" }), { key: "ArrowUp" });
    expect(
      await screen.findByText(
        "Cutoff follows its automation here — you heard your change, but it wasn't saved. Ask Claude to change the automation.",
      ),
    ).toBeInTheDocument();
  });

  it("marks knobs automated where a release would land", () => {
    const cutoff = () => screen.getByRole("slider", { name: "Cutoff" });
    setup("acid", { automatedIn: 1 });
    expect(within(cutoff()).getByTestId("knob-lane")).toBeInTheDocument();
  });

  it("keeps device cards from shrinking so the chain scrolls sideways", () => {
    setup();
    for (const name of ["Acid 303", "Distortion"]) {
      expect(screen.getByRole("group", { name })).toHaveClass("shrink-0");
    }
    expect(screen.getByRole("region", { name: "Mixer strip" })).toHaveClass("flex-none");
  });

  it("hovering a mixer strip knob shows its help", () => {
    setup();
    const strip = within(screen.getByRole("region", { name: "Mixer strip" }));
    fireEvent.pointerEnter(strip.getByRole("slider", { name: "Delay" }).parentElement!);
    expect(helpLine()).toHaveTextContent("Delay");
    fireEvent.pointerLeave(strip.getByRole("slider", { name: "Delay" }).parentElement!);
    fireEvent.pointerEnter(strip.getByRole("slider", { name: "Pan" }).parentElement!);
    expect(helpLine()).toHaveTextContent("Pan");
  });
});
