import { cn } from "../lib/utils";
import { nextHit, type DrumHit } from "../lib/notes";

export type DrumRow = {
  pattern: string;
  label: string;
  hits: DrumHit[];
  highlighted?: Set<number>;
  /** CSS colour of the row's hits; defaults to ink. */
  color?: string;
  /** The docked voice: full strength, while the other rows dim until hovered. */
  emphasis?: boolean;
};

export type DrumGridProps = {
  rows: DrumRow[];
  /** Pattern-relative index of the sounding step. */
  playingStep: number | null;
  onSet: (pattern: string, index: number, hit: DrumHit) => void;
};

const DEFAULT_COLOR = "rgb(var(--ink))";

export function DrumGrid({ rows, playingStep, onSet }: DrumGridProps) {
  return (
    <div className="flex flex-col gap-1.5">
      {rows.map((row) => {
        const color = row.color ?? DEFAULT_COLOR;
        return (
          <div
            key={row.pattern}
            role="group"
            aria-label={row.label}
            data-emphasis={row.emphasis ? "true" : undefined}
            className={cn(
              "flex items-center gap-3 transition-opacity",
              !row.emphasis && "opacity-[.55] hover:opacity-100 focus-within:opacity-100",
            )}
          >
            <span
              className="w-16 shrink-0 truncate text-[10.5px] font-bold uppercase leading-none tracking-[.1em] text-ink2"
              style={row.emphasis ? { color } : undefined}
            >
              {row.label}
            </span>
            <div className="flex gap-1">
              {row.hits.map((hit, i) => {
                const highlighted = row.highlighted?.has(i) ?? false;
                const playing = playingStep === i;
                return (
                  <button
                    key={i}
                    type="button"
                    aria-label={`${row.label} step ${i + 1}: ${hit}`}
                    data-hit={hit}
                    data-playing={playing}
                    data-highlight={highlighted ? "claude" : undefined}
                    onClick={() => onSet(row.pattern, i, nextHit(hit))}
                    className={cn(
                      "relative h-7 w-7 rounded-rs border transition-none",
                      hit === "rest" ? "border-line bg-ink/[.04]" : "border-transparent",
                      i % 4 === 0 && "ml-1.5",
                      playing && "outline outline-2 -outline-offset-1 outline-play",
                      highlighted && "ring-2 ring-agent",
                      "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2",
                      "focus-visible:outline-play",
                    )}
                  >
                    {hit !== "rest" && (
                      <span
                        data-fill
                        aria-hidden="true"
                        className="absolute inset-0 rounded-[inherit]"
                        style={{
                          backgroundColor: color,
                          opacity: hit === "accent" ? 1 : 0.55,
                        }}
                      />
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}
