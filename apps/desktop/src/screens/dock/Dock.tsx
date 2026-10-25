import { cn, IconButton, instrumentTitle, trackColorVar } from "@tune/ui";
import { memo, useRef, type KeyboardEvent } from "react";
import type { Command, SongView, TrackView } from "../../client/protocol";
import type { HighlightKind } from "../../store/studioStore";
import { HelpLine } from "../help";
import type { DockAction, DockState, DockTab } from "./dockState";
import { MixerTab } from "./MixerTab";
import { PatternPicker } from "./PatternPicker";
import { SoundTab } from "./SoundTab";
import { StepsTab } from "./StepsTab";

export type DockProps = {
  song: SongView;
  dock: Exclude<DockState, null>;
  /** The section the dock edits (patterns, steps). */
  section: number | null;
  /** The section at the playhead (or cursor): its lanes mark knobs as automated. */
  automatedIn: number | null;
  highlights: Map<string, HighlightKind>;
  send(cmd: Command): Promise<unknown>;
  fire(cmd: Command): void;
  onAction(a: DockAction): void;
};

const TABS: { tab: DockTab; label: string }[] = [
  { tab: "sound", label: "Sound" },
  { tab: "steps", label: "Steps" },
];

const PANEL_ID = "dock-panel";
const tabId = (tab: DockTab) => `dock-tab-${tab}`;

/**
 * The body: scrolls inside itself. Its tab never shrinks below its content, so a dock shorter than
 * the tab scrolls instead of clipping it.
 */
const scroller = "flex min-h-0 flex-1 flex-col overflow-auto [&>*]:shrink-0";
const cell = "flex flex-col [&>*]:shrink-0";

const head = "flex h-[46px] flex-none items-center gap-2.5 border-b border-line pl-3.5 pr-2.5";

/**
 * The module dock under the timeline: one track's sound and steps, or the mixer. Its body scrolls
 * inside itself, with the help line at the bottom. Only the live sequencer and drum grid inside it
 * follow the playhead.
 */
export const Dock = memo(function Dock({
  song,
  dock,
  section,
  automatedIn,
  highlights,
  send,
  fire,
  onAction,
}: DockProps) {
  const track = dock.kind === "track" ? song.tracks.find((t) => t.id === dock.track) : undefined;
  // The docked track just left the song; the screen prunes the dock right after this render.
  if (dock.kind === "track" && !track) return null;
  const tint = track ? `rgb(var(${trackColorVar(track.instrument)}) / .28)` : undefined;
  const close = () => onAction({ type: "close" });

  return (
    <section
      aria-label="Dock"
      className="dock-open glass relative flex h-[clamp(260px,40vh,360px)] flex-none flex-col overflow-hidden rounded-r"
      style={tint ? { borderColor: tint } : undefined}
    >
      {track && dock.kind === "track" ? (
        <>
          <TrackHeader
            song={song}
            track={track}
            tab={dock.tab}
            section={section}
            fire={fire}
            onAction={onAction}
            onClose={close}
          />
          <TrackBody
            song={song}
            track={track}
            tab={dock.tab}
            section={section}
            automatedIn={automatedIn}
            highlights={highlights}
            send={send}
            fire={fire}
          />
        </>
      ) : (
        <>
          <div className={head}>
            <h2 className="text-[12.5px] font-bold text-ink">Mixer</h2>
            <div className="flex-1" />
            <CloseButton onClose={close} />
          </div>
          <div className={scroller}>
            <MixerTab
              song={song}
              automatedIn={automatedIn}
              highlights={highlights}
              fire={fire}
              send={send}
            />
          </div>
        </>
      )}
      <HelpLine />
    </section>
  );
});

function CloseButton({ onClose }: { onClose(): void }) {
  return (
    <IconButton label="Close dock" kbd="Esc" size="sm" onClick={onClose}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M6 6l12 12M18 6L6 18" />
      </svg>
    </IconButton>
  );
}

type TrackHeaderProps = {
  song: SongView;
  track: TrackView;
  tab: DockTab;
  section: number | null;
  fire(cmd: Command): void;
  onAction(a: DockAction): void;
  onClose(): void;
};

