import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { stateEvent } from "../../client/fakeClient";
import type { Command, SongView } from "../../client/protocol";
import type { HighlightKind } from "../../store/studioStore";
import { HelpProvider } from "../help";
import { LIMITER_OFF, MixerTab } from "./MixerTab";

function setup(
  song: SongView = stateEvent().song!,
  highlights = new Map<string, HighlightKind>(),
  automatedIn: number | null = null,
) {
  const sent: Command[] = [];
  const fired: Command[] = [];
  const send = vi.fn((c: Command) => {
    sent.push(c);
    return Promise.resolve(null);
  });
  const fire = vi.fn((c: Command) => void fired.push(c));
  const utils = render(
    <HelpProvider>
      <MixerTab
        song={song}
        highlights={highlights}
        automatedIn={automatedIn}
        send={send}
        fire={fire}
      />
    </HelpProvider>,
  );
  return { sent, fired, ...utils };
}

const delayPanel = () => screen.getByRole("group", { name: "Delay" });

describe("MixerTab", () => {
  it("has one strip region per track", () => {
    const song = stateEvent().song!;
    setup(song);
    for (const t of song.tracks) {
      expect(screen.getByRole("group", { name: `Track ${t.id}` })).toBeInTheDocument();
    }
  });

  it("a strip's mute button fires set_track_mix", () => {
    const song = stateEvent().song!;
    const { fired } = setup(song);
    const strip = screen.getByRole("group", { name: `Track ${song.tracks[0]!.id}` });
    fireEvent.click(within(strip).getByRole("button", { name: "M" }));
    expect(fired).toEqual([
      {
        cmd: "set_track_mix",
        args: { track: song.tracks[0]!.id, gain_db: null, pan: null, mute: true, solo: null },
      },
    ]);
  });

  it("drag sends set_param on mix with commit false", async () => {
    const { sent } = setup();
    const knob = within(delayPanel()).getByRole("slider", { name: "Feedback" });
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 280, pointerId: 1 });
    await waitFor(() => expect(sent.length).toBe(1));
    expect(sent[0]).toEqual({
      cmd: "set_param",
      args: { track: "mix", param: "delay.feedback", value: expect.any(Number), commit: false },
    });
  });

  it("release commits", () => {
    const { sent } = setup();
    const knob = within(delayPanel()).getByRole("slider", { name: "Feedback" });
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 280, pointerId: 1 });
    fireEvent.pointerUp(knob, { clientY: 280, pointerId: 1 });
    expect(sent).toEqual([
      {
        cmd: "set_param",
        args: { track: "mix", param: "delay.feedback", value: expect.any(Number), commit: true },
      },
    ]);
  });

  it("claude change on mix.delay.feedback rings its knob", () => {
    setup(undefined, new Map([["mix.delay.feedback", "claude"]]));
    const ring = (name: string) =>
      within(delayPanel()).getByRole("slider", { name }).getAttribute("data-highlight");
    expect(ring("Feedback")).toBe("claude");
    expect(ring("Time")).toBeNull();
  });

  it("marks a knob automated by a lane on the mix", () => {
    const song = stateEvent((s) => {
      s.song!.sections[0]!.lanes.push({
        track: "mix",
        param: "delay.feedback",
        points: [[0, 0.5]],
      });
    }).song!;
    setup(song, new Map(), 0);
    const knob = within(delayPanel()).getByRole("slider", { name: "Feedback" });
    expect(within(knob).getByTestId("knob-lane")).toBeInTheDocument();
  });

  it("shows the master mode read-only, with no control to switch it", () => {
    setup();
    const master = screen.getByRole("group", { name: "Master" });
    expect(within(master).getByText("Mode")).toBeInTheDocument();
    expect(within(master).getByText("Clip")).toBeInTheDocument();
    expect(within(master).queryByRole("radio")).toBeNull();
    expect(within(master).queryByRole("radiogroup")).toBeNull();
    expect(within(master).queryByRole("button")).toBeNull();
  });

  it("shows the limit mode and a missing duck source", () => {
    const song = stateEvent((s) => {
      s.song!.mix.master = "limit";
      s.song!.mix.duck_source = null;
    }).song!;
    setup(song);
    expect(within(screen.getByRole("group", { name: "Master" })).getByText("Limit")).toBeVisible();
    expect(screen.getByText("Ducking: no kick to follow")).toBeInTheDocument();
  });

  it("explains the read-only mode in clip mode, and says limiter in limit mode", () => {
    const { unmount } = setup();
    expect(
      screen.getByText(
        'Master: soft clipper (set mix.master = "limit" in the song for the limiter)',
      ),
    ).toBeInTheDocument();
    unmount();
    setup(stateEvent((s) => (s.song!.mix.master = "limit")).song!);
    expect(screen.getByText("Master: limiter")).toBeInTheDocument();
    expect(screen.queryByText(/soft clipper/)).toBeNull();
  });

  it("shows the duck source", () => {
    setup();
    expect(screen.getByText("Ducking follows: kick")).toBeInTheDocument();
  });

  it("dims the limiter's knobs in clip mode, with a hint", () => {
    setup();
    const master = screen.getByRole("group", { name: "Master" });
    expect(master).not.toHaveAttribute("data-inactive");
    expect(within(master).getByRole("slider", { name: "Master gain" })).toBeInTheDocument();
    const limiter = screen.getByRole("group", { name: "Limiter" });
    expect(limiter).toHaveAttribute("data-inactive", "true");
    expect(within(limiter).getByText(LIMITER_OFF)).toBeInTheDocument();
    for (const name of ["Ceiling", "Release"]) {
      expect(within(limiter).getByRole("slider", { name })).toBeInTheDocument();
    }
  });

  it("the limiter's knobs are live in limit mode", () => {
    setup(stateEvent((s) => (s.song!.mix.master = "limit")).song!);
    const limiter = screen.getByRole("group", { name: "Limiter" });
    expect(limiter).not.toHaveAttribute("data-inactive");
    expect(screen.queryByText(LIMITER_OFF)).toBeNull();
  });

  it("a master knob sends set_param on mix", async () => {
    const { sent } = setup();
    const knob = screen.getByRole("slider", { name: "Master gain" });
    fireEvent.pointerDown(knob, { clientY: 300, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 280, pointerId: 1 });
    await waitFor(() => expect(sent.length).toBe(1));
    expect(sent[0]).toMatchObject({
      cmd: "set_param",
      args: { track: "mix", param: "master.gain", commit: false },
    });
  });

  it("explains an unused bus", () => {
    setup();
    expect(
      screen.getAllByText("sends start after the first release", { exact: false }),
    ).toHaveLength(2);
  });

  it("drops the unused-bus note once a track sends", () => {
    const song = stateEvent((s) => {
      s.song!.mix.delay_active = true;
    }).song!;
    setup(song);
    expect(delayPanel()).not.toHaveAttribute("data-inactive");
    expect(
      screen.getAllByText("sends start after the first release", { exact: false }),
    ).toHaveLength(1);
  });
});
