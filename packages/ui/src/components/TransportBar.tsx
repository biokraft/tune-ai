import type { ReactNode } from "react";
import { cn } from "../lib/utils";
import { formatPosition } from "../lib/arrange";

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
  notice?: ReactNode;
  onPlay(): void;
  onStop(): void;
  onToggleLoop(): void;
  onUndo(): void;
  onRedo(): void;
};

const btn = cn(
  "h-8 min-w-8 rounded-sm border border-border bg-faceplate px-2 font-label text-sm text-silkscreen",
  "disabled:opacity-40",
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen",
);

export function TransportBar(p: TransportBarProps) {
  return (
    <div role="toolbar" aria-label="Transport" className="flex items-center gap-3 text-silkscreen">
      <button
        type="button"
        aria-label="Play"
        aria-pressed={p.playing}
        className={cn(btn, p.playing && "border-led text-led")}
        onClick={p.onPlay}
      >
        ▶
      </button>
      <button type="button" aria-label="Stop" className={btn} onClick={p.onStop}>
        ■
      </button>
      <span className="font-value text-sm tabular-nums">{p.bpm} BPM</span>
      <span aria-label="Position" className="font-value text-sm tabular-nums">
        {formatPosition(p.bar, p.beat)}
      </span>
      <button
        type="button"
        aria-label="Loop selected section"
        aria-pressed={p.loopOn}
        disabled={!p.canLoop}
        className={cn(btn, p.loopOn && "border-slide text-slide")}
        onClick={p.onToggleLoop}
      >
        Loop
      </button>
      <span className="font-label text-base">{p.songName}</span>
      <button
        type="button"
        aria-label="Undo"
        disabled={!p.canUndo}
        className={btn}
        onClick={p.onUndo}
      >
        ↶
      </button>
      <button
        type="button"
        aria-label="Redo"
        disabled={!p.canRedo}
        className={btn}
        onClick={p.onRedo}
      >
        ↷
      </button>
      {p.notice && (
        <div role="status" className="font-value text-sm text-claude">
          {p.notice}
        </div>
      )}
    </div>
  );
}
