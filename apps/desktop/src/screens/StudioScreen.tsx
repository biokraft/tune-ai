import { Arrangement, DrumGrid, TrackStrip, TransportBar, type DrumRow } from "@tune/ui";
import { useMemo } from "react";
import type { Command, SongView, StudioState } from "../client/protocol";
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
import type { StudioStore, StudioView } from "../store/studioStore";
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
  const fire = (cmd: Command) => void store.send(cmd).catch(() => {});

  const section = currentSection(view);
  const step = playheadStep(view);
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

  const position =
    step !== null && view.playhead
      ? { bar: view.playhead.bar, beat: view.playhead.beat }
      : { bar: state.cursor_bar, beat: 0 };

  const highlightedPaths = useMemo(() => new Set(highlights.keys()), [highlights]);
  const drumRows: DrumRow[] = song.tracks.flatMap((t) => {
    const p = patternIn(song, section, t.id);
    if (p?.kind !== "drum") return [];
    return [
      { pattern: p.id, label: t.id, hits: p.hits, highlighted: highlightedSteps(highlights, p.id) },
    ];
  });
  const drumLength = drumRows[0]?.hits.length ?? 0;
  const drumsSame = drumRows.every((r) => r.hits.length === drumLength);

  const showNotice = hasChangeNotice(notice, state.offered);

  return (
    <div className="flex min-h-screen min-w-[1280px] flex-col bg-chassis text-silkscreen">
      {view.connection === "lost" && <LostBanner />}
      {state.file_errors.length > 0 && <FileErrorBanner errors={state.file_errors} />}
      {view.message && <MessageBanner message={view.message} onDismiss={store.dismissMessage} />}

      <header className="border-b border-black/40 px-6 py-3">
        <TransportBar
          playing={state.playing}
          bpm={Number(formatNumber(song.bpm))}
          bar={position.bar}
          beat={position.beat}
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
        <Arrangement
          sections={song.sections}
          tracks={song.tracks.map((t) => ({ id: t.id, instrument: t.instrument }))}
          patterns={song.patterns.map((p) => ({ id: p.id, track: p.track }))}
          playhead={step !== null ? step / STEPS_PER_BAR : state.cursor_bar}
          selected={view.selectedSection}
          loop={loop}
          highlighted={highlightedPaths}
          onSelect={(i) => store.select(i)}
          onSeek={(bar) => fire({ cmd: "seek_bar", args: { bar } })}
          onMove={(from, to) => {
            fire({ cmd: "move_section", args: { from, to } });
            store.select(to);
          }}
          onResize={(index, bars) => fire({ cmd: "resize_section", args: { index, bars } })}
          onDuplicate={(index) => fire({ cmd: "duplicate_section", args: { index } })}
          onDelete={(index) => {
            fire({ cmd: "delete_section", args: { index } });
            store.select(null);
          }}
          onSetPlay={(sec, track, pattern) =>
            fire({ cmd: "set_section_play", args: { section: sec, track, pattern } })
          }
        />
      </section>

      <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-6 px-6 py-5">
        <Panel303
          song={song}
          section={section}
          automatedIn={sectionAtBar(
            song.sections,
            step !== null ? Math.floor(step / STEPS_PER_BAR) : state.cursor_bar,
          )}
          playingStep={step}
          highlights={highlights}
          send={store.send}
        />
        <aside aria-label="Mixer" className="flex flex-col gap-3">
          <h2 className="font-label text-lg leading-none text-silkscreen/80">Mixer</h2>
          {song.tracks.map((t) => (
            <TrackStrip
              key={t.id}
              track={t}
              onMix={(patch) =>
                fire({
                  cmd: "set_track_mix",
                  args: {
                    track: t.id,
                    gain_db: patch.gain_db ?? null,
                    pan: patch.pan ?? null,
                    mute: patch.mute ?? null,
                    solo: patch.solo ?? null,
                  },
                })
              }
            />
          ))}
        </aside>
      </div>

      <section
        aria-label="Drums"
        className="flex flex-col gap-3 border-t border-black/40 px-6 py-5"
      >
        <h2 className="font-label text-lg leading-none text-silkscreen/80">
          Drums
          {section !== null && song.sections[section] ? ` in ${song.sections[section]!.name}` : ""}
        </h2>
        {drumRows.length > 0 ? (
          <div className="overflow-x-auto pb-1">
            <DrumGrid
              rows={drumRows}
              playingStep={drumsSame ? stepInPattern(song, section, step, drumLength) : null}
              onSet={(pattern, index, hit) =>
                fire({ cmd: "set_drum_step", args: { pattern, index, hit } })
              }
            />
          </div>
        ) : (
          <p className="font-value text-sm text-silkscreen/70">
            This section plays no drum patterns. Pick some in the arrangement.
          </p>
        )}
      </section>
    </div>
  );
}
