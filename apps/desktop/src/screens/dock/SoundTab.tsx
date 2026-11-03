import {
  FxPanel,
  instrumentTitle,
  Segmented,
  trackColor,
  TrackStrip,
  type ParamSpecView,
} from "@tune/ui";
import { memo, useMemo } from "react";
import type { Command, FxView, ParamView, SongView, TrackView } from "../../client/protocol";
import { laneValueAt, sentenceCase, whyNoDuck } from "../../lib/song";
import type { HighlightKind } from "../../store/studioStore";
import { useKnobStep } from "../../store/useStudio";
import { useHelp } from "../help";
import { useParamDrag } from "../useParamDrag";

/** The 303's large knobs, in order (short ids; the track's params are `acid303.<id>`). */
export const BIG = ["tuning", "cutoff", "resonance", "env_mod", "decay", "accent"];
/** The 303's small knobs, in order. */
export const SMALL = ["waveform", "slide_time", "volume"];

export type SoundTabProps = {
  song: SongView;
  track: TrackView;
  /** The section at the playhead (or cursor): its lanes mark knobs as automated. */
  automatedIn: number | null;
  highlights: Map<string, HighlightKind>;
  send(cmd: Command): Promise<unknown>;
  fire(cmd: Command): void;
};

const byShortId = (params: ParamView[], short: string) =>
  params.find((p) => p.id.slice(p.id.indexOf(".") + 1) === short);

const fxTitle = (kind: string) =>
  kind === "distortion" ? "Distortion" : kind === "eq" ? "EQ" : sentenceCase(kind);

/** The fx the Sound tab has knobs for: the first distortion and the first eq. */
const WITH_KNOBS = ["distortion", "eq"];

/** The ids of the fx in `chain` that get knobs; every other one is listed by name only. */
function knobbedFx(chain: FxView[]): Set<string> {
  return new Set(WITH_KNOBS.flatMap((kind) => chain.find((f) => f.kind === kind)?.id ?? []));
}

/** An effect the Sound tab has no knobs for: its title and device id, read-only. */
function ListedFx({ fx }: { fx: FxView }) {
  const title = fxTitle(fx.kind);
  return (
    <div
      role="group"
      aria-label={`${title} ${fx.id}`}
      className="flex shrink-0 flex-col rounded-rs border border-line bg-glass2"
    >
      <div className="flex items-center gap-2 border-b border-line px-2.5 py-2">
        <span className="text-xs font-bold leading-none text-ink">{title}</span>
        <span className="font-mono text-[11px] leading-none text-ink3">{fx.id}</span>
      </div>
      <p className="max-w-[150px] p-2.5 text-[11px] leading-snug text-ink3">
        No knobs here yet. Ask Claude to change it.
      </p>
    </div>
  );
}

/**
 * The Sound tab: the track's device chain, scrolling sideways inside itself: the instrument, one
 * card per effect, then the mixer strip.
 */
export const SoundTab = memo(function SoundTab({
  song,
  track,
  automatedIn,
  highlights,
  send,
  fire,
}: SoundTabProps) {
  const help = useHelp();
  const color = trackColor(track.instrument);

  const allParams = useMemo(
    () => [...track.params, ...track.fx.flatMap((f) => f.params), ...track.strip],
    [track.params, track.fx, track.strip],
  );
  const drag = useParamDrag(track.id, send, allParams);
  const step = useKnobStep();

  const lanes = automatedIn === null ? undefined : song.sections[automatedIn]?.lanes;
  const automated = (id: string) =>
    lanes?.some((l) => l.track === track.id && l.param === id) ?? false;
  const highlight = (id: string) => highlights.get(`tracks.${track.id}.${id}`) ?? null;
  const onParam = (id: string, v: number, commit: boolean) =>
    commit ? drag.commit(id, v) : drag.change(id, v);
  const live = (params: ParamView[]): ParamSpecView[] =>
    params.map((p) => ({
      ...p,
      label: sentenceCase(p.label),
      value: drag.valueOf(p, laneValueAt(song, track.id, p.id, step)),
    }));

  // The 303 shows its big knobs, then its small ones; any other instrument shows all at "sm".
  const acid = track.instrument === "acid303";
  const instrumentParams = acid
    ? [...BIG, ...SMALL].flatMap((id) => byShortId(track.params, id) ?? [])
    : track.params;
  const big = new Set(acid ? BIG.flatMap((id) => byShortId(track.params, id)?.id ?? []) : []);

  const strip = track.strip.map((p) => ({
    ...p,
    value: drag.valueOf(p, laneValueAt(song, track.id, p.id, step)),
  }));
  const knobbed = knobbedFx(track.fx);

  return (
    <div className="flex min-h-0 flex-1 items-stretch gap-3 overflow-x-auto overflow-y-hidden overscroll-none p-3">
      <FxPanel
        title={instrumentTitle(track.instrument)}
        params={live(instrumentParams)}
        color={color}
        sizeOf={(id) => (big.has(id) ? "md" : "sm")}
        onParam={onParam}
        highlight={highlight}
        automated={automated}
        onHelp={help.show}
      />
      {track.fx.map((fx) =>
        knobbed.has(fx.id) ? (
          <FxPanel
            key={fx.id}
            title={fxTitle(fx.kind)}
            params={live(fx.params)}
            color={color}
            onParam={onParam}
            highlight={highlight}
            automated={automated}
            onHelp={help.show}
            header={
              fx.kind === "distortion" ? (
                <Segmented
                  label="Distortion mode"
                  options={fx.modes.map((m) => ({ value: m, label: sentenceCase(m) }))}
                  value={fx.mode}
                  highlight={highlight(`${fx.id}.mode`)}
                  onChange={(mode) =>
                    fire({
                      cmd: "set_device_setting",
                      args: { track: track.id, device: fx.id, setting: "mode", value: mode },
                    })
                  }
                />
              ) : undefined
            }
          />
        ) : (
          <ListedFx key={fx.id} fx={fx} />
        ),
      )}
      <section
        aria-label="Mixer strip"
        className="flex flex-none flex-col rounded-rs border border-line bg-glass2"
      >
        <h3 className="border-b border-line px-2.5 py-2 text-xs font-bold leading-none text-ink">
          Mixer strip
        </h3>
        <div className="p-2.5">
          <TrackStrip
            track={track}
            strip={strip}
            color={color}
            noDuck={whyNoDuck(song.mix.duck_source, track.id)}
            onParam={onParam}
            onHelp={help.show}
            automated={automated}
            highlight={highlight}
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
        </div>
      </section>
    </div>
  );
});
