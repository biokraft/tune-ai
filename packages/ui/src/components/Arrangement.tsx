import * as Popover from "@radix-ui/react-popover";
import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { cn } from "../lib/utils";
import { dropIndex, holdPoints, PX_PER_BAR, snapBars, type SectionView } from "../lib/arrange";

export type { SectionView } from "../lib/arrange";

export type ArrangementProps = {
  sections: SectionView[];
  tracks: { id: string; instrument: string }[];
  patterns: { id: string; track: string }[];
  /** Fractional zero-based bar. */
  playhead: number;
  selected: number | null;
  loop: [number, number] | null;
  /** Edit paths changed by Claude (`sections`, `sections[i]`, `sections[i].*`). */
  highlighted?: Set<string>;
  onSelect(i: number): void;
  onSeek(bar: number): void;
  onMove(from: number, to: number): void;
  onResize(i: number, bars: number): void;
  onDuplicate(i: number): void;
  onDelete(i: number): void;
  onSetPlay(section: number, track: string, pattern: string | null): void;
};

const RULER_H = 20;
const BLOCK_H = 34;
const ROW_H = 30;
const HANDLE_W = 6;
/** Sticky track-name column width. */
const LABEL_W = 96;
/** Pattern cells are compact chips so the lane curves stay visible across the section. */
const CHIP_W = 44;
/** Pointer travel (px) below which a press is a click, not a drag. */
const DRAG_SLOP = 4;

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen";

type Drag =
  | { kind: "move"; i: number; startX: number; dx: number }
  | { kind: "resize"; i: number; startX: number; bars: number };

function capture(e: PointerEvent<Element>) {
  // jsdom (and a stale pointer id) can lack or reject capture; dragging still works.
  try {
    e.currentTarget.setPointerCapture?.(e.pointerId);
  } catch {
    /* ignore */
  }
}

function release(e: PointerEvent<Element>) {
  try {
    e.currentTarget.releasePointerCapture?.(e.pointerId);
  } catch {
    /* ignore */
  }
}

function isLit(h: Set<string> | undefined, i: number): boolean {
  if (!h) return false;
  if (h.has("sections") || h.has(`sections[${i}]`)) return true;
  const prefix = `sections[${i}].`;
  for (const p of h) if (p.startsWith(prefix)) return true;
  return false;
}

