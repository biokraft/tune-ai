import type { ReactNode } from "react";
import { cn } from "../lib/utils";
import type { ParamSpecView } from "../lib/param";
import { Knob, type Highlight } from "./Knob";

export type FxPanelProps = {
  title: string;
  params: ParamSpecView[];
  /** Dims the knobs and shows this line, e.g. "No track sends here yet". */
  inactiveNote?: string | null;
  /** Live while dragging (commit false), written on release or a key step (commit true). */
  onParam(id: string, value: number, commit: boolean): void;
  highlight?(id: string): Highlight;
  automated?(id: string): boolean;
  onHelp?(spec: ParamSpecView | null): void;
  /** CSS colour for the knobs' arcs (the track colour). */
  color?: string;
  /** The mode slot on the right of the header, e.g. a distortion mode switch. */
  header?: ReactNode;
  /** Knob size per param; "sm" when omitted. */
  sizeOf?(id: string): "md" | "sm";
};

/**
 * A device card: a titled row of small knobs, one per param. The knobs stay live when the module
 * is dimmed, since a drag is how a bus first gets its signal.
 */
export function FxPanel({
  title,
  params,
  inactiveNote = null,
  onParam,
  highlight,
  automated,
  onHelp,
  color,
  header,
  sizeOf,
}: FxPanelProps) {
  const inactive = inactiveNote !== null && inactiveNote !== "";
  return (
    <div
      role="group"
      aria-label={title}
      data-inactive={inactive ? "true" : undefined}
      className="flex shrink-0 flex-col rounded-rs border border-line bg-glass2"
    >
      <div className="flex items-center gap-2 border-b border-line px-2.5 py-2">
        <span className="text-xs font-bold leading-none text-ink">{title}</span>
        {inactive && <span className="text-[11px] leading-none text-ink3">{inactiveNote}</span>}
        {header && <div className="ml-auto">{header}</div>}
      </div>
      <div
        className={cn(
          "flex flex-nowrap items-start gap-1 p-2.5 transition-opacity",
          inactive && "opacity-50",
        )}
      >
        {params.map((p) => (
          <div key={p.id} onPointerEnter={() => onHelp?.(p)} onPointerLeave={() => onHelp?.(null)}>
            <Knob
              spec={p}
              size={sizeOf?.(p.id) ?? "sm"}
              color={color}
              value={p.value}
              highlight={highlight?.(p.id) ?? null}
              automated={automated?.(p.id) ?? false}
              onHelp={onHelp}
              onChange={(v) => onParam(p.id, v, false)}
              onCommit={(v) => onParam(p.id, v, true)}
            />
          </div>
        ))}
      </div>
    </div>
  );
}
