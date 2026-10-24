import { Arrangement, TransportBar, type ArrangementProps, type TransportBarProps } from "@tune/ui";
import { memo, useCallback, useEffect, useMemo, useReducer } from "react";
import type { Command, MixView, SongView, StudioState, TrackView } from "../client/protocol";
import {
  currentSection,
  formatNumber,
  loopSectionName,
  patternGlyph,
  playheadStep,
  sectionAtBar,
  sectionBars,
  sentenceCase,
  STEPS_PER_BAR,
} from "../lib/song";
import { useStudioKeys } from "../lib/useStudioKeys";
import type { StudioStore, StudioView } from "../store/studioStore";
import { usePlayheadPosition, usePlayheadStep, useStudioSelector } from "../store/useStudio";
import { FileErrorBanner, LostBanner, MessageBanner } from "./Banners";
import { Blobs } from "./Blobs";
import { ChangeNotice, hasChangeNotice } from "./ChangeNotice";
import { dockReducer, dockSection } from "./dock/dockState";
import { Dock } from "./dock/Dock";
import { HelpLine, HelpProvider } from "./help";

export type StudioScreenProps = {
  view: StudioView & { state: StudioState & { song: SongView } };
  store: StudioStore;
};

const sameRange = (a: [number, number] | null, b: [number, number] | null) =>
  a !== null && b !== null && a[0] === b[0] && a[1] === b[1];

/**
 * The studio, filling the window without ever scrolling it: the transport on top, the timeline
 * under it, the module dock (one track or the mixer) when open, and Claude's change toast floating
 * top-right.
 */
export function StudioScreen({ view, store }: StudioScreenProps) {
  const { state, highlights, notice, noticeUndoable } = view;
  const song = state.song;
  const fire = useCallback((cmd: Command) => void store.send(cmd).catch(() => {}), [store]);

  // Numbers that follow the playhead only when it crosses a section boundary.
  const section = useStudioSelector(store, currentSection);
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

  const [dock, dispatch] = useReducer(dockReducer, null);
  // A track that leaves the song (an external edit, say) takes the dock with it.
  useEffect(() => dispatch({ type: "prune", tracks: song.tracks.map((t) => t.id) }), [song.tracks]);
  const toggleMixer = useCallback(() => dispatch({ type: "toggleMixer" }), []);

  useStudioKeys({
    playStop: () =>
      fire(state.playing ? { cmd: "stop" } : { cmd: "play", args: { from_bar: null } }),
    undo,
    redo,
    loop: toggleLoop,
    mixer: toggleMixer,
    escape: () => dispatch({ type: "close" }),
  });

  const highlightedPaths = useMemo(() => new Set(highlights.keys()), [highlights]);
  const tracks = useMemo(
    () =>
      song.tracks.map((t) => ({ id: t.id, instrument: t.instrument, mute: t.mute, solo: t.solo })),
    [song.tracks],
  );
  const patterns = useMemo(
    () =>
      song.patterns.map((p) => ({
        id: p.id,
        track: p.track,
        kind: p.kind,
        glyph: patternGlyph(p),
      })),
    [song.patterns],
  );
  const laneLabels = useMemo(() => laneLabelsOf(song.tracks, song.mix), [song.tracks, song.mix]);
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
      onOpenTrack: (track: string) => dispatch({ type: "openTrack", track }),
    }),
    [store, fire],
  );

  const showNotice = hasChangeNotice(notice, state.offered);
  const openTrack = dock?.kind === "track" ? dock.track : null;

  return (
    <HelpProvider>
      <div className="relative isolate flex h-screen min-w-[1280px] flex-col gap-2 overflow-hidden bg-bg p-2 text-ink">
        <Blobs />
        {view.connection === "lost" && <LostBanner />}
        {state.file_errors.length > 0 && <FileErrorBanner errors={state.file_errors} />}
        {view.message && <MessageBanner message={view.message} onDismiss={store.dismissMessage} />}

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
          mixerOpen={dock?.kind === "mixer"}
          onToggleMixer={toggleMixer}
          onPlay={() => fire({ cmd: "play", args: { from_bar: null } })}
          onStop={() => fire({ cmd: "stop" })}
          onToggleLoop={toggleLoop}
          onUndo={undo}
          onRedo={redo}
        />

        <section
          aria-label="Song arrangement"
          className="glass flex min-h-0 flex-1 flex-col overflow-hidden rounded-r"
        >
          <LiveArrangement
            sections={song.sections}
            tracks={tracks}
            patterns={patterns}
            laneLabels={laneLabels}
            cursorBar={state.cursor_bar}
            selected={view.selectedSection}
            loop={loop}
            openTrack={openTrack}
            highlighted={highlightedPaths}
            {...arrangementHandlers}
          />
          {dock === null && <HelpLine />}
        </section>

        {dock !== null && (
          <Dock
            song={song}
            dock={dock}
            section={dockSection(state, view.selectedSection, section)}
            automatedIn={automatedIn}
            highlights={highlights}
            send={store.send}
            fire={fire}
            onAction={dispatch}
          />
        )}

        {showNotice && (
          <ChangeNotice
            notice={notice}
            offered={state.offered}
            canUndo={noticeUndoable}
            song={song}
            onUndo={undo}
            onResolve={(reapply) => {
              store.dismissNotice();
              fire({ cmd: "resolve_offered", args: { reapply } });
            }}
            onDismiss={store.dismissNotice}
          />
        )}
      </div>
    </HelpProvider>
  );
}

/**
 * The timeline's lane row labels, keyed `${track}.${param}`: a track's instrument, fx and strip
 * params, and the mix's under track `"mix"`. Delay and reverb params carry their bus name, since
 * both buses have a "Low cut" and a "Width".
 */
function laneLabelsOf(tracks: TrackView[], mix: MixView): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of tracks) {
    for (const p of [...t.params, ...t.fx.flatMap((f) => f.params), ...t.strip]) {
      out[`${t.id}.${p.id}`] = sentenceCase(p.label);
    }
  }
  for (const p of mix.delay) out[`mix.${p.id}`] = `Delay ${p.label.toLowerCase()}`;
  for (const p of mix.reverb) out[`mix.${p.id}`] = `Reverb ${p.label.toLowerCase()}`;
  for (const p of mix.master_params) out[`mix.${p.id}`] = sentenceCase(p.label);
  return out;
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
