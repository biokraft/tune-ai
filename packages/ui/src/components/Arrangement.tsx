import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { cn } from "../lib/utils";
import {
  dropIndex,
  fitPxPerBar,
  gridBars,
  HEADER_W,
  holdPoints,
  rulerStep,
  snapBars,
  type SectionView,
} from "../lib/arrange";
import { instrumentTitle, trackColor, trackColorVar } from "../lib/instruments";
import type { Highlight } from "./Knob";
import { MuteSolo } from "./MuteSolo";

export type { SectionView } from "../lib/arrange";

export type ArrangementTrack = { id: string; instrument: string; mute: boolean; solo: boolean };

/** A pattern as its block draws it: one glyph value per step (rest 0, hit or note 0.6, accent 1). */
export type ArrangementPattern = {
  id: string;
  kind: "acid" | "drum";
  glyph: number[];
};

export type ArrangementProps = {
  sections: SectionView[];
  tracks: ArrangementTrack[];
  patterns: ArrangementPattern[];
  /** Fractional zero-based bar. */
  playhead: number;
  selected: number | null;
  loop: [number, number] | null;
  /** The track shown in the dock, if any; its header is pressed. */
  openTrack: string | null;
  /**
   * Edit paths changed from outside (`sections`, `sections[i]`, `sections[i].*`,
   * `patterns.<id>.*`), each with who changed it: Claude's are ringed cyan, the song file's
   * outlined dashed in ink2.
   */
  highlighted?: ReadonlyMap<string, Mark>;
  /** Lane row labels, keyed `${track}.${param}` (mix lanes have track `"mix"`); else the param id. */
  laneLabels?: Record<string, string>;
  onSelect(i: number): void;
  /** Jump to a zero-based bar, playing or stopped. */
  onSeek(bar: number): void;
  /** Loop zero-based bars `[start, end)`, from a drag on the ruler. */
  onLoop(bars: [number, number]): void;
  onMove(from: number, to: number): void;
  onResize(i: number, bars: number): void;
  onDuplicate(i: number): void;
  onDelete(i: number): void;
  /** A track header: opens the track in the dock, or closes the dock when it is docked. */
  onOpenTrack(track: string): void;
  /** A block or empty cell: opens the track in the dock, or keeps it there. */
  onShowTrack(track: string): void;
  onMix(track: string, patch: { mute?: boolean; solo?: boolean }): void;
};

/** Title bar, as `.tlbar` on the Main board. */
const TITLE_H = 46;
const RULER_H = 24;
const SECROW_H = 32;
/** Track row height, as `--row`. */
const ROW_H = 46;
/** An automation lane's row, under its track. */
const LANE_H = 22;
/** The "Mix" header row above the mix lanes. */
const MIXHDR_H = 28;
/** A block's inset inside its cell. */
const INSET = 3;
/** The glyph's side padding inside a block. */
const GLYPH_PAD = 6;
/** An accented acid dash's height above the block's foot, as a share of the block. */
const ACID_RISE = 0.45;
/** Ruler and sections rows: the sticky band above the track rows. */
const TOP = RULER_H + SECROW_H;
/** Room after the last bar, as the board's `padding-right`. */
const PAD_R = 10;
const HANDLE_W = 6;
/** Pointer travel (px) below which a press is a click, not a drag. */
const DRAG_SLOP = 4;

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play";
/**
 * Sticky parts cover what scrolls under them. Only then: at rest they stay clear like the
 * Timeline board's header column, so the glass shows through.
 */
const stickyFill = "bg-[rgb(var(--solid)/0.92)]";
const WHITE_EDGE = "rgb(255 255 255 / .4)";
const NO_LABELS: Record<string, string> = {};

/** One row of the timeline body, `y` from the top of the first track row. */
type Row =
  | { kind: "track"; track: ArrangementTrack; y: number; h: number }
  | { kind: "lane"; track: string; param: string; color: string; y: number; h: number }
  | { kind: "mix"; y: number; h: number };

/**
 * Each track's row, then a lane row per param it automates in any section (first seen first),
 * then a "Mix" header over the mix lanes, if there are any.
 */
