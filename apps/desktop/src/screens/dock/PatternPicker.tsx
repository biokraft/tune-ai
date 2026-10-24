import * as Popover from "@radix-ui/react-popover";
import { cn } from "@tune/ui";
import { useState } from "react";
import type { SongView } from "../../client/protocol";

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

/** Which of the track's patterns the section plays: the pattern list, plus "None". */
export function PatternPicker({ song, track, section, onPick }: PatternPickerProps) {
  const [open, setOpen] = useState(false);
  const current = song.sections[section]?.play[track];
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
          className={cn(
            "inline-flex h-[26px] flex-none items-center gap-1.5 rounded-rs border border-line bg-ink/[.04] px-2",
            "font-mono text-[11.5px] hover:bg-ink/[.09]",
            current ? "text-ink" : "text-ink3",
            focusRing,
          )}
        >
          {current ?? "—"}
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
            .filter((p) => p.track === track)
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
