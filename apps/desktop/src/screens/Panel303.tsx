import {
  AcidStepSequencer,
  cn,
  Knob,
  type AcidStep,
  type AcidStepSequencerProps,
  type ParamSpecView,
} from "@tune/ui";
import { memo, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { Command, ParamView, SongView, TrackView } from "../client/protocol";
import { highlightedSteps, patternIn, sentenceCase, stepInPattern } from "../lib/song";
import type { HighlightKind } from "../store/studioStore";
import { usePlayheadStep } from "../store/useStudio";
import { HelpStrip } from "./HelpStrip";
import { useParamDrag } from "./useParamDrag";

export type Panel303Props = {
  song: SongView;
  /** The section whose pattern the sequencer shows. */
  section: number | null;
  /**
   * The section at the playhead (or cursor): the studio writes nothing for a knob released on a
   * param automated there, so its knobs carry the lane badge.
   */
  automatedIn: number | null;
  highlights: Map<string, HighlightKind>;
  send(cmd: Command): Promise<unknown>;
};

const BIG = ["tuning", "cutoff", "resonance", "env_mod", "decay", "accent"];
const SMALL = ["waveform", "slide_time", "volume"];
const DIST = ["drive", "mix", "output"];
/** How long a knob note (e.g. "automated here") stays in the help strip. */
const NOTE_MS = 6000;

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen";

const byShortId = (params: ParamView[], short: string) =>
  params.find((p) => p.id.slice(p.id.indexOf(".") + 1) === short);

/** The 303 faceplate: the first acid303 track's knobs, distortion, help strip and sequencer. */
/** Memoised: playhead ticks reach only the sequencer's LEDs, never the knobs. */
export const Panel303 = memo(function Panel303({
  song,
  section,
  automatedIn,
  highlights,
  send,
}: Panel303Props) {
  const acids = song.tracks.filter((t) => t.instrument === "acid303");
  const [chosen, setChosen] = useState<string | null>(null);
  const track = acids.find((t) => t.id === chosen) ?? acids[0];
  if (!track) {
    return (
      <section aria-label="303" className="faceplate rounded-lg p-6">
        <p className="font-value text-sm text-silkscreen/70">
          This song has no 303 track. Ask Claude to add one with instrument = &quot;acid303&quot;.
        </p>
      </section>
    );
  }
  return (
    <TrackPanel
      key={track.id}
      song={song}
      track={track}
      acids={acids}
      onChoose={setChosen}
      section={section}
      automatedIn={automatedIn}
      highlights={highlights}
      send={send}
    />
  );
});

type TrackPanelProps = Omit<Panel303Props, "song"> & {
  song: SongView;
  track: TrackView;
  acids: TrackView[];
  onChoose(id: string): void;
};

function TrackPanel({
  song,
  track,
  acids,
  onChoose,
  section,
  automatedIn,
  highlights,
  send,
}: TrackPanelProps) {
  const [hovered, setHovered] = useState<ParamSpecView | null>(null);
  const [focused, setFocused] = useState<ParamSpecView | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const noteTimer = useRef<ReturnType<typeof setTimeout>>();

  const dist = track.fx.find((f) => f.kind === "distortion");
  const allParams = useMemo(
    () => [...track.params, ...(dist?.params ?? [])],
    [track.params, dist?.params],
  );

  useEffect(() => () => clearTimeout(noteTimer.current), []);

  const showNote = (text: string) => {
    clearTimeout(noteTimer.current);
    setNote(text);
    noteTimer.current = setTimeout(() => setNote(null), NOTE_MS);
  };

  const drag = useParamDrag(track.id, send, allParams, (id) => {
    const p = allParams.find((x) => x.id === id);
    showNote(
      `${sentenceCase(p?.label ?? id)} follows its automation here — you heard your change, but it wasn't saved. Ask Claude to change the automation.`,
    );
  });

  const shownSection = section === null ? undefined : song.sections[section];
  const soundingSection = automatedIn === null ? undefined : song.sections[automatedIn];
  const automated = (id: string) =>
    soundingSection?.lanes.some((l) => l.track === track.id && l.param === id) ?? false;

  const knob = (p: ParamView | undefined, size: "lg" | "md" | "sm") => {
    if (!p) return null;
    const spec = { ...p, label: sentenceCase(p.label) };
    const h = highlights.get(`tracks.${track.id}.${p.id}`) ?? null;
    return (
      <div
        key={p.id}
        onPointerEnter={() => setHovered(spec)}
        onPointerLeave={() => setHovered((cur) => (cur?.id === p.id ? null : cur))}
      >
        <Knob
          spec={spec}
          value={drag.valueOf(p)}
          size={size}
          highlight={h}
          automated={automated(p.id)}
          onHelp={(s) => setFocused(s)}
          onChange={(v) => drag.change(p.id, v)}
          onCommit={(v) => drag.commit(p.id, v)}
        />
      </div>
    );
  };

  const pattern = patternIn(song, section, track.id);
  const acid = pattern?.kind === "acid" ? pattern : null;

  return (
    <section
      aria-label={`303 ${track.id}`}
      className="faceplate relative flex flex-col gap-4 rounded-lg px-6 pb-5 pt-4"
    >
      <Screws />
      <header className="flex items-baseline gap-4">
        <h2 className="font-label text-2xl leading-none text-silkscreen">{track.id}</h2>
        <span className="font-value text-xs text-silkscreen/60">Bass line</span>
        {acids.length > 1 && (
          <div role="tablist" aria-label="303 tracks" className="ml-auto flex gap-1">
            {acids.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={t.id === track.id}
                onClick={() => onChoose(t.id)}
                className={cn(
                  "rounded-sm border border-border px-3 py-1 font-label text-sm text-silkscreen",
                  t.id === track.id && "border-silkscreen bg-black/20",
                  focusRing,
                )}
              >
                {t.id}
              </button>
            ))}
          </div>
        )}
      </header>

      <div className="flex items-start gap-3">
        {BIG.map((id) => knob(byShortId(track.params, id), "lg"))}
      </div>

      <div className="flex items-end gap-6">
        <div className="flex items-start gap-2">
          {SMALL.map((id) => knob(byShortId(track.params, id), "sm"))}
        </div>
        {dist && (
          <fieldset className="relative flex items-end gap-3 rounded-md border border-silkscreen/30 px-3 pb-2 pt-3">
            <legend className="px-1 font-label text-sm leading-none text-silkscreen/80">
              Distortion
            </legend>
            {DIST.map((id) => knob(byShortId(dist.params, id), "sm"))}
            <ModeSwitch
              modes={dist.modes}
              mode={dist.mode}
              highlight={highlights.get(`tracks.${track.id}.dist.mode`) ?? null}
              onPick={(mode) =>
                void send({ cmd: "set_fx_mode", args: { track: track.id, mode } }).catch(() => {})
              }
            />
          </fieldset>
        )}
      </div>

      <HelpStrip spec={hovered ?? focused} note={note} />

      <div className="flex flex-col gap-2 rounded-md bg-chassis/70 p-3 shadow-inner">
        {acid ? (
          <>
            <p className="font-label text-sm leading-none text-silkscreen/80">
              Pattern {acid.id} in {shownSection?.name}
            </p>
            <div className="overflow-x-auto pb-1">
              <LiveSequencer
                song={song}
                section={section}
                patternId={acid.id}
                steps={acid.steps}
                highlighted={highlightedSteps(highlights, acid.id)}
                onChange={(index: number, s: AcidStep) =>
                  void send({
                    cmd: "set_acid_step",
                    args: {
                      pattern: acid.id,
                      index,
                      note: s.note,
                      accent: s.accent,
                      slide: s.slide,
                    },
                  }).catch(() => {})
                }
              />
            </div>
          </>
        ) : (
          <p className="py-4 font-value text-sm text-silkscreen/70">
            The 303 rests in this section.
          </p>
        )}
      </div>
    </section>
  );
}