function layoutRows(tracks: ArrangementTrack[], sections: SectionView[]): Row[] {
  const lanes = new Map<string, string[]>();
  for (const s of sections) {
    for (const l of s.lanes) {
      const list = lanes.get(l.track) ?? [];
      if (!list.includes(l.param)) list.push(l.param);
      lanes.set(l.track, list);
    }
  }
  const rows: Row[] = [];
  let y = 0;
  const laneRows = (track: string, color: string) => {
    for (const param of lanes.get(track) ?? []) {
      rows.push({ kind: "lane", track, param, color, y, h: LANE_H });
      y += LANE_H;
    }
  };
  for (const t of tracks) {
    rows.push({ kind: "track", track: t, y, h: ROW_H });
    y += ROW_H;
    laneRows(t.id, trackColor(t.instrument));
  }
  if (lanes.has("mix")) {
    rows.push({ kind: "mix", y, h: MIXHDR_H });
    y += MIXHDR_H;
    laneRows("mix", "rgb(var(--ink2))");
  }
  return rows;
}

/** Who changed something: Claude or the song file on disk. */
type Mark = NonNullable<Highlight>;
type Marks = ReadonlyMap<string, Mark> | undefined;

/** The strongest mark among the paths that `hit` matches: Claude's over the song file's. */
function strongest(h: Marks, hit: (path: string) => boolean): Highlight {
  if (!h) return null;
  let out: Highlight = null;
  for (const [path, kind] of h) {
    if (!hit(path)) continue;
    if (kind === "claude") return kind;
    out = kind;
  }
  return out;
}

/** Who changed what `track` plays in section `i`, or the pattern `id` itself, if anyone. */
function blockMark(h: Marks, i: number, track: string, id: string | undefined): Highlight {
  const play = `sections[${i}].play.${track}`;
  const pat = `patterns.${id}`;
  return strongest(
    h,
    (p) => p === play || (id !== undefined && (p === pat || p.startsWith(`${pat}.`))),
  );
}

/** Who changed section `i` (or the section list), if anyone. */
function sectionMark(h: Marks, i: number): Highlight {
  const self = `sections[${i}]`;
  return strongest(h, (p) => p === "sections" || p === self || p.startsWith(`${self}.`));
}

