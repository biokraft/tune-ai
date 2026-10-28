import * as Popover from "@radix-ui/react-popover";
import { cn } from "@tune/ui";
import { useState } from "react";
import type { SongView } from "../../client/protocol";
import { partialClipsIn } from "../../lib/song";

export type PatternPickerProps = {
  song: SongView;
  track: string;
  /** 0-based. */
  section: number;
  /** A pattern id, or null for none (the track rests in the section). */
  onPick(pattern: string | null): void;
};

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play";

const option = cn(
  "rounded-[7px] border border-line px-2 py-1 text-left text-xs text-ink2 hover:bg-ink/5",
  focusRing,
);

/**
 * The label of what the track plays in the section: the clip, "partial: <clip>" or "mixed" when it
 * plays part of a clip there (or from mid-clip) that no one clip names, else "—".
 */
function currentLabel(song: SongView, track: string, section: number): string {
  const current = song.sections[section]?.play[track];
  if (current !== undefined) return current;
  const partial = partialClipsIn(song, section, track);
  if (partial.length === 0) return "—";
  return partial.length === 1 ? `partial: ${partial[0]}` : "mixed";
}

/**
 * Which clip the track plays in the section: the clips its instrument can play, plus "None". A
 * section the track plays only in part shows that, so "None" there visibly removes something.
 */
export function PatternPicker({ song, track, section, onPick }: PatternPickerProps) {
  const [open, setOpen] = useState(false);
  const plays = song.tracks.find((t) => t.id === track)?.plays ?? [];
  const current = song.sections[section]?.play[track];
  const label = currentLabel(song, track, section);
  const partial = current === undefined && label !== "—";
  const pick = (pattern: string | null) => {
    onPick(pattern);
    setOpen(false);
  };
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button
          type="button"
          aria-label={`${track} in section ${section + 1}`}
          title={
            partial
              ? "Plays part of a clip here (read only); picking replaces what lies in the section"
              : undefined
          }
          className={cn(
            "inline-flex h-[26px] flex-none items-center gap-1.5 rounded-rs border border-line bg-ink/[.04] px-2",
            "font-mono text-[11.5px] hover:bg-ink/[.09]",
            current || partial ? "text-ink" : "text-ink3",
            focusRing,
          )}
        >
          {label}
          <svg
            viewBox="0 0 24 24"
            aria-hidden="true"
            className="h-3 w-3 fill-none stroke-current text-ink2 [stroke-linecap:round] [stroke-linejoin:round] [stroke-width:2]"
          >
            <path d="M6 9l6 6 6-6" />
          </svg>
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          sideOffset={4}
          align="start"
          className="glass-solid z-50 flex w-40 flex-col gap-1 rounded-rs p-2 text-ink"
        >
          {song.patterns
            .filter((p) => plays.includes(p.kind))
            .map((p) => (
              <button
                key={p.id}
                type="button"
                aria-current={current === p.id ? "true" : undefined}
                className={cn(
                  option,
                  "font-mono",
                  current === p.id && "border-line2 bg-ink/10 text-ink",
                )}
                onClick={() => pick(p.id)}
              >
                {p.id}
              </button>
            ))}
          <button type="button" className={option} onClick={() => pick(null)}>
            None
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
