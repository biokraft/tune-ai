import * as Popover from "@radix-ui/react-popover";
import { useRef, useState, type KeyboardEvent } from "react";
import { cn } from "../lib/utils";
import type { Highlight } from "./Knob";
import { noteName, PITCH_CLASSES, type AcidStep } from "../lib/notes";

export type { AcidStep } from "../lib/notes";

export type AcidStepSequencerProps = {
  /** Identifies the pattern shown; the last-note memory resets when it changes. */
  patternId: string;
  steps: AcidStep[];
  /** Pattern-relative index of the sounding step. */
  playingStep: number | null;
  /** Steps changed from outside, by whom: Claude's ringed cyan, the song file's dashed ink2. */
  highlighted?: ReadonlyMap<number, NonNullable<Highlight>>;
  onChange: (index: number, step: AcidStep) => void;
  /** CSS colour of lit step lights and flags; defaults to the acid token. */
  color?: string;
};

const DEFAULT_NOTE = 36;
const MIN_OCT = -1;
const MAX_OCT = 9;

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play";

const octaveOf = (midi: number) => Math.floor(midi / 12) - 1;

/** Rests carry no flags: the Rust pattern format rejects a modifier on a rest. */
const REST: AcidStep = { note: null, accent: false, slide: false };

type PianoProps = {
  step: AcidStep;
  lastNote: number;
  onPick: (step: AcidStep) => void;
  onClose: () => void;
};

function Piano({ step, lastNote, onPick, onClose }: PianoProps) {
  const [octave, setOctave] = useState(octaveOf(step.note ?? lastNote));
  const key = "rounded-rs border border-line2 bg-glass2 px-2 py-1 font-mono text-sm text-ink";
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <button
          type="button"
          aria-label="Octave down"
          disabled={octave <= MIN_OCT}
          className={cn(key, focusRing, "disabled:opacity-40")}
          onClick={() => setOctave((o) => Math.max(MIN_OCT, o - 1))}
        >
          −
        </button>
        <span role="status" aria-label="Octave" className="font-mono text-sm tabular-nums text-ink">
          {octave}
        </span>
        <button
          type="button"
          aria-label="Octave up"
          disabled={octave >= MAX_OCT}
          className={cn(key, focusRing, "disabled:opacity-40")}
          onClick={() => setOctave((o) => Math.min(MAX_OCT, o + 1))}
        >
          +
        </button>
      </div>
      <div className="grid grid-cols-6 gap-1">
        {PITCH_CLASSES.map((pc, i) => {
          const midi = (octave + 1) * 12 + i;
          return (
            <button
              key={pc}
              type="button"
              aria-label={pc}
              disabled={midi > 127}
              aria-current={step.note === midi ? "true" : undefined}
              className={cn(
                key,
                focusRing,
                pc.includes("#") && "bg-bg/60",
                step.note === midi && "border-play",
                "disabled:opacity-40",
              )}
              onClick={() => {
                onPick({ ...step, note: midi });
                onClose();
              }}
            >
              {pc}
            </button>
          );
        })}
      </div>
      <button
        type="button"
        className={cn(key, focusRing)}
        onClick={() => {
          onPick(REST);
          onClose();
        }}
      >
        Rest
      </button>
    </div>
  );
}

