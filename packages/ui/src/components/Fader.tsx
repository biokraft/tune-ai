import { memo, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { cn } from "../lib/utils";

export type FaderProps = {
  label: string;
  value: number;
  min: number;
  max: number;
  defaultValue: number;
  unit: string;
  /** CSS colour of the filled part of the track. */
  color?: string;
  /** Live, on every pointer move. */
  onChange(v: number): void;
  /** On release, keyboard step and double-click reset. */
  onCommit(v: number): void;
  /** Called with a short description on focus and with null on blur (drives a help strip). */
  onHelp?(text: string | null): void;
};

/** Pointer travel (px) for the full range; Shift makes it ten times finer. */
const TRAVEL_PX = 160;
const FINE = 0.1;
const STEP = 0.5;
const STEP_PAGE = 3;

type Drag = { startY: number; startValue: number; shift: boolean; value: number; moved: boolean };

/** A vertical channel fader. */
export const Fader = memo(function Fader({
  label,
  value,
  min,
  max,
  defaultValue,
  unit,
  color = "rgb(var(--ink))",
  onChange,
  onCommit,
  onHelp,
}: FaderProps) {
  const [dragging, setDragging] = useState(false);
  const drag = useRef<Drag | null>(null);
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const shown = clamp(value);
  const norm = max > min ? (shown - min) / (max - min) : 0;
  const text = `${shown.toFixed(1)} ${unit}`.trim();

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    drag.current = {
      startY: e.clientY,
      startValue: shown,
      shift: e.shiftKey,
      value: shown,
      moved: false,
    };
    setDragging(true);
    try {
      e.currentTarget.setPointerCapture?.(e.pointerId);
    } catch {
      /* ignore */
    }
    e.currentTarget.focus();
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    if (e.shiftKey !== d.shift) {
      // Re-anchor so pressing or releasing Shift mid-drag does not make the value jump.
      d.startY = e.clientY;
      d.startValue = d.value;
      d.shift = e.shiftKey;
    }
    const scale = d.shift ? FINE : 1;
    d.value = clamp(d.startValue + ((d.startY - e.clientY) / TRAVEL_PX) * (max - min) * scale);
    d.moved = true;
    onChange(d.value);
  };

  const endDrag = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    setDragging(false);
    try {
      e.currentTarget.releasePointerCapture?.(e.pointerId);
    } catch {
      /* ignore */
    }
    // A click without movement touched nothing; committing would add a write and an undo step.
    if (d.moved) onCommit(d.value);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    let delta = 0;
    switch (e.key) {
      case "ArrowUp":
        delta = STEP;
        break;
      case "ArrowDown":
        delta = -STEP;
        break;
      case "PageUp":
        delta = STEP_PAGE;
        break;
      case "PageDown":
        delta = -STEP_PAGE;
        break;
      default:
        return;
    }
    e.preventDefault();
    onCommit(clamp(shown + delta));
  };

  return (
    <div className="flex w-8 select-none flex-col items-center gap-1.5">
      <div
        role="slider"
        tabIndex={0}
        aria-label={label}
        aria-orientation="vertical"
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={shown}
        aria-valuetext={text}
        className={cn(
          "relative w-8 cursor-ns-resize touch-none rounded-rs",
          "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play",
        )}
        style={{ height: TRAVEL_PX }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={() => onCommit(clamp(defaultValue))}
        onKeyDown={onKeyDown}
        onFocus={() => onHelp?.(`${label} ${text}`)}
        onBlur={() => onHelp?.(null)}
      >
        <span
          aria-hidden="true"
          className="absolute bottom-0 left-1/2 top-0 w-1.5 -translate-x-1/2 rounded-full bg-ink/10"
        />
        <span
          aria-hidden="true"
          data-testid="fader-fill"
          className="absolute bottom-0 left-1/2 w-1.5 -translate-x-1/2 rounded-full"
          style={{ height: `${norm * 100}%`, background: color }}
        />
        <span
          aria-hidden="true"
          data-testid="fader-thumb"
          className="absolute left-1/2 h-3 w-[22px] -translate-x-1/2 translate-y-1/2 rounded-[4px] border border-line2 bg-ink"
          style={{ bottom: `${norm * 100}%` }}
        />
        {dragging && (
          <span
            data-testid="fader-chip"
            className="glass-solid pointer-events-none absolute bottom-full left-1/2 z-10 mb-1.5 -translate-x-1/2 whitespace-nowrap rounded-rs px-1.5 py-0.5 font-mono text-[11px] font-medium leading-none tabular-nums text-ink"
          >
            {text}
          </span>
        )}
      </div>
      <span
        aria-hidden="true"
        className="font-mono text-[11px] font-medium leading-none tabular-nums text-ink"
      >
        {text}
      </span>
    </div>
  );
});
