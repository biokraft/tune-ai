import { memo } from "react";
import { cn } from "../lib/utils";

export type MuteSoloProps = {
  track: string;
  mute: boolean;
  solo: boolean;
  onMute(v: boolean): void;
  onSolo(v: boolean): void;
};

const ms = cn(
  "h-5 w-[22px] cursor-pointer rounded-[5px] border p-0 font-sans text-[10px] font-bold leading-none",
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play",
);

const off = "border-line bg-transparent text-ink2 hover:bg-ink/[.08] hover:text-ink";

/** Mute and solo toggles; named "M" and "S" (titles Mute and Solo), as TrackStrip's are. */
export const MuteSolo = memo(function MuteSolo({
  track,
  mute,
  solo,
  onMute,
  onSolo,
}: MuteSoloProps) {
  return (
    <span className="inline-flex gap-1" data-track={track}>
      <button
        type="button"
        title="Mute"
        aria-pressed={mute}
        className={cn(ms, mute ? "border-transparent bg-ink/80 text-bg" : off)}
        onClick={() => onMute(!mute)}
      >
        M
      </button>
      <button
        type="button"
        title="Solo"
        aria-pressed={solo}
        className={cn(ms, solo ? "border-transparent bg-clap text-bg" : off)}
        onClick={() => onSolo(!solo)}
      >
        S
      </button>
    </span>
  );
});