/** A highlight outline's stroke: cyan for Claude, a dashed ink2 line for the song file. */
const markStroke = (kind: Mark) =>
  kind === "claude"
    ? { stroke: "rgb(var(--agent))" }
    : { stroke: "rgb(var(--ink2))", strokeDasharray: "4 3" };

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

  /** The bar the playhead's cap is being dragged to; the playhead is drawn there, not at ticks. */
  const [capBar, setCapBar] = useState<number | null>(null);
  /** The cap was dropped and seeked: hold the dropped bar until the next tick moves the playhead. */
  const capHeld = useRef(false);
  const capDrag = useCallback((bar: number | null) => {
    capHeld.current = false;
    setCapBar(bar);
  }, []);
  const capDrop = useCallback((bar: number) => {
    capHeld.current = true;
    setCapBar(bar);
  }, []);
  // A new playhead after a drop is the backend's answer to the seek: follow the ticks again.
  // Without the hold the playhead would flash back to its stale position until that tick.
  useEffect(() => {
    if (!capHeld.current) return;
    capHeld.current = false;
    setCapBar(null);
  }, [p.playhead]);
  /** The sticky band over the lanes: its left edge is bar 0 on screen. */
  const band = useRef<HTMLDivElement>(null);

  const scroller = useRef<HTMLDivElement>(null);
  const [measured, setMeasured] = useState(0);
  /** Whether rows (y) or lanes (x) have scrolled under the sticky band and header column. */
  const [under, setUnder] = useState({ x: false, y: false });
  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const x = el.scrollLeft > 0;
    const y = el.scrollTop > 0;
    setUnder((u) => (u.x === x && u.y === y ? u : { x, y }));
  };
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
  const rows = useMemo(() => layoutRows(p.tracks, p.sections), [p.tracks, p.sections]);
  const rowsH = rows.reduce((sum, r) => sum + r.h, 0);
  const secRow = useRef<SVGSVGElement>(null);
  const height = TOP + rowsH;
  const step = rulerStep(ppb);
  const playhead = capBar ?? p.playhead;
  /**
   * The bar at a pointer's clientX, on the song's bars: the nearest bar line while dragging the
   * playhead, or the bar under the pointer (as the ruler picks it) for a still press.
   */
  const capBarAt = (clientX: number, snap: CapSnap) => {
    const bars = (clientX - (band.current?.getBoundingClientRect().left ?? 0)) / ppb;
    return clampBar(snap === "nearest" ? Math.round(bars) : Math.floor(bars), total);
  };

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
    // Portalled content bubbles React events here; only act on our own subtree.
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
      <div
        ref={scroller}
        className="relative min-h-0 flex-1 select-none overflow-auto overscroll-none"
        onScroll={onScroll}
      >
        <div className="relative flex" style={{ width: HEADER_W + laneW + PAD_R, height }}>
          <TrackHeaders
            fillColumn={under.x}
            fillTop={under.y}
            rows={rows}
            laneLabels={p.laneLabels ?? NO_LABELS}
            openTrack={p.openTrack}
            onOpenTrack={p.onOpenTrack}
            onMix={p.onMix}
          />
          <div className="relative shrink-0" style={{ width: laneW, height }}>
            <div
              ref={band}
              className={cn("sticky top-0 z-20", under.y && stickyFill)}
              style={{ height: TOP }}
            >
              <Ruler
                laneW={laneW}
                ppb={ppb}
                step={step}
                loop={p.loop}
                total={total}
                onSeek={p.onSeek}
                onLoop={p.onLoop}
              />
              {total > 0 && (
                <PlayheadCap
                  x={playhead * ppb}
                  bar={clampBar(Math.floor(playhead), total)}
                  total={total}
                  barAt={capBarAt}
                  onDrag={capDrag}
                  onDrop={capDrop}
                  onSeek={p.onSeek}
                />
              )}
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
                  const lit = sectionMark(p.highlighted, i);
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
                          data-highlight={lit}
                          x={1}
                          y={4}
                          width={Math.max(0, w - 2)}
                          height={SECROW_H - 8}
                          rx={7}
                          fill="none"
                          {...markStroke(lit)}
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
                        data-highlight={lit ?? undefined}
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
              {rows.map((r) => (
                <line
                  key={r.y}
                  x1={0}
                  x2={laneW}
                  y1={r.y + r.h - 0.5}
                  y2={r.y + r.h - 0.5}
                  stroke="rgb(var(--ink) / .045)"
                />
              ))}
              <LoopBand loop={p.loop} ppb={ppb} height={rowsH} testId="loop-band" />
            </svg>
            <Blocks
              sections={p.sections}
              rows={rows}
              patterns={p.patterns}
              highlighted={p.highlighted}
              selected={p.selected}
              drag={drag}
              ppb={ppb}
              onSelect={p.onSelect}
              onShowTrack={p.onShowTrack}
            />
            <LaneOverlay
              sections={p.sections}
              rows={rows}
              ppb={ppb}
              width={laneW}
              height={height}
            />
            <Playhead x={playhead * ppb} height={height} />
          </div>
        </div>
      </div>
    </div>
  );
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

/**
 * The playhead: one 2 px line over the ruler, the sections and the rows, centred on `x`. It
 * glows with a box-shadow and moves by transform alone. A CSS `filter` here would make WebKit
 * repaint the full-size filtered layer in tiles on every tick and leave stale glow ("fog")
 * behind it.
 */
function Playhead({ x, height }: { x: number; height: number }) {
  return (
    <div
      data-testid="playhead"
      data-x={x}
      aria-hidden="true"
      className="pointer-events-none absolute left-0 top-0 z-[25]"
      style={{
        width: 2,
        height,
        background: "rgb(var(--play))",
        boxShadow: "0 0 6px rgb(var(--play) / .8)",
        transform: `translateX(${x - 1}px)`,
        willChange: "transform",
      }}
    />
  );
}

/** Bar `bar` kept on the song's bars, `[0, total - 1]`. */
const clampBar = (bar: number, total: number) => Math.max(0, Math.min(total - 1, bar));

type RulerProps = {
  laneW: number;
  ppb: number;
  step: number;
  loop: [number, number] | null;
  /** The song's length in bars. */
  total: number;
  onSeek(bar: number): void;
  onLoop(bars: [number, number]): void;
};

