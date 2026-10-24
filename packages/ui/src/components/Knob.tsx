import { useId, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { cn } from "../lib/utils";
import { formatValue, toNormalized, toPlain, type ParamSpecView } from "../lib/param";

export type Highlight = "claude" | "external" | null;

export type KnobProps = {
  spec: ParamSpecView;
  /** Current plain value. */
  value: number;
  size?: "lg" | "md" | "sm";
  /** Live, on every pointer move. */
  onChange: (plain: number) => void;
  /** On release, keyboard step and double-click reset. */
  onCommit: (plain: number) => void;
  /** Called with the spec on focus and with null on blur (drives the help strip). */
  onHelp?: (spec: ParamSpecView | null) => void;
  highlight?: Highlight;
  /** The param is automated in the current section. */
  automated?: boolean;
  /** CSS colour for the value arc and the automation dot. */
  color?: string;
  /** Dimmed; ignores pointer and keys. */
  disabled?: boolean;
};

/** Pointer travel (px) for the full range; Shift makes it ten times finer. */
const DRAG_PX = 200;
const FINE = 0.1;
const STEP = 0.01;
const STEP_FINE = 0.001;
const STEP_PAGE = 0.1;

const SWEEP = 270;
const START = -135;
const CX = 50;
const CY = 50;
const ARC_R = 42;

const PX = { lg: 56, md: 46, sm: 36 } as const;
/** Column width per size, as the board's `.knob` (62) and `.knob.sm` (50). */
const WIDTH = { lg: 72, md: 62, sm: 50 } as const;

const point = (deg: number, r: number) => {
  const a = (deg * Math.PI) / 180;
  return { x: CX + r * Math.sin(a), y: CY - r * Math.cos(a) };
};

function arc(fromDeg: number, toDeg: number, r: number): string {
  const a = point(fromDeg, r);
  const b = point(toDeg, r);
  const large = toDeg - fromDeg > 180 ? 1 : 0;
  return `M ${a.x.toFixed(3)} ${a.y.toFixed(3)} A ${r} ${r} 0 ${large} 1 ${b.x.toFixed(3)} ${b.y.toFixed(3)}`;
}

type Drag = { startY: number; startNorm: number; shift: boolean; norm: number; moved: boolean };

export function Knob({
  spec,
  value,
  size = "md",
  onChange,
  onCommit,
  onHelp,
  highlight = null,
  automated = false,
  color = "rgb(var(--ink))",
  disabled = false,
}: KnobProps) {
  const [dragging, setDragging] = useState(false);
  const norm = toNormalized(spec, value);
  const drag = useRef<Drag | null>(null);
  const gradientId = `knob-body-${useId().replace(/:/g, "")}`;
  const valueText = formatValue(spec, value);

  const angle = START + norm * SWEEP;
  const px = PX[size];

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled || e.button !== 0) return;
    drag.current = { startY: e.clientY, startNorm: norm, shift: e.shiftKey, norm, moved: false };
    setDragging(true);
    // jsdom (and a stale pointer id) can lack or reject capture; dragging still works.
    try {
      e.currentTarget.setPointerCapture?.(e.pointerId);
    } catch {
      /* ignore */
    }
    e.currentTarget.focus();
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d || disabled) return;
    if (e.shiftKey !== d.shift) {
      // Re-anchor so pressing or releasing Shift mid-drag does not make the value jump.
      d.startY = e.clientY;
      d.startNorm = d.norm;
      d.shift = e.shiftKey;
    }
    const scale = d.shift ? FINE : 1;
    d.norm = Math.min(1, Math.max(0, d.startNorm + ((d.startY - e.clientY) / DRAG_PX) * scale));
    d.moved = true;
    onChange(toPlain(spec, d.norm));
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
    if (d.moved) onCommit(toPlain(spec, d.norm));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    let next: number | null = null;
    switch (e.key) {
      case "ArrowUp":
      case "ArrowRight":
        next = norm + (e.shiftKey ? STEP_FINE : STEP);
        break;
      case "ArrowDown":
      case "ArrowLeft":
        next = norm - (e.shiftKey ? STEP_FINE : STEP);
        break;
      case "PageUp":
        next = norm + STEP_PAGE;
        break;
      case "PageDown":
        next = norm - STEP_PAGE;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = 1;
        break;
    }
    if (next === null) return;
    e.preventDefault();
    onCommit(toPlain(spec, next));
  };

  return (
    <div
      className={cn("flex select-none flex-col items-center gap-[3px]", disabled && "opacity-40")}
      style={{ width: WIDTH[size] }}
    >
      <div
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={spec.label}
        aria-valuemin={spec.min}
        aria-valuemax={spec.max}
        aria-valuenow={value}
        aria-valuetext={valueText}
        aria-disabled={disabled ? "true" : undefined}
        data-highlight={highlight ?? undefined}
        className={cn(
          "group relative touch-none rounded-full",
          disabled ? "cursor-not-allowed" : "cursor-ns-resize",
          "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2",
          "focus-visible:outline-ink2",
        )}
        style={{ width: px, height: px }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={() => {
          if (!disabled) onCommit(spec.default);
        }}
        onKeyDown={onKeyDown}
        onFocus={() => onHelp?.(spec)}
        onBlur={() => onHelp?.(null)}
      >
        <svg viewBox="0 0 100 100" width={px} height={px} aria-hidden="true" overflow="visible">
          <defs>
            <radialGradient id={gradientId} cx="35%" cy="30%" r="72%">
              <stop offset="0%" stopColor="#3B2F57" />
              <stop offset="72%" stopColor="#140E1F" />
            </radialGradient>
          </defs>
          {highlight && (
            <circle
              data-testid="knob-highlight"
              cx={CX}
              cy={CY}
              r={49}
              fill="none"
              stroke={highlight === "claude" ? "rgb(var(--agent))" : "rgb(var(--ink2))"}
              strokeWidth={3}
              style={
                highlight === "claude"
                  ? { filter: "drop-shadow(0 0 5px rgb(var(--agent) / 0.6))" }
                  : undefined
              }
            />
          )}
          <path
            d={arc(START, START + SWEEP, ARC_R)}
            fill="none"
            stroke="rgba(255,255,255,.1)"
            strokeWidth={8}
            strokeLinecap="round"
            className={disabled ? undefined : "group-hover:[stroke:rgba(255,255,255,.16)]"}
          />
          {norm > 0 && (
            <path
              data-testid="knob-arc"
              d={arc(START, angle, ARC_R)}
              fill="none"
              stroke={color}
              strokeWidth={8}
              strokeLinecap="round"
            />
          )}
          <circle cx={CX} cy={CY} r={31} fill={`url(#${gradientId})`} />
          <rect
            x={49}
            y={CY - 31 + 4}
            width={4}
            height={17}
            rx={2}
            fill="rgb(var(--ink))"
            transform={`rotate(${angle} ${CX} ${CY})`}
          />
        </svg>
        {highlight === "claude" && (
          <span className="absolute -right-2 -top-1.5 rounded-[4px] bg-agent/15 px-[3px] font-mono text-[8.5px] leading-[12px] text-agent">
            AI
          </span>
        )}
        {automated && (
          <span
            data-testid="knob-lane"
            title="Automated in this section"
            className="pointer-events-none absolute left-1/2 h-1 w-1 -translate-x-1/2 rounded-full"
            // Sits under the value and label, which render below the dial.
            style={{ top: px + 38, background: color }}
          />
        )}
        {dragging && (
          <span
            data-testid="knob-chip"
            className="glass-solid pointer-events-none absolute bottom-full left-1/2 z-10 mb-1.5 -translate-x-1/2 whitespace-nowrap rounded-rs px-1.5 py-0.5 font-mono text-[11px] font-medium leading-none tabular-nums text-ink"
          >
            {valueText}
          </span>
        )}
      </div>
      <span
        aria-hidden="true"
        className={cn(
          "whitespace-nowrap font-mono font-medium leading-none tabular-nums text-ink",
          size === "sm" ? "text-[10px]" : "text-[11px]",
        )}
      >
        {valueText}
      </span>
      <span
        aria-hidden="true"
        className={cn(
          "text-center font-bold uppercase leading-[1.15] text-ink2",
          size === "sm" ? "text-[9px] tracking-[.06em]" : "text-[9.5px] tracking-[.08em]",
        )}
      >
        {spec.label}
      </span>
    </div>
  );
}
