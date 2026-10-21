import type { ParamSpecView } from "@tune/ui";
import { sentenceCase } from "../lib/song";

export type HelpStripProps = {
  /** The hovered or focused knob. */
  spec: ParamSpecView | null;
  /** A short-lived message about the last knob action; shown in place of the help. */
  note?: string | null;
};

/** One line under the knobs explaining the knob in hand, so help never covers a knob. */
export function HelpStrip({ spec, note }: HelpStripProps) {
  return (
    <div
      aria-live="polite"
      className="flex min-h-[2.75rem] items-baseline gap-3 border-t border-black/30 px-1 pt-3 text-silkscreen"
    >
      {note ? (
        <p className="font-value text-sm text-slide">{note}</p>
      ) : spec ? (
        <>
          <span className="shrink-0 font-label text-base">{sentenceCase(spec.label)}</span>
          <p className="max-w-[72ch] font-value text-sm text-silkscreen/85">{spec.help}</p>
        </>
      ) : (
        <p className="font-value text-sm text-silkscreen/60">
          Hover or focus a knob to learn what it does.
        </p>
      )}
    </div>
  );
}
