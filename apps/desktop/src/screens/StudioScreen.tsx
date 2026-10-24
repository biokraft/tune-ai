import {
  Arrangement,
  DrumGrid,
  TrackStrip,
  TransportBar,
  type ArrangementProps,
  type DrumRow,
  type ParamSpecView,
  type SectionView,
  type TransportBarProps,
} from "@tune/ui";
import { memo, useCallback, useMemo, useState } from "react";
import type { Command, SongView, StudioState, TrackView } from "../client/protocol";
import {
  acidSection,
  currentSection,
  formatNumber,
  highlightedSteps,
  loopSectionName,
  patternIn,
  playheadStep,
  sectionAtBar,
  sectionBars,
  stepInPattern,
  STEPS_PER_BAR,
  whyNoDuck,
} from "../lib/song";
import { useStudioKeys } from "../lib/useStudioKeys";
import type { HighlightKind, StudioStore, StudioView } from "../store/studioStore";
import { usePlayheadPosition, usePlayheadStep, useStudioSelector } from "../store/useStudio";
import { FileErrorBanner, LostBanner, MessageBanner } from "./Banners";
import { ChangeNotice, hasChangeNotice } from "./ChangeNotice";
import { drumRows, LiveDrumGrid } from "./dock/StepsTab";
import { MixerStrip } from "./dock/MixerTab";
import { MixPanel } from "./MixPanel";
import { Panel303 } from "./Panel303";
import { useParamDrag } from "./useParamDrag";

export type StudioScreenProps = {
  view: StudioView & { state: StudioState & { song: SongView } };
  store: StudioStore;
};

const noop = () => {};

const sameRange = (a: [number, number] | null, b: [number, number] | null) =>
  a !== null && b !== null && a[0] === b[0] && a[1] === b[1];

/**
 * The studio: transport on top, the arrangement under it, the 303 faceplate with its sequencer
 * beside the mixer, and the drum grid at the bottom.
 */
export function StudioScreen({ view, store }: StudioScreenProps) {
  const { state, highlights, notice, noticeUndoable } = view;
  const song = state.song;
  const fire = useCallback((cmd: Command) => void store.send(cmd).catch(() => {}), [store]);

  // Numbers that follow the playhead only when it crosses a section boundary.
  const section = useStudioSelector(store, currentSection);
  const acidShown = useStudioSelector(store, acidSection);
  const automatedIn = useStudioSelector(store, soundingSection);
  const selectedBars =
    view.selectedSection === null ? null : sectionBars(state, view.selectedSection);
  const loop = state.loop_bars;

  const toggleLoop = () => {
    if (loop !== null && (selectedBars === null || sameRange(loop, selectedBars))) {
      fire({ cmd: "set_loop", args: { bars: null } });
    } else if (selectedBars !== null) {
      fire({ cmd: "set_loop", args: { bars: selectedBars } });
    }
  };

  // A deliberate undo or redo moves the history under Claude's notice, so the notice goes.
  const undo = () => {
    store.dismissNotice();
    fire({ cmd: "undo" });
  };
  const redo = () => {
    store.dismissNotice();
    fire({ cmd: "redo" });
  };

  useStudioKeys({
    playStop: () =>
      fire(state.playing ? { cmd: "stop" } : { cmd: "play", args: { from_bar: null } }),
    undo,
    redo,
    loop: toggleLoop,
  });

  const highlightedPaths = useMemo(() => new Set(highlights.keys()), [highlights]);
  const tracks = useMemo(
    () =>
      song.tracks.map((t) => ({ id: t.id, instrument: t.instrument, mute: t.mute, solo: t.solo })),
    [song.tracks],
  );
  const patterns = useMemo(
    () => song.patterns.map((p) => ({ id: p.id, track: p.track })),
    [song.patterns],
  );
  const arrangementHandlers = useMemo(
    () => ({
      onSelect: (i: number) => store.select(i),
      onSeek: (bar: number) => fire({ cmd: "seek_bar", args: { bar } }),
      onMove: (from: number, to: number) => {
        fire({ cmd: "move_section", args: { from, to } });
        store.select(to);
      },
      onResize: (index: number, bars: number) =>
        fire({ cmd: "resize_section", args: { index, bars } }),
      onDuplicate: (index: number) => fire({ cmd: "duplicate_section", args: { index } }),
      onDelete: (index: number) => {
        fire({ cmd: "delete_section", args: { index } });
        store.select(null);
      },
      onSetPlay: (sec: number, track: string, pattern: string | null) =>
        fire({ cmd: "set_section_play", args: { section: sec, track, pattern } }),
      onMix: (track: string, patch: { mute?: boolean; solo?: boolean }) =>
        fire({
          cmd: "set_track_mix",
          args: {
            track,
            gain_db: null,
            pan: null,
            mute: patch.mute ?? null,
            solo: patch.solo ?? null,
          },
        }),
      // Task 9 opens the track in the dock.
      onOpenTrack: noop,
    }),
    [store, fire],
  );

  const showNotice = hasChangeNotice(notice, state.offered);

  return (
    <div className="flex min-h-screen min-w-[1280px] flex-col bg-chassis text-silkscreen">
      {view.connection === "lost" && <LostBanner />}
      {state.file_errors.length > 0 && <FileErrorBanner errors={state.file_errors} />}
      {view.message && <MessageBanner message={view.message} onDismiss={store.dismissMessage} />}

      {/* Sticky, so play state and Claude's change notice stay in view over the drum grid. */}
      <header className="sticky top-0 z-30 px-3 pb-2 pt-3">
        <LiveTransport
          cursorBar={state.cursor_bar}
          playing={state.playing}
          bpm={Number(formatNumber(song.bpm))}
          loopOn={loop !== null}
          canLoop={loop !== null || selectedBars !== null}
          canUndo={state.can_undo}
          canRedo={state.can_redo}
          songName={song.name}
          totalBars={song.total_bars}
          loopBars={loop}
          loopName={loopSectionName(song.sections, loop)}
          mixerOpen={false}
          onToggleMixer={noop}
          onPlay={() => fire({ cmd: "play", args: { from_bar: null } })}
          onStop={() => fire({ cmd: "stop" })}
          onToggleLoop={toggleLoop}
          onUndo={undo}
          onRedo={redo}
        />
        {/* Task 9 turns this into a floating toast. */}
        {showNotice && (
          <div role="status" className="mt-2 font-mono text-sm text-agent">
            <ChangeNotice
              notice={notice}
              offered={state.offered}
              canUndo={noticeUndoable}
              song={song}
              onUndo={() => {
                store.dismissNotice();
                fire({ cmd: "undo" });
              }}
              onResolve={(reapply) => {
                store.dismissNotice();
                fire({ cmd: "resolve_offered", args: { reapply } });
              }}
              onDismiss={store.dismissNotice}
            />
          </div>
        )}
      </header>

      <section
        aria-label="Song arrangement"
        className="glass mx-3 flex min-h-0 flex-1 flex-col overflow-hidden rounded-r"
      >
        <LiveArrangement
          sections={song.sections}
          tracks={tracks}
          patterns={patterns}
          cursorBar={state.cursor_bar}
          selected={view.selectedSection}
          loop={loop}
          openTrack={null}
          highlighted={highlightedPaths}
          {...arrangementHandlers}
        />
      </section>

      <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-6 px-6 py-5">
        <Panel303
          song={song}
          section={acidShown}
          automatedIn={automatedIn}
          highlights={highlights}
          send={store.send}
        />
        <Mixer
          song={song}
          highlights={highlights}
          automatedIn={automatedIn}
          fire={fire}
          send={store.send}
        />
      </div>

      <DrumSection song={song} section={section} highlights={highlights} fire={fire} />
    </div>
  );
}

