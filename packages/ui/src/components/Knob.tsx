import { useId, useRef, type KeyboardEvent, type PointerEvent } from "react";
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

const PX = { lg: 96, md: 72, sm: 56 } as const;

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
}: KnobProps) {
  const norm = toNormalized(spec, value);
  const drag = useRef<Drag | null>(null);
  const gradientId = `knob-body-${useId().replace(/:/g, "")}`;
  const valueText = formatValue(spec, value);

  const angle = START + norm * SWEEP;
  const tip = point(angle, 30);
  const hub = point(angle, 10);
  const px = PX[size];

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    drag.current = { startY: e.clientY, startNorm: norm, shift: e.shiftKey, norm, moved: false };
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
    if (!d) return;
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
    try {
      e.currentTarget.releasePointerCapture?.(e.pointerId);
    } catch {
      /* ignore */
    }
    // A click without movement touched nothing; committing would add a write and an undo step.
    if (d.moved) onCommit(toPlain(spec, d.norm));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
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
    <div className="flex select-none flex-col items-center gap-1" style={{ width: px + 16 }}>
      <div
        role="slider"
        tabIndex={0}
        aria-label={spec.label}
        aria-valuemin={spec.min}
        aria-valuemax={spec.max}
        aria-valuenow={value}
        aria-valuetext={valueText}
        data-highlight={highlight ?? undefined}
        className={cn(
          "relative cursor-ns-resize touch-none rounded-full",
          "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2",
          "focus-visible:outline-silkscreen",
        )}
        style={{ width: px, height: px }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={() => onCommit(spec.default)}
        onKeyDown={onKeyDown}
        onFocus={() => onHelp?.(spec)}
        onBlur={() => onHelp?.(null)}
      >
        <svg viewBox="0 0 100 100" width={px} height={px} aria-hidden="true">
          <defs>
            <radialGradient id={gradientId} cx="40%" cy="32%" r="75%">
              <stop offset="0%" stopColor="hsl(215 9% 34%)" />
              <stop offset="60%" stopColor="hsl(216 10% 19%)" />
              <stop offset="100%" stopColor="hsl(210 8% 9%)" />
            </radialGradient>
          </defs>
          {highlight && (
            <circle
              data-testid="knob-highlight"
              cx={CX}
              cy={CY}
              r={47}
              fill="none"
              stroke="rgb(var(--claude))"
              strokeWidth={2.5}
              strokeDasharray={highlight === "external" ? "4 3" : undefined}
            />
          )}
          <path
            d={arc(START, START + SWEEP, ARC_R)}
            fill="none"
            stroke="rgb(var(--border))"
            strokeWidth={4}
            strokeLinecap="round"
          />
          {norm > 0 && (
            <path
              data-testid="knob-arc"
              d={arc(START, angle, ARC_R)}
              fill="none"
              stroke="rgb(var(--led))"
              strokeWidth={4}
              strokeLinecap="round"
            />
          )}
          <circle
            cx={CX}
            cy={CY}
            r={33}
            fill={`url(#${gradientId})`}
            stroke="hsl(210 8% 6%)"
            strokeWidth={1.5}
          />
          <line
            x1={hub.x}
            y1={hub.y}
            x2={tip.x}
            y2={tip.y}
            stroke="rgb(var(--silkscreen))"
            strokeWidth={3}
            strokeLinecap="round"
          />
          {automated && (
            <g data-testid="knob-lane">
              <title>Automated in this section</title>
              <rect x={78} y={4} width={18} height={9} rx={2} fill="rgb(var(--slide))" />
              <path
                d="M81 10 L85 7 L89 10 L93 7"
                fill="none"
                stroke="rgb(var(--chassis))"
                strokeWidth={1.4}
              />
            </g>
          )}
        </svg>
      </div>
      <span aria-hidden="true" className="font-label text-sm leading-none text-silkscreen">
        {spec.label}
      </span>
      <span
        aria-hidden="true"
        className="font-value text-xs leading-none tabular-nums text-silkscreen/80"
      >
        {valueText}
      </span>
    </div>
  );
}
