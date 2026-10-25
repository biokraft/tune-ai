import { cn } from "../lib/utils";
import { nextHit, type DrumHit } from "../lib/notes";
import type { Highlight } from "./Knob";

export type DrumRow = {
  pattern: string;
  label: string;
  hits: DrumHit[];
  /** Steps changed from outside, by whom: Claude's ringed cyan, the song file's dashed ink2. */
  highlighted?: ReadonlyMap<number, NonNullable<Highlight>>;
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

/** The row head: colour bar and voice name, as the DrumGrid board's `.rh`. */
const HEAD = "flex w-[104px] shrink-0 items-center gap-2";
/** A step cell's slot: cells share the width, with a wider gap before each beat. */
const slot = (i: number) => cn("min-w-[18px] flex-1", i > 0 && i % 4 === 0 && "ml-2");

export function DrumGrid({ rows, playingStep, onSet }: DrumGridProps) {
  const steps = rows.reduce((m, r) => Math.max(m, r.hits.length), 0);
  return (
    <div className="flex w-full flex-col gap-1.5">
      <div aria-hidden="true" className="flex items-center gap-3">
        <span className={HEAD} />
        <div className="flex flex-1 gap-1">
          {Array.from({ length: steps }, (_, i) => (
            <span
              key={i}
              data-step-number
              className={cn(
                slot(i),
                "text-center font-mono text-[10px] leading-4 tabular-nums",
                playingStep === i ? "font-semibold text-play" : "text-ink3",
              )}
            >
              {i + 1}
            </span>
          ))}
        </div>
      </div>
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
            <span className={HEAD} title={`${row.label} · ${row.pattern}`}>
              <span
                aria-hidden="true"
                data-color-bar
                className="h-6 w-1 shrink-0 rounded-sm"
                style={{ backgroundColor: color }}
              />
              <span className="min-w-0 truncate text-[12px] font-bold leading-none text-ink">
                {row.label}
              </span>
            </span>
            <div className="flex flex-1 gap-1">
              {row.hits.map((hit, i) => {
                const highlighted = row.highlighted?.get(i) ?? null;
                const playing = playingStep === i;
                return (
                  <button
                    key={i}
                    type="button"
                    aria-label={`${row.label} step ${i + 1}: ${hit}`}
                    data-hit={hit}
                    data-playing={playing}
                    data-highlight={highlighted ?? undefined}
                    onClick={() => onSet(row.pattern, i, nextHit(hit))}
                    className={cn(
                      slot(i),
                      "relative h-8 rounded-rs border transition-none",
                      hit === "rest" ? "border-line bg-ink/[.04]" : "border-transparent",
                      playing && "outline outline-2 -outline-offset-1 outline-play",
                      highlighted === "claude" && "ring-2 ring-agent",
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
                    {highlighted === "external" && (
                      // Where Claude's ring sits, but dashed: a ring cannot be.
                      <span
                        data-external-mark
                        aria-hidden="true"
                        className="pointer-events-none absolute -inset-[3px] rounded-[11px] border-2 border-dashed border-ink2"
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