export function Arrangement(p: ArrangementProps) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  /** A body drag passed the slop; the click that follows the release must do nothing. */
  const didDrag = useRef(false);
  const update = (d: Drag | null) => {
    dragRef.current = d;
    setDrag(d);
  };

  const total = p.sections.reduce((m, s) => Math.max(m, s.start_bar + s.bars), 0);
  const width = (total + 8) * PX_PER_BAR;
  const height = RULER_H + BLOCK_H + p.tracks.length * ROW_H;

  const widthOf = (i: number, s: SectionView) =>
    drag?.kind === "resize" && drag.i === i ? drag.bars * PX_PER_BAR : s.bars * PX_PER_BAR;

  const bodyDown = (i: number, e: PointerEvent<SVGRectElement>) => {
    if (e.button !== 0) return;
    capture(e);
    didDrag.current = false;
    update({ kind: "move", i, startX: e.clientX, dx: 0 });
  };
  const bodyMove = (e: PointerEvent<SVGRectElement>) => {
    const d = dragRef.current;
    if (d?.kind !== "move") return;
    update({ ...d, dx: e.clientX - d.startX });
  };
  const bodyUp = (e: PointerEvent<SVGRectElement>) => {
    const d = dragRef.current;
    if (d?.kind !== "move") return;
    release(e);
    update(null);
    if (Math.abs(d.dx) < DRAG_SLOP) return;
    didDrag.current = true;
    const to = Math.min(
      p.sections.length - 1,
      dropIndex(p.sections, p.sections[d.i]!.start_bar * PX_PER_BAR + d.dx),
    );
    if (to !== d.i) {
      p.onMove(d.i, to);
      p.onSelect(to);
    }
  };

  const edgeDown = (i: number, e: PointerEvent<SVGRectElement>) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    capture(e);
    update({ kind: "resize", i, startX: e.clientX, bars: p.sections[i]!.bars });
  };
  const edgeMove = (e: PointerEvent<SVGRectElement>) => {
    const d = dragRef.current;
    if (d?.kind !== "resize") return;
    const raw = p.sections[d.i]!.bars + (e.clientX - d.startX) / PX_PER_BAR;
    update({ ...d, bars: snapBars(raw, e.altKey) });
  };
  const edgeUp = (e: PointerEvent<SVGRectElement>) => {
    const d = dragRef.current;
    if (d?.kind !== "resize") return;
    release(e);
    update(null);
    if (d.bars !== p.sections[d.i]!.bars) p.onResize(d.i, d.bars);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    // Portalled popovers bubble React events here; only act on our own subtree.
    if (!e.currentTarget.contains(e.target as Node)) return;
    const sel = p.selected;
    if (sel === null) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "d") {
      e.preventDefault();
      p.onDuplicate(sel);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      p.onDelete(sel);
    } else if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      const to = sel + (e.key === "ArrowLeft" ? -1 : 1);
      if (to >= 0 && to < p.sections.length) p.onMove(sel, to);
    }
  };

  const ticks: number[] = [];
  for (let b = 0; b <= total + 8; b += 8) ticks.push(b);

  return (
    <div
      role="group"
      aria-label="Arrangement"
      tabIndex={0}
      onKeyDown={onKeyDown}
      className={cn("overflow-x-auto bg-chassis text-silkscreen", focusRing)}
    >
      <div className="flex" style={{ width: width + LABEL_W, height }}>
        <div
          role="rowgroup"
          aria-label="Tracks"
          className="sticky left-0 z-20 shrink-0 bg-chassis"
          style={{ width: LABEL_W, paddingTop: RULER_H + BLOCK_H }}
        >
          {p.tracks.map((t) => (
            <div
              key={t.id}
              role="rowheader"
              className="flex flex-col justify-center overflow-hidden px-2"
              style={{ height: ROW_H }}
            >
              <span className="truncate font-label text-sm leading-none text-silkscreen">
                {t.id}
              </span>
              <span className="truncate font-value text-[10px] leading-none text-silkscreen/60">
                {t.instrument}
              </span>
            </div>
          ))}
        </div>
        <div className="relative shrink-0" style={{ width, height }}>
          <svg width={width} height={height} className="absolute left-0 top-0">
            <g aria-hidden="true">
              {ticks.map((b) => (
                <g key={b}>
                  <line
                    x1={b * PX_PER_BAR}
                    x2={b * PX_PER_BAR}
                    y1={RULER_H - 6}
                    y2={height}
                    stroke="hsl(var(--border))"
                    strokeWidth={1}
                    opacity={0.5}
                  />
                  <text
                    x={b * PX_PER_BAR + 3}
                    y={12}
                    fontSize={11}
                    fill="hsl(var(--silkscreen))"
                    className="font-value"
                  >
                    {b + 1}
                  </text>
                </g>
              ))}
            </g>
            {p.loop && (
              <rect
                data-testid="loop-range"
                x={p.loop[0] * PX_PER_BAR}
                y={RULER_H - 4}
                width={(p.loop[1] - p.loop[0]) * PX_PER_BAR}
                height={4}
                fill="hsl(var(--slide))"
              />
            )}
            {p.sections.map((s, i) => {
              const moving = drag?.kind === "move" && drag.i === i;
              const x = s.start_bar * PX_PER_BAR + (moving ? drag.dx : 0);
              const w = widthOf(i, s);
              const lit = isLit(p.highlighted, i);
              const sel = p.selected === i;
              return (
                <g key={i} transform={`translate(${x} ${RULER_H})`} opacity={moving ? 0.7 : 1}>
                  <rect
                    data-testid={`section-${i}`}
                    data-highlight={lit ? "claude" : undefined}
                    data-selected={sel}
                    x={0}
                    y={0}
                    width={w}
                    height={BLOCK_H}
                    rx={3}
                    fill="hsl(var(--faceplate))"
                    stroke={
                      lit
                        ? "hsl(var(--claude))"
                        : sel
                          ? "hsl(var(--silkscreen))"
                          : "hsl(var(--border))"
                    }
                    strokeWidth={lit || sel ? 2 : 1}
                    className="cursor-grab touch-none"
                    onPointerDown={(e) => bodyDown(i, e)}
                    onPointerMove={bodyMove}
                    onPointerUp={bodyUp}
                    onPointerCancel={bodyUp}
                    onClick={() => {
                      if (!didDrag.current) p.onSelect(i);
                    }}
                    onDoubleClick={() => {
                      if (!didDrag.current) p.onSeek(s.start_bar);
                    }}
                  />
                  <text
                    x={8}
                    y={21}
                    fontSize={14}
                    fill="hsl(var(--silkscreen))"
                    pointerEvents="none"
                    className="font-label"
                  >
                    {s.name}
                  </text>
                  <rect
                    data-testid={`resize-${i}`}
                    x={w - HANDLE_W}
                    y={0}
                    width={HANDLE_W}
                    height={BLOCK_H}
                    fill="hsl(var(--silkscreen))"
                    opacity={0.25}
                    className="cursor-ew-resize touch-none"
                    onPointerDown={(e) => edgeDown(i, e)}
                    onPointerMove={edgeMove}
                    onPointerUp={edgeUp}
                    onPointerCancel={edgeUp}
                  />
                </g>
              );
            })}
            {p.tracks.map((t, r) => (
              <g key={t.id} transform={`translate(0 ${RULER_H + BLOCK_H + r * ROW_H})`}>
                <line
                  x1={0}
                  x2={width}
                  y1={ROW_H}
                  y2={ROW_H}
                  stroke="hsl(var(--border))"
                  opacity={0.4}
                />
              </g>
            ))}
            <line
              data-testid="playhead"
              x1={p.playhead * PX_PER_BAR}
              x2={p.playhead * PX_PER_BAR}
              y1={0}
              y2={height}
              stroke="hsl(var(--led))"
              strokeWidth={2}
              pointerEvents="none"
            />
          </svg>
          {p.tracks.map((t, r) =>
            p.sections.map((s, i) => {
              const key = `${i}:${t.id}`;
              const current = s.play[t.id];
              const x =
                s.start_bar * PX_PER_BAR + (drag?.kind === "move" && drag.i === i ? drag.dx : 0);
              return (
                <Popover.Root
                  key={key}
                  open={open === key}
                  onOpenChange={(o) => setOpen(o ? key : null)}
                >
                  <Popover.Trigger asChild>
                    <button
                      type="button"
                      aria-label={`${t.id} in section ${i + 1}`}
                      className={cn(
                        "absolute truncate rounded-sm border border-border bg-faceplate px-1",
                        "font-value text-xs text-silkscreen",
                        !current && "text-silkscreen/50",
                        focusRing,
                      )}
                      style={{
                        left: x + 2,
                        top: RULER_H + BLOCK_H + r * ROW_H + 3,
                        width: Math.min(CHIP_W, widthOf(i, s) - 4),
                        height: ROW_H - 6,
                      }}
                    >
                      {current ?? "—"}
                    </button>
                  </Popover.Trigger>
                  <Popover.Portal>
                    <Popover.Content
                      sideOffset={4}
                      className="z-50 flex w-40 flex-col gap-1 rounded-md border border-border bg-faceplate p-2 shadow-lg"
                    >
                      {p.patterns
                        .filter((pat) => pat.track === t.id)
                        .map((pat) => (
                          <button
                            key={pat.id}
                            type="button"
                            aria-current={current === pat.id ? "true" : undefined}
                            className={cn(
                              "rounded-sm border border-border px-2 py-1 text-left font-value text-sm text-silkscreen",
                              current === pat.id && "border-led",
                              focusRing,
                            )}
                            onClick={() => {
                              p.onSetPlay(i, t.id, pat.id);
                              setOpen(null);
                            }}
                          >
                            {pat.id}
                          </button>
                        ))}
                      <button
                        type="button"
                        className={cn(
                          "rounded-sm border border-border px-2 py-1 text-left font-value text-sm text-silkscreen",
                          focusRing,
                        )}
                        onClick={() => {
                          p.onSetPlay(i, t.id, null);
                          setOpen(null);
                        }}
                      >
                        None
                      </button>
                    </Popover.Content>
                  </Popover.Portal>
                </Popover.Root>
              );
            }),
          )}
          <svg
            data-testid="lane-overlay"
            width={width}
            height={height}
            className="pointer-events-none absolute left-0 top-0 z-10"
            aria-hidden="true"
          >
            {p.tracks.map((t, r) => (
              <g key={t.id} transform={`translate(0 ${RULER_H + BLOCK_H + r * ROW_H})`}>
                {p.sections.map((s, i) =>
                  s.lanes
                    .filter((l) => l.track === t.id && l.points.length > 0)
                    .map((l) => (
                      <polyline
                        key={`${i}-${l.param}`}
                        data-lane={l.param}
                        fill="none"
                        stroke="hsl(var(--slide))"
                        strokeWidth={1}
                        points={holdPoints(l.points, s.bars)
                          .map(([bar, v]) => {
                            const y = ROW_H - 4 - Math.min(1, Math.max(0, v)) * (ROW_H - 8);
                            return `${(s.start_bar + bar) * PX_PER_BAR},${y}`;
                          })
                          .join(" ")}
                      />
                    )),
                )}
              </g>
            ))}
          </svg>
        </div>
      </div>
    </div>
  );
}
