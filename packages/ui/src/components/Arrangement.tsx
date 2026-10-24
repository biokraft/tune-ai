import * as Popover from "@radix-ui/react-popover";
import {
  memo,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { cn } from "../lib/utils";
import {
  dropIndex,
  fitPxPerBar,
  HEADER_W,
  holdPoints,
  rulerStep,
  snapBars,
  type SectionView,
} from "../lib/arrange";
import { instrumentTitle, trackColor } from "../lib/instruments";
import { MuteSolo } from "./MuteSolo";

export type { SectionView } from "../lib/arrange";

export type ArrangementTrack = { id: string; instrument: string; mute: boolean; solo: boolean };

export type ArrangementProps = {
  sections: SectionView[];
  tracks: ArrangementTrack[];
  patterns: { id: string; track: string }[];
  /** Fractional zero-based bar. */
  playhead: number;
  selected: number | null;
  loop: [number, number] | null;
  /** The track shown in the dock, if any; its header is pressed. */
  openTrack: string | null;
  /** Edit paths changed by Claude (`sections`, `sections[i]`, `sections[i].*`). */
  highlighted?: Set<string>;
  onSelect(i: number): void;
  onSeek(bar: number): void;
  onMove(from: number, to: number): void;
  onResize(i: number, bars: number): void;
  onDuplicate(i: number): void;
  onDelete(i: number): void;
  onSetPlay(section: number, track: string, pattern: string | null): void;
  onOpenTrack(track: string): void;
  onMix(track: string, patch: { mute?: boolean; solo?: boolean }): void;
};

/** Title bar, as `.tlbar` on the Main board. */
const TITLE_H = 46;
const RULER_H = 24;
const SECROW_H = 32;
/** Track row height, as `--row`. */
const ROW_H = 46;
/** Ruler and sections rows: the sticky band above the track rows. */
const TOP = RULER_H + SECROW_H;
/** Room after the last bar, as the board's `padding-right`. */
const PAD_R = 10;
const HANDLE_W = 6;
/** Pattern cells are chips sized to their name (at least this wide, at most the section), so
 * names stay readable and the lane curves stay visible across the rest of the section. */
const CHIP_W = 44;
/** Pointer travel (px) below which a press is a click, not a drag. */
const DRAG_SLOP = 4;

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play";
/** Sticky parts cover what scrolls under them. */
const stickyFill = "bg-[rgb(var(--solid)/0.92)]";
const WHITE_EDGE = "rgb(255 255 255 / .4)";

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

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** Left offset of section `i`, following a move drag. */
const xOf = (s: SectionView, i: number, drag: Drag | null, ppb: number) =>
  s.start_bar * ppb + (drag?.kind === "move" && drag.i === i ? drag.dx : 0);

/** Width of section `i`, following a resize drag. */
const widthOf = (s: SectionView, i: number, drag: Drag | null, ppb: number) =>
  (drag?.kind === "resize" && drag.i === i ? drag.bars : s.bars) * ppb;

/**
 * The song's timeline: a title bar, then a scroller holding the sticky 168 px track-header
 * column beside the lane area. The lane area stacks the sticky ruler and sections rows over
 * one row per track. The bar width fits the song to the scroller's width.
 */
export function Arrangement(p: ArrangementProps) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  /** A body drag passed the slop; the click that follows the release must do nothing. */
  const didDrag = useRef(false);
  const update = (d: Drag | null) => {
    dragRef.current = d;
    setDrag(d);
  };

  const scroller = useRef<HTMLDivElement>(null);
  const [measured, setMeasured] = useState(0);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    setMeasured(el.clientWidth);
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => setMeasured(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const total = p.sections.reduce((m, s) => Math.max(m, s.start_bar + s.bars), 0);
  // Fit to the bars the song has; a resize drag past the end widens the lanes, not the zoom.
  const ppb = fitPxPerBar(measured > 0 ? measured - PAD_R : 0, total);
  const dragEnd = drag?.kind === "resize" ? p.sections[drag.i]!.start_bar + drag.bars : 0;
  const laneW = Math.max(total, dragEnd) * ppb;
  const rowsH = p.tracks.length * ROW_H;
  const secRow = useRef<SVGSVGElement>(null);
  const height = TOP + rowsH;
  const step = rulerStep(ppb);

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
      dropIndex(p.sections, p.sections[d.i]!.start_bar * ppb + d.dx, ppb),
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
    const raw = p.sections[d.i]!.bars + (e.clientX - d.startX) / ppb;
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
      // Only from the frame or the sections row: a focused header or block button keeps its section.
      const onSections =
        e.target === e.currentTarget || !!secRow.current?.contains(e.target as Node);
      if (!onSections) return;
      e.preventDefault();
      p.onDelete(sel);
    } else if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      const to = sel + (e.key === "ArrowLeft" ? -1 : 1);
      if (to >= 0 && to < p.sections.length) p.onMove(sel, to);
    }
  };

  return (
    <div
      role="group"
      aria-label="Arrangement"
      tabIndex={0}
      onKeyDown={onKeyDown}
      className={cn("flex h-full min-h-0 flex-col rounded-r text-ink", focusRing)}
    >
      <div
        className="flex flex-none items-center gap-2 border-b border-line px-3"
        style={{ height: TITLE_H }}
      >
        <span className="text-[13px] font-extrabold">Arrangement</span>
        <span className="text-xs text-ink2">
          {plural(p.sections.length, "section")} · {plural(p.tracks.length, "track")}
        </span>
      </div>
      <div ref={scroller} className="relative min-h-0 flex-1 overflow-auto">
        <div className="relative flex" style={{ width: HEADER_W + laneW + PAD_R, height }}>
          <TrackHeaders
            tracks={p.tracks}
            openTrack={p.openTrack}
            onOpenTrack={p.onOpenTrack}
            onMix={p.onMix}
          />
          <div className="relative shrink-0" style={{ width: laneW, height }}>
            <div className={cn("sticky top-0 z-20", stickyFill)} style={{ height: TOP }}>
              <Ruler laneW={laneW} ppb={ppb} step={step} loop={p.loop} />
              <svg
                ref={secRow}
                width={laneW}
                height={SECROW_H}
                className="block border-b border-line"
              >
                <LoopBand loop={p.loop} ppb={ppb} height={SECROW_H} />
                {p.sections.map((s, i) => {
                  const moving = drag?.kind === "move" && drag.i === i;
                  const w = widthOf(s, i, drag, ppb);
                  const lit = isLit(p.highlighted, i);
                  const sel = p.selected === i;
                  return (
                    <g
                      key={i}
                      transform={`translate(${xOf(s, i, drag, ppb)} 0)`}
                      opacity={moving ? 0.7 : 1}
                    >
                      <rect
                        x={1}
                        y={4}
                        width={Math.max(0, w - 2)}
                        height={SECROW_H - 8}
                        rx={7}
                        fill={sel ? "rgb(var(--ink) / .08)" : "rgb(var(--ink) / .035)"}
                        stroke={sel ? "var(--line2)" : "none"}
                        pointerEvents="none"
                      />
                      {lit && (
                        <rect
                          data-highlight="claude"
                          x={1}
                          y={4}
                          width={Math.max(0, w - 2)}
                          height={SECROW_H - 8}
                          rx={7}
                          fill="none"
                          stroke="rgb(var(--agent))"
                          strokeWidth={1.5}
                          pointerEvents="none"
                        />
                      )}
                      <svg x={8} width={Math.max(0, w - 8 - HANDLE_W)} height={SECROW_H}>
                        <text
                          y={20}
                          fontSize={11}
                          fontWeight={700}
                          fill={sel ? "rgb(var(--ink))" : "rgb(var(--ink2))"}
                          pointerEvents="none"
                        >
                          {s.name}
                        </text>
                      </svg>
                      <rect
                        data-testid={`section-${i}`}
                        data-highlight={lit ? "claude" : undefined}
                        data-selected={sel}
                        x={0}
                        y={0}
                        width={w}
                        height={SECROW_H}
                        fill="transparent"
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
                      <rect
                        data-testid={`resize-${i}`}
                        x={w - HANDLE_W}
                        y={4}
                        width={HANDLE_W}
                        height={SECROW_H - 8}
                        rx={2}
                        fill="rgb(var(--ink))"
                        opacity={0.16}
                        className="cursor-ew-resize touch-none"
                        onPointerDown={(e) => edgeDown(i, e)}
                        onPointerMove={edgeMove}
                        onPointerUp={edgeUp}
                        onPointerCancel={edgeUp}
                      />
                    </g>
                  );
                })}
              </svg>
            </div>
            <svg
              width={laneW}
              height={rowsH}
              className="absolute left-0"
              style={{ top: TOP }}
              aria-hidden="true"
            >
              {gridBars(laneW / ppb, step).map((b) => (
                <line
                  key={b}
                  x1={b * ppb}
                  x2={b * ppb}
                  y1={0}
                  y2={rowsH}
                  stroke="rgb(var(--ink) / .05)"
                />
              ))}
              {p.tracks.map((t, r) => (
                <line
                  key={t.id}
                  x1={0}
                  x2={laneW}
                  y1={(r + 1) * ROW_H - 0.5}
                  y2={(r + 1) * ROW_H - 0.5}
                  stroke="rgb(var(--ink) / .045)"
                />
              ))}
              <LoopBand loop={p.loop} ppb={ppb} height={rowsH} testId="loop-band" />
            </svg>
            <Cells
              sections={p.sections}
              tracks={p.tracks}
              patterns={p.patterns}
              drag={drag}
              ppb={ppb}
              onSetPlay={p.onSetPlay}
            />
            <LaneOverlay
              sections={p.sections}
              tracks={p.tracks}
              ppb={ppb}
              width={laneW}
              height={height}
            />
            <svg
              width={laneW}
              height={height}
              className="pointer-events-none absolute left-0 top-0 z-[25]"
              style={{ filter: "drop-shadow(0 0 6px rgb(var(--play) / .8))" }}
              aria-hidden="true"
            >
              <line
                data-testid="playhead"
                x1={p.playhead * ppb}
                x2={p.playhead * ppb}
                y1={0}
                y2={height}
                stroke="rgb(var(--play))"
                strokeWidth={2}
              />
            </svg>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Bars with a ruler label (and a grid line), from 0 up to but not including `bars`. */
function gridBars(bars: number, step: number): number[] {
  const out: number[] = [];
  for (let b = 0; b < bars - 1e-9; b += step) out.push(b);
  return out;
}

/** The loop range as a faint white band with 1 px white edges. */
function LoopBand({
  loop,
  ppb,
  height,
  testId,
}: {
  loop: [number, number] | null;
  ppb: number;
  height: number;
  testId?: string;
}) {
  if (!loop) return null;
  const x0 = loop[0] * ppb;
  const x1 = loop[1] * ppb;
  return (
    <g pointerEvents="none">
      <rect
        data-testid={testId}
        x={x0}
        y={0}
        width={x1 - x0}
        height={height}
        fill="rgb(var(--ink) / .06)"
      />
      <line x1={x0 + 0.5} x2={x0 + 0.5} y1={0} y2={height} stroke={WHITE_EDGE} />
      <line x1={x1 - 0.5} x2={x1 - 0.5} y1={0} y2={height} stroke={WHITE_EDGE} />
    </g>
  );
}

type RulerProps = {
  laneW: number;
  ppb: number;
  step: number;
  loop: [number, number] | null;
};

/** One-based bar numbers every `step` bars over a tick per bar, and the loop's white bracket. */
const Ruler = memo(function Ruler({ laneW, ppb, step, loop }: RulerProps) {
  return (
    <svg
      width={laneW}
      height={RULER_H}
      className="block border-b border-line"
      style={{
        backgroundImage:
          "repeating-linear-gradient(90deg, rgb(255 255 255 / .22) 0 1px, transparent 1px 100%)",
        backgroundSize: `${ppb}px 5px`,
        backgroundRepeat: "repeat-x",
        backgroundPosition: "0 100%",
      }}
    >
      <LoopBand loop={loop} ppb={ppb} height={RULER_H} />
      {loop && (
        <rect
          data-testid="loop-range"
          x={loop[0] * ppb}
          y={0}
          width={(loop[1] - loop[0]) * ppb}
          height={5}
          rx={2}
          fill="rgb(255 255 255 / .55)"
          pointerEvents="none"
        />
      )}
      <g aria-hidden="true">
        {gridBars(laneW / ppb, step).map((b) => (
          <g key={b}>
            <line
              x1={b * ppb + 0.5}
              x2={b * ppb + 0.5}
              y1={0}
              y2={RULER_H}
              stroke="rgb(255 255 255 / .18)"
            />
            <text
              x={b * ppb + 4}
              y={14}
              fontSize={10}
              fontWeight={500}
              fill="rgb(var(--ink3))"
              className="font-mono tabular-nums"
            >
              {b + 1}
            </text>
          </g>
        ))}
      </g>
    </svg>
  );
});

type TrackHeadersProps = Pick<ArrangementProps, "tracks" | "openTrack" | "onOpenTrack" | "onMix">;

/** The sticky header column: the ruler and sections labels, then one `.hdr` per track. */
const TrackHeaders = memo(function TrackHeaders({
  tracks,
  openTrack,
  onOpenTrack,
  onMix,
}: TrackHeadersProps) {
  return (
    <div
      role="rowgroup"
      aria-label="Tracks"
      className={cn("sticky left-0 z-30 shrink-0 border-r border-line", stickyFill)}
      style={{ width: HEADER_W }}
    >
      <div aria-hidden="true" className={cn("sticky top-0 z-10", stickyFill)}>
        <div className="flex items-center border-b border-line pl-3" style={{ height: RULER_H }}>
          <span className="lbl">Bar</span>
        </div>
        <div className="flex items-center border-b border-line pl-3" style={{ height: SECROW_H }}>
          <span className="lbl">Sections</span>
        </div>
      </div>
      {tracks.map((t) => {
        const open = openTrack === t.id;
        return (
          <div
            key={t.id}
            role="rowheader"
            className="flex items-center gap-2 border-b border-ink/5 pr-2"
            style={{ height: ROW_H }}
          >
            <button
              type="button"
              aria-label={`Open ${t.id}`}
              aria-pressed={open}
              onClick={() => onOpenTrack(t.id)}
              className={cn(
                "flex h-full min-w-0 flex-1 cursor-pointer items-center gap-2 border-0 bg-transparent p-0 text-left text-ink",
                "hover:bg-ink/5",
                open && "bg-ink/10 hover:bg-ink/10",
                focusRing,
              )}
            >
              <span
                aria-hidden="true"
                className="my-1.5 ml-0 mr-1 w-[3px] flex-none self-stretch rounded-sm"
                style={{ background: trackColor(t.instrument) }}
              />
              <span className="flex min-w-0 flex-col">
                <span className="truncate text-[12.5px] font-bold leading-tight">{t.id}</span>
                <span className="truncate text-[11px] leading-tight text-ink3">
                  {instrumentTitle(t.instrument)}
                </span>
              </span>
            </button>
            <MuteSolo
              track={t.id}
              mute={t.mute}
              solo={t.solo}
              onMute={(v) => onMix(t.id, { mute: v })}
              onSolo={(v) => onMix(t.id, { solo: v })}
            />
          </div>
        );
      })}
    </div>
  );
});

type CellsProps = Pick<ArrangementProps, "sections" | "tracks" | "patterns" | "onSetPlay"> & {
  drag: Drag | null;
  ppb: number;
};

/** One pattern picker per track and section. Task 10 replaces these with blocks. */
const Cells = memo(function Cells({
  sections,
  tracks,
  patterns,
  drag,
  ppb,
  onSetPlay,
}: CellsProps) {
  const [open, setOpen] = useState<string | null>(null);
  return (
    <>
      {tracks.map((t, r) =>
        sections.map((s, i) => {
          const key = `${i}:${t.id}`;
          const current = s.play[t.id];
          const w = widthOf(s, i, drag, ppb);
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
                  title={current ?? undefined}
                  className={cn(
                    "absolute truncate rounded-[7px] border border-line bg-ink/5 px-1.5 text-left",
                    "font-mono text-[10.5px]",
                    current ? "text-ink2" : "text-ink3",
                    focusRing,
                  )}
                  style={{
                    left: xOf(s, i, drag, ppb) + 2,
                    top: TOP + r * ROW_H + 5,
                    minWidth: Math.min(CHIP_W, w - 4),
                    maxWidth: w - 4,
                    height: ROW_H - 10,
                  }}
                >
                  {current ?? "—"}
                </button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content
                  sideOffset={4}
                  className="glass-solid z-50 flex w-40 flex-col gap-1 rounded-rs p-2 text-ink"
                >
                  {patterns
                    .filter((pat) => pat.track === t.id)
                    .map((pat) => (
                      <button
                        key={pat.id}
                        type="button"
                        aria-current={current === pat.id ? "true" : undefined}
                        className={cn(
                          "rounded-[7px] border border-line px-2 py-1 text-left font-mono text-xs text-ink2 hover:bg-ink/5",
                          current === pat.id && "border-line2 bg-ink/10 text-ink",
                          focusRing,
                        )}
                        onClick={() => {
                          onSetPlay(i, t.id, pat.id);
                          setOpen(null);
                        }}
                      >
                        {pat.id}
                      </button>
                    ))}
                  <button
                    type="button"
                    className={cn(
                      "rounded-[7px] border border-line px-2 py-1 text-left text-xs text-ink2 hover:bg-ink/5",
                      focusRing,
                    )}
                    onClick={() => {
                      onSetPlay(i, t.id, null);
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
    </>
  );
});

type LaneOverlayProps = Pick<ArrangementProps, "sections" | "tracks"> & {
  ppb: number;
  width: number;
  height: number;
};

/** Automation curves over the cells, in the track's colour; clicks pass through. */
const LaneOverlay = memo(function LaneOverlay({
  sections,
  tracks,
  ppb,
  width,
  height,
}: LaneOverlayProps) {
  return (
    <svg
      data-testid="lane-overlay"
      width={width}
      height={height}
      className="pointer-events-none absolute left-0 top-0 z-10"
      aria-hidden="true"
    >
      {tracks.map((t, r) => (
        <g key={t.id} transform={`translate(0 ${TOP + r * ROW_H})`}>
          {sections.map((s, i) =>
            s.lanes
              .filter((l) => l.track === t.id && l.points.length > 0)
              .map((l) => (
                <polyline
                  key={`${i}-${l.param}`}
                  data-lane={l.param}
                  fill="none"
                  stroke={trackColor(t.instrument)}
                  strokeOpacity={0.75}
                  strokeWidth={1.25}
                  points={holdPoints(l.points, s.bars)
                    .map(([bar, v]) => {
                      const y = ROW_H - 6 - Math.min(1, Math.max(0, v)) * (ROW_H - 12);
                      return `${(s.start_bar + bar) * ppb},${y}`;
                    })
                    .join(" ")}
                />
              )),
          )}
        </g>
      ))}
    </svg>
  );
});