type ModeSwitchProps = {
  modes: string[];
  mode: string;
  highlight: HighlightKind | null;
  onPick(mode: string): void;
};

/** A three-position hardware switch: one radio per distortion mode, arrows move between them. */
function ModeSwitch({ modes, mode, highlight, onPick }: ModeSwitchProps) {
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const i = modes.indexOf(mode);
    const step =
      e.key === "ArrowDown" || e.key === "ArrowRight"
        ? 1
        : e.key === "ArrowUp" || e.key === "ArrowLeft"
          ? -1
          : 0;
    if (step === 0) return;
    e.preventDefault();
    const next = modes[(i + step + modes.length) % modes.length]!;
    onPick(next);
    e.currentTarget.querySelector<HTMLButtonElement>(`[data-mode="${next}"]`)?.focus();
  };
  return (
    <div
      role="radiogroup"
      aria-label="Distortion mode"
      data-highlight={highlight ?? undefined}
      onKeyDown={onKeyDown}
      className={cn(
        "flex flex-col gap-1 self-center rounded-sm p-1",
        highlight === "claude" && "ring-2 ring-claude",
        highlight === "external" && "outline-dashed outline-2 outline-claude",
      )}
    >
      {modes.map((m) => {
        const on = m === mode;
        return (
          <button
            key={m}
            type="button"
            role="radio"
            aria-checked={on}
            tabIndex={on ? 0 : -1}
            data-mode={m}
            onClick={() => !on && onPick(m)}
            className={cn("flex items-center gap-2 rounded-sm px-1 text-left", focusRing)}
          >
            <span
              aria-hidden="true"
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                on ? "bg-led shadow-[0_0_6px_rgb(var(--led))]" : "bg-black/50",
              )}
            />
            <span className="font-label text-sm leading-none text-silkscreen">
              {sentenceCase(m)}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** Four panel screws: the faceplate is a plate bolted onto the chassis. */
function Screws() {
  const pos = ["left-2 top-2", "right-2 top-2", "left-2 bottom-2", "right-2 bottom-2"];
  return (
    <>
      {pos.map((p) => (
        <span key={p} aria-hidden="true" className={cn("screw absolute", p)} />
      ))}
    </>
  );
}

type LiveSequencerProps = Omit<AcidStepSequencerProps, "playingStep"> & {
  song: SongView;
  section: number | null;
};

/** The step sequencer, lighting the sounding step as the playhead moves. */
function LiveSequencer({ song, section, ...rest }: LiveSequencerProps) {
  const step = usePlayheadStep();
  return (
    <AcidStepSequencer
      {...rest}
      playingStep={stepInPattern(song, section, step, rest.steps.length)}
    />
  );
}