/** A press on the ruler: the bar it started on, the bar under the pointer, and whether it moved. */
type RulerPress = { pointer: number; x0: number; from: number; to: number; drag: boolean };

/**
 * One-based bar numbers every `step` bars over a tick per bar, and the loop's white bracket.
 * A click seeks to the bar under the pointer; a drag previews a range and loops it on release.
 */
const Ruler = memo(function Ruler({ laneW, ppb, step, loop, total, onSeek, onLoop }: RulerProps) {
  const [press, setPress] = useState<RulerPress | null>(null);
  const pressRef = useRef<RulerPress | null>(null);
  const set = (next: RulerPress | null) => {
    pressRef.current = next;
    setPress(next);
  };
  // From the ruler's own left edge, so a scrolled timeline still finds the right bar.
  const barAt = (e: PointerEvent<SVGSVGElement>) =>
    clampBar(Math.floor((e.clientX - e.currentTarget.getBoundingClientRect().left) / ppb), total);

  /** The press in progress, if `e` comes from its pointer; a second pointer is ignored. */
  const pressOf = (e: PointerEvent<SVGSVGElement>) => {
    const cur = pressRef.current;
    return cur && cur.pointer === e.pointerId ? cur : null;
  };
  const down = (e: PointerEvent<SVGSVGElement>) => {
    if (e.button !== 0 || total <= 0) return;
    // A press still holding its pointer owns the ruler. One that never got capture (or whose
    // release landed elsewhere) is stale, and this press takes over.
    const cur = pressRef.current;
    if (cur && e.currentTarget.hasPointerCapture?.(cur.pointer)) return;
    capture(e);
    const bar = barAt(e);
    set({ pointer: e.pointerId, x0: e.clientX, from: bar, to: bar, drag: false });
  };
  const move = (e: PointerEvent<SVGSVGElement>) => {
    const cur = pressOf(e);
    if (!cur) return;
    const drag = cur.drag || Math.abs(e.clientX - cur.x0) >= DRAG_SLOP;
    const to = barAt(e);
    if (drag !== cur.drag || to !== cur.to) set({ ...cur, to, drag });
  };
  const up = (e: PointerEvent<SVGSVGElement>) => {
    const cur = pressOf(e);
    if (!cur) return;
    release(e);
    set(null);
    if (!cur.drag) {
      onSeek(cur.from);
      return;
    }
    const to = barAt(e);
    onLoop([Math.min(cur.from, to), Math.min(total, Math.max(cur.from, to) + 1)]);
  };
  const cancel = (e: PointerEvent<SVGSVGElement>) => {
    if (!pressOf(e)) return;
    release(e);
    set(null);
  };
  // The browser took the pointer away (a system gesture, say): drop the press, loop nothing.
  const lost = (e: PointerEvent<SVGSVGElement>) => {
    if (pressOf(e)) set(null);
  };

  const preview: [number, number] | null =
    press?.drag === true
      ? [Math.min(press.from, press.to), Math.min(total, Math.max(press.from, press.to) + 1)]
      : null;

  return (
    <svg
      data-testid="ruler"
      width={laneW}
      height={RULER_H}
      className="block cursor-pointer touch-none border-b border-line"
      style={{
        backgroundImage:
          "repeating-linear-gradient(90deg, rgb(255 255 255 / .22) 0 1px, transparent 1px 100%)",
        backgroundSize: `${ppb}px 5px`,
        backgroundRepeat: "repeat-x",
        backgroundPosition: "0 100%",
      }}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={cancel}
      onLostPointerCapture={lost}
    >
      {preview ? (
        <LoopBand loop={preview} ppb={ppb} height={RULER_H} testId="loop-preview" />
      ) : (
        <LoopBand loop={loop} ppb={ppb} height={RULER_H} />
      )}
      {loop && !preview && (
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
      <g aria-hidden="true" pointerEvents="none">
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

/** How the cap turns a pointer into a bar: the nearest bar line, or the bar under it. */
type CapSnap = "nearest" | "under";

/** The cap's hit area, centred on the playhead; the visible tab inside it is smaller. */
const CAP_W = 16;
const CAP_TAB = 12;

type PlayheadCapProps = {
  /** The playhead's x in the lanes. */
  x: number;
  /** The zero-based bar the playhead is in. */
  bar: number;
  total: number;
  /** The bar at a pointer's clientX: the nearest bar line, or the bar under it. */
  barAt(clientX: number, snap: CapSnap): number;
  /** The bar the cap is dragged to, or null when the drag ends without a seek. */
  onDrag(bar: number | null): void;
  /** The drag ended on `bar`, which is about to be seeked: keep drawing the playhead there. */
  onDrop(bar: number): void;
  onSeek(bar: number): void;
};

/**
 * A grab tab on the playhead's top, in the ruler: drag it to move the playhead bar by bar (it
 * seeks on release), or focus it and step a bar with the arrow keys. A click on it seeks to the
 * bar under the pointer, as the ruler beneath it would.
 */
function PlayheadCap({ x, bar, total, barAt, onDrag, onDrop, onSeek }: PlayheadCapProps) {
  /** Where the drag started and the bar it has reached, if it has moved. */
  const drag = useRef<{ x0: number; bar: number | null } | null>(null);
  const onDragRef = useRef(onDrag);
  onDragRef.current = onDrag;
  // Unmounted mid-drag (the song emptied, say): let go, or the playhead stays frozen.
  useEffect(() => () => onDragRef.current(null), []);
  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    // The ruler under the cap must not start a loop drag.
    e.stopPropagation();
    capture(e);
    drag.current = { x0: e.clientX, bar: null };
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    if (d.bar === null && Math.abs(e.clientX - d.x0) < DRAG_SLOP) return;
    const next = barAt(e.clientX, "nearest");
    if (next === d.bar) return;
    d.bar = next;
    onDrag(next);
  };
  const up = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    e.stopPropagation();
    release(e);
    drag.current = null;
    if (d.bar !== null) {
      onDrop(d.bar);
      onSeek(d.bar);
      return;
    }
    // A press that never became a drag is a click on the ruler under the cap: seek there.
    onDrag(null);
    onSeek(barAt(e.clientX, "under"));
  };
  const cancel = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    release(e);
    drag.current = null;
    onDrag(null);
  };
  // Capture lost mid-drag: end the drag without seeking, so the playhead follows ticks again.
  const lost = () => {
    if (!drag.current) return;
    drag.current = null;
    onDrag(null);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    // The frame's Alt+arrow moves the selected section; on the cap, arrows move the playhead only.
    e.preventDefault();
    e.stopPropagation();
    onSeek(clampBar(bar + (e.key === "ArrowLeft" ? -1 : 1), total));
  };
  return (
    <div
      role="slider"
      aria-label="Playhead"
      aria-valuemin={1}
      aria-valuemax={total}
      aria-valuenow={bar + 1}
      aria-valuetext={`Bar ${bar + 1}`}
      tabIndex={0}
      className={cn(
        "pointer-events-auto absolute left-0 top-0 z-10 flex cursor-ew-resize touch-none justify-center rounded-sm",
        focusRing,
      )}
      // Moved by transform on its own composited layer every tick, like the playhead line.
      style={{
        width: CAP_W,
        height: RULER_H,
        transform: `translateX(${x - CAP_W / 2}px)`,
        willChange: "transform",
      }}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={cancel}
      onLostPointerCapture={lost}
      onKeyDown={onKeyDown}
    >
      <span
        aria-hidden="true"
        className="block rounded-t-[3px]"
        style={{
          width: CAP_TAB,
          height: 11,
          background: "rgb(var(--play))",
          clipPath: "polygon(0 0, 100% 0, 100% 60%, 50% 100%, 0 60%)",
        }}
      />
    </div>
  );
}

type TrackHeadersProps = Pick<ArrangementProps, "openTrack" | "onOpenTrack" | "onMix"> & {
  rows: Row[];
  laneLabels: Record<string, string>;
  /** Lanes have scrolled under the column. */
  fillColumn: boolean;
  /** Rows have scrolled under the Bar and Sections labels. */
  fillTop: boolean;
};

/**
 * The sticky header column: the ruler and sections labels, then one `.hdr` per track with its
 * `↳` lane labels under it, and a "Mix" header over the mix lanes.
 */
const TrackHeaders = memo(function TrackHeaders({
  fillColumn,
  fillTop,
  rows,
  laneLabels,
  openTrack,
  onOpenTrack,
  onMix,
}: TrackHeadersProps) {
  return (
    <div
      role="rowgroup"
      aria-label="Tracks"
      className={cn("sticky left-0 z-30 shrink-0 border-r border-line", fillColumn && stickyFill)}
      style={{ width: HEADER_W }}
    >
      <div aria-hidden="true" className={cn("sticky top-0 z-10", fillTop && stickyFill)}>
        <div className="flex items-center border-b border-line pl-3" style={{ height: RULER_H }}>
          <span className="lbl">Bar</span>
        </div>
        <div className="flex items-center border-b border-line pl-3" style={{ height: SECROW_H }}>
          <span className="lbl">Sections</span>
        </div>
      </div>
      {rows.map((r) => {
        if (r.kind === "mix") {
          return (
            <div
              key="mix"
              data-testid="mix-header"
              className="flex items-center border-b border-ink/5 pl-[15px]"
              style={{ height: r.h }}
            >
              <span className="text-[12.5px] font-bold leading-tight">Mix</span>
            </div>
          );
        }
        if (r.kind === "lane") {
          const label = laneLabels[`${r.track}.${r.param}`] ?? r.param;
          return (
            <div
              key={`${r.track}.${r.param}`}
              className="flex items-center border-b border-ink/5 pl-6 pr-2"
              style={{ height: r.h }}
            >
              <span className="lbl truncate" style={{ color: r.color }}>
                {`↳ ${label}`}
              </span>
            </div>
          );
        }
        const t = r.track;
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

type BlocksProps = Pick<
  ArrangementProps,
  "sections" | "patterns" | "highlighted" | "selected" | "onSelect" | "onShowTrack"
> & {
  rows: Row[];
  drag: Drag | null;
  ppb: number;
};

/**
 * A block per section and track that plays a pattern: the track colour, the pattern's glyph, its
 * id and a `×N` badge when N sections play it. An empty cell shows a dashed outline on hover.
 * Both show the track in the dock on that section, never closing it. Memoised away from the
 * playhead.
 */
const Blocks = memo(function Blocks({
  sections,
  rows,
  patterns,
  highlighted,
  selected,
  drag,
  ppb,
  onSelect,
  onShowTrack,
}: BlocksProps) {
  const byId = new Map(patterns.map((pat) => [pat.id, pat]));
  const uses = new Map<string, number>();
  for (const s of sections) {
    for (const id of Object.values(s.play)) uses.set(id, (uses.get(id) ?? 0) + 1);
  }
  return (
    <>
      {rows.map((r) => {
        if (r.kind !== "track") return null;
        const t = r.track;
        const c = trackColorVar(t.instrument);
        return sections.map((s, i) => {
          const id = s.play[t.id];
          const w = Math.max(0, widthOf(s, i, drag, ppb) - 2 * INSET);
          const h = ROW_H - 2 * INSET;
          const box = {
            left: xOf(s, i, drag, ppb) + INSET,
            top: TOP + r.y + INSET,
            width: w,
            height: h,
          };
          const lit = blockMark(highlighted, i, t.id, id);
          const ring = lit && (
            <rect
              data-testid="block-highlight"
              data-highlight={lit}
              x={0.75}
              y={0.75}
              width={Math.max(0, w - 1.5)}
              height={h - 1.5}
              rx={9}
              fill="none"
              {...markStroke(lit)}
              strokeWidth={1.5}
            />
          );
          const open = () => {
            onSelect(i);
            onShowTrack(t.id);
          };
          if (id === undefined) {
            return (
              <div
                key={`${i}:${t.id}`}
                aria-hidden="true"
                data-testid={`empty-${i}-${t.id}`}
                onClick={open}
                className="absolute cursor-pointer rounded-rs border border-dashed border-transparent hover:border-line"
                style={box}
              >
                {ring && (
                  <svg width={w} height={h} className="absolute -left-px -top-px">
                    {ring}
                  </svg>
                )}
              </div>
            );
          }
          const pat = byId.get(id);
          const n = uses.get(id) ?? 0;
          return (
            <button
              key={`${i}:${t.id}`}
              type="button"
              aria-label={`${id} in ${s.name}`}
              title={id}
              onClick={open}
              className={cn(
                "absolute cursor-pointer overflow-hidden rounded-rs border-0 bg-transparent p-0 text-left",
                focusRing,
              )}
              style={box}
            >
              <svg width={w} height={h} className="absolute left-0 top-0" aria-hidden="true">
                <rect
                  data-testid="block-fill"
                  x={0.5}
                  y={0.5}
                  width={Math.max(0, w - 1)}
                  height={h - 1}
                  rx={9}
                  fill={`rgb(var(${c}) / ${selected === i ? ".32" : ".22"})`}
                  stroke={`rgb(var(${c}) / .45)`}
                />
                {pat && (
                  <Glyph kind={pat.kind} glyph={pat.glyph} w={w} h={h} color={`rgb(var(${c}))`} />
                )}
                {ring}
              </svg>
              <span
                className="absolute left-1.5 right-0 top-[3px] truncate text-[10.5px] font-bold leading-[14px] text-ink"
                style={{ paddingRight: n > 1 ? 30 : 6 }}
              >
                {id}
              </span>
              {n > 1 && (
                <span className="badge absolute right-1 top-[3px] bg-bg/[.35] leading-[13px]">
                  ×{n}
                </span>
              )}
            </button>
          );
        });
      })}
    </>
  );
});

type GlyphProps = { kind: "acid" | "drum"; glyph: number[]; w: number; h: number; color: string };

/**
 * A pattern's shape across its block. Drums: a bar per step, `value` × 60 % of the block tall.
 * Acid: a 2 px dash per note, raised by its value (accents sit higher) but kept under the id.
 */
function Glyph({ kind, glyph, w, h, color }: GlyphProps) {
  const inner = w - 2 * GLYPH_PAD;
  if (glyph.length === 0 || inner <= 0) return null;
  const step = inner / glyph.length;
  const barW = Math.max(1, step - 1.5);
  const base = h - INSET;
  return (
    <g fill={color}>
      {glyph.map((v, k) => {
        if (v <= 0) return null;
        const x = GLYPH_PAD + k * step;
        // Dashes rise less than bars: an accent at 60 % would sit on the pattern id's line.
        const rise = v * (kind === "drum" ? 0.6 : ACID_RISE) * h;
        return kind === "drum" ? (
          <rect
            key={k}
            data-testid="glyph-step"
            x={x}
            y={base - rise}
            width={barW}
            height={rise}
            rx={1}
            opacity={v >= 1 ? 1 : 0.7}
          />
        ) : (
          <rect
            key={k}
            data-testid="glyph-step"
            x={x}
            y={base - rise}
            width={barW}
            height={2}
            rx={1}
            opacity={v >= 1 ? 1 : 0.6}
          />
        );
      })}
    </g>
  );
}

type LaneOverlayProps = Pick<ArrangementProps, "sections"> & {
  rows: Row[];
  ppb: number;
  width: number;
  height: number;
};

/** Automation curves, each in its own lane row and its track's colour; clicks pass through. */
const LaneOverlay = memo(function LaneOverlay({
  sections,
  rows,
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
      {rows.map((r) => {
        if (r.kind !== "lane") return null;
        const top = TOP + r.y;
        return (
          <g key={`${r.track}.${r.param}`}>
            {sections.map((s, i) =>
              s.lanes
                .filter((l) => l.track === r.track && l.param === r.param && l.points.length > 0)
                .map((l) => (
                  <polyline
                    key={i}
                    data-lane={l.param}
                    fill="none"
                    stroke={r.color}
                    strokeOpacity={0.85}
                    strokeWidth={1.25}
                    points={holdPoints(l.points, s.bars)
                      .map(([bar, v]) => {
                        const y = top + LANE_H - 3 - Math.min(1, Math.max(0, v)) * (LANE_H - 6);
                        return `${(s.start_bar + bar) * ppb},${y}`;
                      })
                      .join(" ")}
                  />
                )),
            )}
          </g>
        );
      })}
    </svg>
  );
});
