import * as Popover from "@radix-ui/react-popover";
import { useRef, useState, type KeyboardEvent } from "react";
import { cn } from "../lib/utils";
import { noteName, PITCH_CLASSES, type AcidStep } from "../lib/notes";

export type { AcidStep } from "../lib/notes";

export type AcidStepSequencerProps = {
  /** Identifies the pattern shown; the last-note memory resets when it changes. */
  patternId: string;
  steps: AcidStep[];
  /** Pattern-relative index of the sounding step. */
  playingStep: number | null;
  highlighted?: Set<number>;
  onChange: (index: number, step: AcidStep) => void;
};

const DEFAULT_NOTE = 36;
const MIN_OCT = -1;
const MAX_OCT = 9;

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen";

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
  const key =
    "rounded-sm border border-border bg-faceplate px-2 py-1 font-value text-sm text-silkscreen";
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
        <span
          role="status"
          aria-label="Octave"
          className="font-value text-sm tabular-nums text-silkscreen"
        >
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
                pc.includes("#") && "bg-chassis",
                step.note === midi && "border-led",
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

  const toggle = cn(
    "h-6 w-full rounded-sm border border-border font-label text-xs leading-none text-silkscreen",
    "disabled:opacity-30",
    focusRing,
  );

  return (
    <div ref={root} role="group" aria-label="303 step sequencer" className="flex gap-1">
      {steps.map((step, i) => {
        const playing = playingStep === i;
        const lit = highlighted?.has(i) ?? false;
        const n = i + 1;
        const rest = step.note === null;
        return (
          <div
            key={i}
            data-step={i}
            data-playing={playing}
            data-highlight={lit ? "claude" : undefined}
            className={cn(
              "flex w-12 flex-col items-stretch gap-1 rounded-sm p-1",
              i % 4 === 0 && i > 0 && "ml-2",
              lit && "ring-2 ring-claude",
            )}
          >
            <span
              aria-hidden="true"
              className={cn("mx-auto h-2 w-2 rounded-full", playing ? "bg-led" : "bg-border")}
            />
            <Popover.Root open={open === i} onOpenChange={(o) => setOpen(o ? i : null)}>
              <Popover.Trigger asChild>
                <button
                  type="button"
                  data-note-button={i}
                  aria-label={`Step ${n} note`}
                  onKeyDown={(e) => onNoteKey(i, step, e)}
                  className={cn(
                    "h-9 rounded-sm border border-border bg-chassis font-value text-sm tabular-nums",
                    rest ? "text-silkscreen/50" : "text-silkscreen",
                    focusRing,
                  )}
                >
                  {rest ? "—" : noteName(step.note!)}
                </button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content
                  sideOffset={6}
                  className="z-50 w-56 rounded-md border border-border bg-faceplate p-3 shadow-lg"
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
            <button
              type="button"
              aria-label={`Step ${n} accent`}
              aria-pressed={step.accent}
              disabled={rest}
              onClick={() => onChange(i, { ...step, accent: !step.accent })}
              className={cn(toggle, step.accent && "border-led bg-led text-chassis")}
            >
              Acc
            </button>
            <button
              type="button"
              aria-label={`Step ${n} slide`}
              aria-pressed={step.slide}
              disabled={rest}
              onClick={() => onChange(i, { ...step, slide: !step.slide })}
              className={cn(toggle, step.slide && "border-slide bg-slide text-chassis")}
            >
              Sld
            </button>
          </div>
        );
      })}
    </div>
  );
}
