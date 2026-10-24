import {
  FxPanel,
  Knob,
  TrackStrip,
  trackColor,
  type Highlight,
  type ParamSpecView,
  type SectionView,
} from "@tune/ui";
import { memo, useMemo } from "react";
import type { Command, ParamView, SongView, TrackView } from "../../client/protocol";
import { whyNoDuck } from "../../lib/song";
import type { HighlightKind } from "../../store/studioStore";
import { useHelp } from "../help";
import { useParamDrag } from "../useParamDrag";

/** Said on a bus nobody sends to: its knobs work, but only a release builds the bus. */
export const UNUSED = "No track sends here yet — sends start after the first release";

/** Said on the limiter's knobs in clip mode: they are written, but nothing hears them yet. */
export const LIMITER_OFF = "Off in clip mode";

/** The line under the master knobs: which track the sidechain follows, if any. */
export const duckText = (source: string | null): string =>
  source ? `Ducking follows: ${source}` : "Ducking: no kick to follow";

/** The gain drives either master stage; ceiling and release only the limiter. */
export function splitMaster(params: ParamView[]): { gain: ParamView[]; limiter: ParamView[] } {
  return {
    gain: params.filter((p) => p.id === "master.gain"),
    limiter: params.filter((p) => p.id !== "master.gain"),
  };
}

/** One mixer channel; re-renders only when its track changes. */
export const MixerStrip = memo(function MixerStrip({
  track,
  noDuck,
  fire,
  send,
  highlights,
  lanes,
  onHelp,
  compact,
}: {
  track: TrackView;
  noDuck: string | null;
  fire(cmd: Command): void;
  send(cmd: Command): Promise<unknown>;
  highlights: Map<string, HighlightKind>;
  lanes: SectionView["lanes"] | undefined;
  onHelp(spec: ParamSpecView | null): void;
  compact?: boolean;
}) {
  const drag = useParamDrag(track.id, send, track.strip);
  const strip = track.strip.map((p) => ({ ...p, value: drag.valueOf(p) }));
  return (
    <TrackStrip
      track={track}
      strip={strip}
      compact={compact}
      color={trackColor(track.instrument)}
      onParam={(id, v, commit) => (commit ? drag.commit(id, v) : drag.change(id, v))}
      onHelp={onHelp}
      noDuck={noDuck}
      automated={(id) => lanes?.some((l) => l.track === track.id && l.param === id) ?? false}
      highlight={(id) => highlights.get(`tracks.${track.id}.${id}`) ?? null}
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

export type MixerTabProps = {
  song: SongView;
  /** The section at the playhead (or cursor): its lanes mark knobs as automated. */
  automatedIn: number | null;
  highlights: Map<string, HighlightKind>;
  fire(cmd: Command): void;
  send(cmd: Command): Promise<unknown>;
};

/**
 * The dock's Mixer tab: a compact strip per track, the Delay and Reverb buses, and the master
 * (its mode is shown, not switchable). The row scrolls sideways inside itself.
 */
export const MixerTab = memo(function MixerTab({
  song,
  automatedIn,
  highlights,
  fire,
  send,
}: MixerTabProps) {
  const { mix } = song;
  const help = useHelp();
  const lanes = automatedIn === null ? undefined : song.sections[automatedIn]?.lanes;
  const all = useMemo(
    () => [...mix.delay, ...mix.reverb, ...mix.master_params],
    [mix.delay, mix.reverb, mix.master_params],
  );
  const drag = useParamDrag("mix", send, all);
  const live = (params: ParamView[]) => params.map((p) => ({ ...p, value: drag.valueOf(p) }));
  const onParam = (id: string, v: number, commit: boolean) =>
    commit ? drag.commit(id, v) : drag.change(id, v);
  const automated = (id: string) =>
    lanes?.some((l) => l.track === "mix" && l.param === id) ?? false;
  const highlight = (id: string): Highlight => highlights.get(`mix.${id}`) ?? null;

  const bus = (title: string, params: ParamView[], active: boolean) => (
    <FxPanel
      title={title}
      params={live(params)}
      inactiveNote={active ? null : UNUSED}
      onParam={onParam}
      highlight={highlight}
      automated={automated}
      onHelp={help.show}
    />
  );

  const { gain, limiter } = splitMaster(mix.master_params);
  const limiting = mix.master === "limit";
  const knob = (p: ParamView) => (
    <Knob
      key={p.id}
      spec={p}
      size="sm"
      value={drag.valueOf(p)}
      highlight={highlight(p.id)}
      automated={automated(p.id)}
      onHelp={help.show}
      onChange={(v) => onParam(p.id, v, false)}
      onCommit={(v) => onParam(p.id, v, true)}
    />
  );

  return (
    <div className="flex min-h-0 items-stretch gap-3 overflow-x-auto overflow-y-hidden p-3">
      {song.tracks.map((t) => (
        <MixerStrip
          key={t.id}
          compact
          track={t}
          noDuck={whyNoDuck(mix.duck_source, t.id)}
          fire={fire}
          send={send}
          highlights={highlights}
          lanes={lanes}
          onHelp={help.show}
        />
      ))}
      <div className="w-px shrink-0 self-stretch bg-line" aria-hidden="true" />
      {bus("Delay", mix.delay, mix.delay_active)}
      {bus("Reverb", mix.reverb, mix.reverb_active)}
      <div className="w-px shrink-0 self-stretch bg-line" aria-hidden="true" />
      <div
        role="group"
        aria-label="Master"
        className="flex shrink-0 flex-col gap-2 rounded-rs border border-line2 bg-ink/[.06] px-2.5 py-2"
      >
        <div className="flex items-center justify-between gap-3">
          <span className="text-xs font-bold leading-none text-ink">Master</span>
          <span className="flex items-baseline gap-1.5">
            <span className="lbl">Mode</span>
            <span className="text-[11px] font-bold leading-none text-ink">
              {limiting ? "Limit" : "Clip"}
            </span>
          </span>
        </div>
        <div className="flex flex-nowrap items-start gap-1">
          {gain.map(knob)}
          <div
            role="group"
            aria-label="Limiter"
            data-inactive={limiting ? undefined : "true"}
            className="flex flex-col gap-1"
          >
            <div
              className={`flex flex-nowrap items-start gap-1 transition-opacity ${limiting ? "" : "opacity-50"}`}
            >
              {limiter.map(knob)}
            </div>
            {!limiting && <span className="text-[11px] leading-none text-ink3">{LIMITER_OFF}</span>}
          </div>
        </div>
        <p className="text-[10px] leading-tight text-ink2">{duckText(mix.duck_source)}</p>
      </div>
    </div>
  );
});