/** The section under the playhead (or the cursor): where a knob release would land. */
function soundingSection(v: StudioView): number | null {
  const song = v.state?.song;
  if (!song) return null;
  const step = playheadStep(v);
  return sectionAtBar(
    song.sections,
    step !== null ? Math.floor(step / STEPS_PER_BAR) : v.state!.cursor_bar,
  );
}

type LiveTransportProps = Omit<TransportBarProps, "bar" | "beat"> & { cursorBar: number };

/** The transport, following the playhead's bar and beat (it re-renders per beat, not per tick). */
function LiveTransport({ cursorBar, ...rest }: LiveTransportProps) {
  const at = usePlayheadPosition() ?? { bar: cursorBar, beat: 0 };
  return <TransportBar {...rest} bar={at.bar} beat={at.beat} />;
}

type LiveArrangementProps = Omit<ArrangementProps, "playhead"> & { cursorBar: number };

/** The arrangement, following the playhead step by step. */
const LiveArrangement = memo(function LiveArrangement({
  cursorBar,
  ...rest
}: LiveArrangementProps) {
  const step = usePlayheadStep();
  return <Arrangement {...rest} playhead={step !== null ? step / STEPS_PER_BAR : cursorBar} />;
});

type MixerProps = {
  song: SongView;
  highlights: Map<string, HighlightKind>;
  automatedIn: number | null;
  fire(cmd: Command): void;
  send(cmd: Command): Promise<unknown>;
};

/** The channel strips and the effects panel; one help line serves every knob in the column. */
function Mixer({ song, highlights, automatedIn, fire, send }: MixerProps) {
  const [help, setHelp] = useState<ParamSpecView | null>(null);
  const lanes = automatedIn === null ? undefined : song.sections[automatedIn]?.lanes;
  return (
    <aside aria-label="Mixer" className="flex flex-col gap-3">
      <h2 className="font-label text-lg leading-none text-silkscreen/80">Mixer</h2>
      {song.tracks.map((t) => (
        <MixerStrip
          key={t.id}
          track={t}
          noDuck={whyNoDuck(song.mix.duck_source, t.id)}
          fire={fire}
          send={send}
          highlights={highlights}
          lanes={lanes}
          onHelp={setHelp}
        />
      ))}
      <MixPanel
        song={song}
        highlights={highlights}
        automatedIn={automatedIn}
        send={send}
        help={help}
        onHelp={setHelp}
      />
    </aside>
  );
}

type DrumSectionProps = {
  song: SongView;
  section: number | null;
  highlights: Map<string, HighlightKind>;
  fire(cmd: Command): void;
};

/** The drum patterns the shown section plays; only the grid follows the playhead. */
const DrumSection = memo(function DrumSection({
  song,
  section,
  highlights,
  fire,
}: DrumSectionProps) {
  const rows = useMemo(() => drumRows(song, section, highlights), [song, section, highlights]);
  const name = section === null ? undefined : song.sections[section]?.name;
  return (
    <section aria-label="Drums" className="flex flex-col gap-3 border-t border-black/40 px-6 py-5">
      <h2 className="font-label text-lg leading-none text-silkscreen/80">
        Drums{name ? ` in ${name}` : ""}
      </h2>
      {rows.length > 0 ? (
        <div className="overflow-x-auto pb-1">
          <LiveDrumGrid song={song} section={section} rows={rows} fire={fire} />
        </div>
      ) : (
        <p className="font-value text-sm text-silkscreen/70">
          This section plays no drum patterns. Pick some in the arrangement.
        </p>
      )}
    </section>
  );
});
