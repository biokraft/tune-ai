import { cn } from "../lib/utils";
import { nextHit, type DrumHit } from "../lib/notes";

export type DrumRow = {
  pattern: string;
  label: string;
  hits: DrumHit[];
  highlighted?: Set<number>;
};

export type DrumGridProps = {
  rows: DrumRow[];
  /** Pattern-relative index of the sounding step. */
  playingStep: number | null;
  onSet: (pattern: string, index: number, hit: DrumHit) => void;
};

const FILL: Record<DrumHit, string> = {
  rest: "border-border/60 bg-transparent opacity-50",
  hit: "border-led bg-transparent",
  accent: "border-led bg-led",
};

export function DrumGrid({ rows, playingStep, onSet }: DrumGridProps) {
  return (
    <div className="flex flex-col gap-1.5">
      {rows.map((row) => (
        <div
          key={row.pattern}
          role="group"
          aria-label={row.label}
          className="flex items-center gap-3"
        >
          <span className="w-16 shrink-0 font-label text-sm leading-none text-silkscreen">
            {row.label}
          </span>
          <div className="flex gap-1">
            {row.hits.map((hit, i) => {
              const highlighted = row.highlighted?.has(i) ?? false;
              return (
                <button
                  key={i}
                  type="button"
                  aria-label={`${row.label} step ${i + 1}: ${hit}`}
                  data-hit={hit}
                  data-playing={playingStep === i}
                  data-highlight={highlighted ? "claude" : undefined}
                  onClick={() => onSet(row.pattern, i, nextHit(hit))}
                  className={cn(
                    "h-7 w-7 rounded-sm border-2 transition-none",
                    FILL[hit],
                    i % 4 === 0 && "ml-1.5",
                    playingStep === i && "brightness-150 ring-1 ring-silkscreen",
                    highlighted && "ring-2 ring-claude",
                    "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2",
                    "focus-visible:outline-silkscreen",
                  )}
                />
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
