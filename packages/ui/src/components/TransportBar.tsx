import { memo, type ReactNode } from "react";
import { formatPosition, formatTime } from "../lib/arrange";
import { IconButton } from "./IconButton";

export type TransportBarProps = {
  playing: boolean;
  bpm: number;
  /** Zero-based bar and beat. */
  bar: number;
  beat: number;
  loopOn: boolean;
  canLoop: boolean;
  canUndo: boolean;
  canRedo: boolean;
  songName: string;
  totalBars: number;
  /** Half-open `[start, end)`, zero-based. */
  loopBars: [number, number] | null;
  /** Name of the section the loop spans exactly, if any. */
  loopName: string | null;
  mixerOpen: boolean;
  onPlay(): void;
  onStop(): void;
  onToggleLoop(): void;
  onUndo(): void;
  onRedo(): void;
  onToggleMixer(): void;
};

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {children}
    </svg>
  );
}

function Readout({
  label,
  value,
  className,
  minWidth,
}: {
  label: string;
  value: string;
  className: string;
  minWidth: number;
}) {
  return (
    <div className="flex flex-col gap-px" style={{ minWidth }}>
      <span className="lbl">{label}</span>
      <span aria-label={label === "Bar · beat" ? "Position" : label} className={className}>
        {value}
      </span>
    </div>
  );
}

const big = "font-mono text-[20px] font-semibold leading-[22px] tabular-nums";

export const TransportBar = memo(function TransportBar(p: TransportBarProps) {
  return (
    <div
      role="toolbar"
      aria-label="Transport"
      className="glass flex h-14 items-center gap-3 rounded-r px-3 text-ink"
    >
      <div className="flex items-center gap-2 pr-1">
        <svg
          viewBox="0 0 24 24"
          width="22"
          height="22"
          aria-hidden="true"
          className="fill-none stroke-acid"
          style={{ strokeWidth: 2.2, strokeLinecap: "round" }}
        >
          <path d="M3 12h2l2-6 3 12 3-9 2 5 2-2h4" />
        </svg>
        <div className="flex flex-col">
          <span className="text-[13px] font-extrabold tracking-[.02em]">{p.songName}</span>
          <span className="text-[10.5px] text-ink2">
            {p.bpm} BPM · {p.totalBars} bars
          </span>
        </div>
      </div>
      <div className="h-7 w-px flex-none bg-line" />
      <IconButton
        label="Loop (L)"
        pressed={p.loopOn}
        disabled={!p.canLoop}
        onClick={p.onToggleLoop}
      >
        <Icon>
          <path d="M17 2l3 3-3 3" />
          <path d="M4 11V9a4 4 0 014-4h12" />
          <path d="M7 22l-3-3 3-3" />
          <path d="M20 13v2a4 4 0 01-4 4H4" />
        </Icon>
      </IconButton>
      <IconButton label="Stop" onClick={p.onStop}>
        <Icon>
          <rect x="6" y="6" width="12" height="12" rx="2" />
        </Icon>
      </IconButton>
      <button
        type="button"
        aria-label={p.playing ? "Pause (space)" : "Play (space)"}
        onClick={p.onPlay}
        className="inline-flex h-10 w-10 flex-none items-center justify-center rounded-full bg-play text-bg shadow-[0_0_0_4px_rgb(var(--play)/.12),0_0_24px_rgb(220_200_255/.55)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true" className="h-[18px] w-[18px] fill-current">
          {p.playing ? (
            <>
              <rect x="6" y="5" width="4" height="14" rx="1" />
              <rect x="14" y="5" width="4" height="14" rx="1" />
            </>
          ) : (
            <path d="M8 5v14l11-7z" />
          )}
        </svg>
      </button>
      <div className="ml-1 flex gap-3">
        <Readout
          label="Bar · beat"
          value={formatPosition(p.bar, p.beat)}
          className={big}
          minWidth={76}
        />
        <Readout
          label="Time"
          value={formatTime(p.bar, p.beat, p.bpm)}
          className="font-mono text-[14px] leading-[22px] tabular-nums text-ink2"
          minWidth={64}
        />
        <Readout label="BPM" value={String(p.bpm)} className={big} minWidth={54} />
      </div>
      {p.loopBars && (
        <>
          <div className="h-7 w-px flex-none bg-line" />
          <div aria-label="Loop range" role="group" className="flex items-center gap-2">
            <span className="lbl">Loop</span>
            <span className="font-mono text-[13px] tabular-nums">
              {p.loopBars[0] + 1} – {p.loopBars[1]}
            </span>
            {p.loopName && <span className="text-[11px] text-ink2">{p.loopName}</span>}
          </div>
        </>
      )}
      <div className="flex-1" />
      <IconButton label="Undo" disabled={!p.canUndo} onClick={p.onUndo}>
        <Icon>
          <path d="M9 14L4 9l5-5" />
          <path d="M4 9h10a6 6 0 010 12h-3" />
        </Icon>
      </IconButton>
      <IconButton label="Redo" disabled={!p.canRedo} onClick={p.onRedo}>
        <Icon>
          <path d="M15 14l5-5-5-5" />
          <path d="M20 9H10a6 6 0 000 12h3" />
        </Icon>
      </IconButton>
      <IconButton label="Mixer (M)" kbd="M" pressed={p.mixerOpen} onClick={p.onToggleMixer}>
        <Icon>
          <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
        </Icon>
        Mixer
      </IconButton>
    </div>
  );
});
