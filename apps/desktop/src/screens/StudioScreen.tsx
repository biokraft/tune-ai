import {
  Arrangement,
  DrumGrid,
  TrackStrip,
  TransportBar,
  type ArrangementProps,
  type DrumRow,
  type TransportBarProps,
} from "@tune/ui";
import { memo, useCallback, useMemo } from "react";
import type { Command, SongView, StudioState, TrackView } from "../client/protocol";
import {
  currentSection,
  formatNumber,
  highlightedSteps,
  patternIn,
  playheadStep,
  sectionAtBar,
  sectionBars,
  stepInPattern,
  STEPS_PER_BAR,
} from "../lib/song";
import { useStudioKeys } from "../lib/useStudioKeys";
import type { HighlightKind, StudioStore, StudioView } from "../store/studioStore";
import { usePlayheadPosition, usePlayheadStep, useStudioSelector } from "../store/useStudio";
import { FileErrorBanner, LostBanner, MessageBanner } from "./Banners";
import { ChangeNotice, hasChangeNotice } from "./ChangeNotice";
import { Panel303 } from "./Panel303";

export type StudioScreenProps = {
  view: StudioView & { state: StudioState & { song: SongView } };
  store: StudioStore;
};

const sameRange = (a: [number, number] | null, b: [number, number] | null) =>
  a !== null && b !== null && a[0] === b[0] && a[1] === b[1];

/**
 * The studio: transport on top, the arrangement under it, the 303 faceplate with its sequencer
 * beside the mixer, and the drum grid at the bottom.
 */
export function StudioScreen({ view, store }: StudioScreenProps) {
  const { state, highlights, notice } = view;
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

  useStudioKeys({
    playStop: () =>
      fire(state.playing ? { cmd: "stop" } : { cmd: "play", args: { from_bar: null } }),
    undo: () => fire({ cmd: "undo" }),
    redo: () => fire({ cmd: "redo" }),
    loop: toggleLoop,
  });

  const highlightedPaths = useMemo(() => new Set(highlights.keys()), [highlights]);
  const tracks = useMemo(
    () => song.tracks.map((t) => ({ id: t.id, instrument: t.instrument })),
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
    }),
    [store, fire],
  );

  const showNotice = hasChangeNotice(notice, state.offered);

  return (
    <div className="flex min-h-screen min-w-[1280px] flex-col bg-chassis text-silkscreen">
      {view.connection === "lost" && <LostBanner />}
      {state.file_errors.length > 0 && <FileErrorBanner errors={state.file_errors} />}
      {view.message && <MessageBanner message={view.message} onDismiss={store.dismissMessage} />}

      <header className="border-b border-black/40 px-6 py-3">
        <LiveTransport
          cursorBar={state.cursor_bar}
          playing={state.playing}
          bpm={Number(formatNumber(song.bpm))}
          loopOn={loop !== null}
          canLoop={loop !== null || selectedBars !== null}
          canUndo={state.can_undo}
          canRedo={state.can_redo}
          songName={song.name}
          notice={
            showNotice ? (
              <ChangeNotice
                notice={notice}
                offered={state.offered}
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
            ) : undefined
          }
          onPlay={() => fire({ cmd: "play", args: { from_bar: null } })}
          onStop={() => fire({ cmd: "stop" })}
          onToggleLoop={toggleLoop}
          onUndo={() => fire({ cmd: "undo" })}
          onRedo={() => fire({ cmd: "redo" })}
        />
      </header>

      <section aria-label="Song arrangement" className="border-b border-black/40 py-2">
        <LiveArrangement
          sections={song.sections}
          tracks={tracks}
          patterns={patterns}
          cursorBar={state.cursor_bar}
          selected={view.selectedSection}
          loop={loop}
          highlighted={highlightedPaths}
          {...arrangementHandlers}
        />
      </section>

      <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-6 px-6 py-5">
        <Panel303
          song={song}
          section={section}
          automatedIn={automatedIn}
          highlights={highlights}
          send={store.send}
        />
        <aside aria-label="Mixer" className="flex flex-col gap-3">
          <h2 className="font-label text-lg leading-none text-silkscreen/80">Mixer</h2>
          {song.tracks.map((t) => (
            <MixerStrip key={t.id} track={t} fire={fire} />
          ))}
        </aside>
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

/** One mixer channel; re-renders only when its track changes. */
const MixerStrip = memo(function MixerStrip({
  track,
  fire,
}: {
  track: TrackView;
  fire(cmd: Command): void;
}) {
  return (
    <TrackStrip
      track={track}
      onMix={(patch) =>
        fire({
          cmd: "set_track_mix",
          args: {
            track: track.id,
            gain_db: patch.gain_db ?? null,
            pan: patch.pan ?? null,
            mute: patch.mute ?? null,
            solo: patch.solo ?? null,
          },
        })
      }
    />
  );
});

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
  const rows: DrumRow[] = useMemo(
    () =>
      song.tracks.flatMap((t) => {
        const p = patternIn(song, section, t.id);
        if (p?.kind !== "drum") return [];
        return [
          {
            pattern: p.id,
            label: t.id,
            hits: p.hits,
            highlighted: highlightedSteps(highlights, p.id),
          },
        ];
      }),
    [song, section, highlights],
  );
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

function LiveDrumGrid({
  song,
  section,
  rows,
  fire,
}: {
  song: SongView;
  section: number | null;
  rows: DrumRow[];
  fire(cmd: Command): void;
}) {
  const step = usePlayheadStep();
  const length = rows[0]?.hits.length ?? 0;
  // One chase for all rows; rows of different lengths would disagree, so show none then.
  const same = rows.every((r) => r.hits.length === length);
  return (
    <DrumGrid
      rows={rows}
      playingStep={same ? stepInPattern(song, section, step, length) : null}
      onSet={(pattern, index, hit) => fire({ cmd: "set_drum_step", args: { pattern, index, hit } })}
    />
  );
}
