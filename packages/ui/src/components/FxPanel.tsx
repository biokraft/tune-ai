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
};

/**
 * A rack module: a titled row of small knobs, one per param. The knobs stay live when the module
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
}: FxPanelProps) {
  const inactive = inactiveNote !== null && inactiveNote !== "";
  return (
    <div
      role="group"
      aria-label={title}
      data-inactive={inactive ? "true" : undefined}
      className="flex flex-col gap-1 rounded-md border border-silkscreen/30 px-3 pb-2 pt-2"
    >
      <div className="flex items-baseline justify-between gap-3">
        <span className="font-label text-sm leading-none text-silkscreen/80">{title}</span>
        {inactive && (
          <span className="font-value text-xs leading-none text-silkscreen/60">{inactiveNote}</span>
        )}
      </div>
      <div className={cn("flex items-start gap-1 transition-opacity", inactive && "opacity-50")}>
        {params.map((p) => (
          <Knob
            key={p.id}
            spec={p}
            size="sm"
            value={p.value}
            highlight={highlight?.(p.id) ?? null}
            automated={automated?.(p.id) ?? false}
            onHelp={onHelp}
            onChange={(v) => onParam(p.id, v, false)}
            onCommit={(v) => onParam(p.id, v, true)}
          />
        ))}
      </div>
    </div>
  );
}