export function AcidStepSequencer({
  patternId,
  steps,
  playingStep,
  highlighted,
  onChange,
  color = "rgb(var(--acid))",
}: AcidStepSequencerProps) {
  const root = useRef<HTMLDivElement>(null);
  const lastNotes = useRef<Map<number, number>>(new Map());
  const [open, setOpen] = useState<number | null>(null);

  const seenPattern = useRef(patternId);
  if (seenPattern.current !== patternId) {
    seenPattern.current = patternId;
    lastNotes.current.clear();
  }
  steps.forEach((s, i) => {
    if (s.note !== null) lastNotes.current.set(i, s.note);
  });
  const lastNote = (i: number) => lastNotes.current.get(i) ?? DEFAULT_NOTE;

  const focusStep = (i: number) => {
    root.current?.querySelector<HTMLButtonElement>(`[data-note-button="${i}"]`)?.focus();
  };

  const onNoteKey = (i: number, step: AcidStep, e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const toggleRest = () =>
      onChange(i, step.note === null ? { ...step, note: lastNote(i) } : REST);
    switch (e.key) {
      case "ArrowUp":
      case "ArrowDown": {
        e.preventDefault();
        const delta = (e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 12 : 1);
        const next = Math.min(127, Math.max(0, (step.note ?? lastNote(i)) + delta));
        onChange(i, { ...step, note: next });
        break;
      }
      case "ArrowLeft":
        e.preventDefault();
        focusStep(Math.max(0, i - 1));
        break;
      case "ArrowRight":
        e.preventDefault();
        focusStep(Math.min(steps.length - 1, i + 1));
        break;
      case "a":
        if (step.note !== null) onChange(i, { ...step, accent: !step.accent });
        break;
      case "s":
        if (step.note !== null) onChange(i, { ...step, slide: !step.slide });
        break;
      case "r":
      case "Delete":
      case "Backspace":
        e.preventDefault();
        toggleRest();
        break;
      default:
        return;
    }
    // Handled here; keep global shortcuts (e.g. delete the selected section) from also firing.
    e.stopPropagation();
  };

  const pip = cn(
    "h-6 w-full rounded-[6px] border text-[10px] font-bold uppercase leading-none tracking-[.06em]",
    "disabled:opacity-30",
    focusRing,
  );

  return (
    <div ref={root} role="group" aria-label="303 step sequencer" className="flex w-full">
      {steps.map((step, i) => {
        const playing = playingStep === i;
        const lit = highlighted?.get(i) ?? null;
        const n = i + 1;
        const rest = step.note === null;
        return (
          <div
            key={i}
            data-step={i}
            data-playing={playing}
            data-highlight={lit ?? undefined}
            className={cn(
              // Columns share the dock's width, as the Seq303 board's `.sc`; past 16 steps on a
              // narrow dock they hold 44 px and the tab scrolls sideways.
              "relative flex min-w-[44px] flex-1 flex-col items-stretch gap-1 border-l px-1 pb-1 pt-2",
              i % 4 === 0 ? "border-line2" : "border-line",
              lit === "claude" && "outline outline-2 -outline-offset-2 outline-agent",
              lit === "external" && "outline-dashed outline-2 -outline-offset-2 outline-ink2",
            )}
          >
            {playing && (
              <span
                data-playhead-bar
                aria-hidden="true"
                className="absolute inset-x-0 top-0 h-[3px] bg-play shadow-[0_0_8px_rgb(var(--play)/.6)]"
              />
            )}
            <span
              aria-hidden="true"
              data-step-number
              className={cn(
                "text-center font-mono text-[10px] leading-4 tabular-nums",
                playing ? "font-semibold text-play" : "text-ink3",
              )}
            >
              {n}
            </span>
            <Popover.Root open={open === i} onOpenChange={(o) => setOpen(o ? i : null)}>
              <Popover.Trigger asChild>
                <button
                  type="button"
                  data-note-button={i}
                  aria-label={`Step ${n} note`}
                  onKeyDown={(e) => onNoteKey(i, step, e)}
                  className={cn(
                    "h-10 rounded-rs bg-ink/[.06] font-mono text-[15px] font-semibold tabular-nums",
                    rest ? "text-ink3" : "text-ink",
                    focusRing,
                  )}
                >
                  {rest ? "—" : noteName(step.note!)}
                </button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content
                  sideOffset={6}
                  className="glass-solid z-50 w-56 rounded-rs p-3 text-ink"
                >
                  <Piano
                    step={step}
                    lastNote={lastNote(i)}
                    onPick={(s) => onChange(i, s)}
                    onClose={() => setOpen(null)}
                  />
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>
            <span
              aria-hidden="true"
              data-step-light={rest ? "off" : "on"}
              className="mx-auto h-2 w-2 rounded-full bg-ink/[.12]"
              style={rest ? undefined : { backgroundColor: color, boxShadow: `0 0 8px ${color}` }}
            />
            <button
              type="button"
              aria-label={`Step ${n} accent`}
              aria-pressed={step.accent}
              disabled={rest}
              onClick={() => onChange(i, { ...step, accent: !step.accent })}
              className={cn(
                pip,
                step.accent ? "border-transparent text-bg" : "border-line text-ink2",
              )}
              style={step.accent ? { backgroundColor: color } : undefined}
            >
              Acc
            </button>
            <button
              type="button"
              aria-label={`Step ${n} slide`}
              aria-pressed={step.slide}
              disabled={rest}
              onClick={() => onChange(i, { ...step, slide: !step.slide })}
              className={cn(pip, step.slide ? "text-ink" : "border-line text-ink2")}
              // The board's slide: a tint of the track colour with a strong edge, so it reads
              // apart from the solid accent.
              style={
                step.slide
                  ? {
                      backgroundColor: `color-mix(in srgb, ${color} 18%, transparent)`,
                      borderColor: `color-mix(in srgb, ${color} 70%, transparent)`,
                    }
                  : undefined
              }
            >
              Slide
            </button>
          </div>
        );
      })}
    </div>
  );
}