/** Track chip, section and bars, the pattern it plays there, the 303's tabs, and close. */
function TrackHeader({ song, track, tab, section, fire, onAction, onClose }: TrackHeaderProps) {
  const color = trackColorVar(track.instrument);
  const sec = section === null ? undefined : song.sections[section];
  const pattern = sec?.play[track.id];
  const uses = pattern ? song.sections.filter((s) => s.play[track.id] === pattern).length : 0;
  const acid = track.instrument === "acid303";

  return (
    <div className={head}>
      <span
        data-testid="dock-track"
        className="inline-flex h-[26px] flex-none items-center gap-[7px] rounded-[13px] pl-2 pr-2.5 text-xs font-extrabold text-ink"
        style={{
          background: `rgb(var(${color}) / .14)`,
          boxShadow: `inset 0 0 0 1px rgb(var(${color}) / .45)`,
        }}
      >
        <i
          aria-hidden="true"
          className="h-2 w-2 rounded-full"
          style={{ background: `rgb(var(${color}))`, boxShadow: `0 0 8px rgb(var(${color}))` }}
        />
        <span>{track.id}</span>
        <span className="font-semibold text-ink2">{instrumentTitle(track.instrument)}</span>
      </span>
      {sec && section !== null && (
        <>
          <span className="truncate text-[11.5px] text-ink2">
            {`${sec.name} · bars ${sec.start_bar + 1}–${sec.start_bar + sec.bars}`}
          </span>
          <PatternPicker
            song={song}
            track={track.id}
            section={section}
            onPick={(p) =>
              fire({ cmd: "set_section_play", args: { section, track: track.id, pattern: p } })
            }
          />
          {uses > 1 && <span className="badge flex-none">×{uses}</span>}
        </>
      )}
      <div className="flex-1" />
      {acid && <Tabs track={track.id} tab={tab} onTab={(t) => onAction({ type: "tab", tab: t })} />}
      <CloseButton onClose={onClose} />
    </div>
  );
}

/** Sound | Steps, with arrow keys moving between them (the ARIA tabs pattern). */
function Tabs({ track, tab, onTab }: { track: string; tab: DockTab; onTab(t: DockTab): void }) {
  const refs = useRef<Partial<Record<DockTab, HTMLButtonElement | null>>>({});
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const i = TABS.findIndex((t) => t.tab === tab);
    const next = TABS[(i + (e.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length]!.tab;
    onTab(next);
    refs.current[next]?.focus();
  };
  return (
    <div role="tablist" aria-label={`${track} views`} className="flex flex-none gap-1">
      {TABS.map((t) => {
        const on = t.tab === tab;
        return (
          <button
            key={t.tab}
            ref={(el) => void (refs.current[t.tab] = el)}
            type="button"
            role="tab"
            id={tabId(t.tab)}
            aria-selected={on}
            aria-controls={PANEL_ID}
            tabIndex={on ? 0 : -1}
            onClick={() => onTab(t.tab)}
            onKeyDown={onKeyDown}
            className={cn(
              "inline-flex h-7 items-center rounded-[7px] px-[9px] text-xs font-bold",
              "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play",
              on ? "bg-ink/10 text-ink" : "text-ink2 hover:bg-ink/5",
            )}
          >
            {t.label}
          </button>
        );
      })}
    </div>
  );
}

type TrackBodyProps = Omit<DockProps, "dock" | "onAction"> & { track: TrackView; tab: DockTab };

/** The 303 shows its active tab; any other track shows its sound and steps side by side. */
function TrackBody({
  song,
  track,
  tab,
  section,
  automatedIn,
  highlights,
  send,
  fire,
}: TrackBodyProps) {
  const sound = (
    <SoundTab
      song={song}
      track={track}
      automatedIn={automatedIn}
      highlights={highlights}
      send={send}
      fire={fire}
    />
  );
  const steps = (
    <StepsTab
      song={song}
      track={track}
      section={section}
      highlights={highlights}
      fire={fire}
      send={send}
    />
  );
  if (track.instrument === "acid303") {
    return (
      <div role="tabpanel" id={PANEL_ID} aria-labelledby={tabId(tab)} className={scroller}>
        {tab === "sound" ? sound : steps}
      </div>
    );
  }
  return (
    <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)] overflow-auto">
      <div className={cn(cell, "min-w-0")}>{sound}</div>
      <div className={cn(cell, "min-w-0 border-l border-line")}>{steps}</div>
    </div>
  );
}
