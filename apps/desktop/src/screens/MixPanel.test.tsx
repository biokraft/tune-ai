import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { stateEvent } from "../client/fakeClient";
import type { Command, SongView } from "../client/protocol";
import type { HighlightKind } from "../store/studioStore";
import { LIMITER_OFF, MixPanel } from "./MixPanel";

function setup(
  song: SongView = stateEvent().song!,
  highlights = new Map<string, HighlightKind>(),
  automatedIn: number | null = null,
) {
  const sent: Command[] = [];
  const send = vi.fn((c: Command) => {
    sent.push(c);
    return Promise.resolve(null);
  });
  const utils = render(
    <MixPanel song={song} highlights={highlights} automatedIn={automatedIn} send={send} />,
  );
  return { sent, ...utils };
}

const delayPanel = () => screen.getByRole("group", { name: "Delay" });

describe("MixPanel", () => {
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

  it("shows master mode and duck source", () => {
    setup();
    expect(
      screen.getByText(
        'Master: soft clipper (set mix.master = "limit" in the song for the limiter)',
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Ducking follows: kick")).toBeInTheDocument();
  });

  it("shows the limiter and a missing duck source", () => {
    const song = stateEvent((s) => {
      s.song!.mix.master = "limit";
      s.song!.mix.duck_source = null;
    }).song!;
    setup(song);
    expect(screen.getByText("Master: limiter")).toBeInTheDocument();
    expect(screen.getByText("Ducking: no kick to follow")).toBeInTheDocument();
  });

  it("dims the limiter's knobs in clip mode, with a hint", () => {
    setup();
    const limiter = screen.getByRole("group", { name: "Limiter" });
    expect(limiter).toHaveAttribute("data-inactive", "true");
    expect(within(limiter).getByText(LIMITER_OFF)).toBeInTheDocument();
    for (const name of ["Ceiling", "Release"]) {
      expect(within(limiter).getByRole("slider", { name })).toBeInTheDocument();
    }
    const master = screen.getByRole("group", { name: "Master" });
    expect(master).not.toHaveAttribute("data-inactive");
    expect(within(master).getByRole("slider", { name: "Master gain" })).toBeInTheDocument();
  });

  it("the limiter's knobs are live in limit mode", () => {
    setup(stateEvent((s) => (s.song!.mix.master = "limit")).song!);
    const limiter = screen.getByRole("group", { name: "Limiter" });
    expect(limiter).not.toHaveAttribute("data-inactive");
    expect(screen.queryByText(LIMITER_OFF)).toBeNull();
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
