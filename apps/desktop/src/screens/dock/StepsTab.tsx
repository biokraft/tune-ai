import {
  AcidStepSequencer,
  DrumGrid,
  trackColor,
  type AcidStep,
  type AcidStepSequencerProps,
  type DrumRow,
} from "@tune/ui";
import { memo, useMemo } from "react";
import type { Command, SongView, TrackView } from "../../client/protocol";
import { highlightedSteps, patternIn, stepInPattern } from "../../lib/song";
import type { HighlightKind } from "../../store/studioStore";
import { usePlayheadStep } from "../../store/useStudio";

export type StepsTabProps = {
  song: SongView;
  /** The docked track. */
  track: TrackView;
  /** The section whose patterns are shown. */
  section: number | null;
  highlights: Map<string, HighlightKind>;
  fire(cmd: Command): void;
  send(cmd: Command): Promise<unknown>;
};

type LiveSequencerProps = Omit<AcidStepSequencerProps, "playingStep"> & {
  song: SongView;
  section: number | null;
};

/** The step sequencer, lighting the sounding step as the playhead moves. */
function LiveSequencer({ song, section, ...rest }: LiveSequencerProps) {
  const step = usePlayheadStep();
  return (
    <AcidStepSequencer
      {...rest}
      playingStep={stepInPattern(song, section, step, rest.steps.length)}
    />
  );
}

/** Every drum track's pattern in `section`; the `docked` track's row is emphasised. */
function drumRows(
  song: SongView,
  section: number | null,
  highlights: Map<string, HighlightKind>,
  docked: string | null = null,
): DrumRow[] {
  return song.tracks.flatMap((t) => {
    const p = patternIn(song, section, t.id);
    if (p?.kind !== "drum") return [];
    return [
      {
        pattern: p.id,
        label: t.id,
        hits: p.hits,
        highlighted: highlightedSteps(highlights, p.id),
        color: trackColor(t.instrument),
        emphasis: t.id === docked,
      },
    ];
  });
}

/** The drum grid, chasing the playhead. */
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

const empty = "p-4 text-[13px] text-ink2";

/** The dock's Steps tab: the 303 sequencer, or the drum grid with the docked voice emphasised. */
export const StepsTab = memo(function StepsTab({
  song,
  track,
  section,
  highlights,
  fire,
  send,
}: StepsTabProps) {
  const rows = useMemo(
    () => drumRows(song, section, highlights, track.id),
    [song, section, highlights, track.id],
  );
  if (section === null) {
    return <p className={empty}>Move the playhead into a section to see its steps.</p>;
  }
  const pattern = patternIn(song, section, track.id);
  if (!pattern) {
    return (
      <p className={empty}>
        {track.id} plays nothing in {song.sections[section]?.name}. Pick a pattern above.
      </p>
    );
  }
  if (pattern.kind === "acid") {
    return (
      <div className="min-h-0 overflow-x-auto p-3">
        <LiveSequencer
          song={song}
          section={section}
          patternId={pattern.id}
          steps={pattern.steps}
          color={trackColor(track.instrument)}
          highlighted={highlightedSteps(highlights, pattern.id)}
          onChange={(index: number, s: AcidStep) =>
            void send({
              cmd: "set_acid_step",
              args: { pattern: pattern.id, index, note: s.note, accent: s.accent, slide: s.slide },
            }).catch(() => {})
          }
        />
      </div>
    );
  }
  return (
    <div className="min-h-0 overflow-auto p-3">
      <LiveDrumGrid song={song} section={section} rows={rows} fire={fire} />
    </div>
  );
});
