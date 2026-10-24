import { FxPanel, type Highlight, type ParamSpecView } from "@tune/ui";
import { memo, useMemo, useState } from "react";
import type { Command, ParamView, SongView } from "../client/protocol";
import type { HighlightKind } from "../store/studioStore";
import { duckText, LIMITER_OFF, splitMaster, UNUSED } from "./dock/MixerTab";
import { HelpStrip } from "./HelpStrip";
import { useParamDrag } from "./useParamDrag";

export type MixPanelProps = {
  song: SongView;
  highlights: Map<string, HighlightKind>;
  /** The section at the playhead (or cursor): its lanes mark knobs as automated. */
  automatedIn: number | null;
  send(cmd: Command): Promise<unknown>;
  /** The knob in hand, when the parent also shows help for knobs outside this panel. */
  help?: ParamSpecView | null;
  onHelp?(spec: ParamSpecView | null): void;
};

const MASTER_NOTE = 'Master: soft clipper (set mix.master = "limit" in the song for the limiter)';

export { LIMITER_OFF };

/**
 * The shared effects and the master: delay and reverb buses, the master gain, and the limiter's
 * ceiling and release (dimmed in clip mode).
 */
export const MixPanel = memo(function MixPanel({
  song,
  highlights,
  automatedIn,
  send,
  help: sharedHelp,
  onHelp: setSharedHelp,
}: MixPanelProps) {
  const { mix } = song;
  const [ownHelp, setOwnHelp] = useState<ParamSpecView | null>(null);
  const help = setSharedHelp ? (sharedHelp ?? null) : ownHelp;
  const setHelp = setSharedHelp ?? setOwnHelp;
  const all = useMemo(
    () => [...mix.delay, ...mix.reverb, ...mix.master_params],
    [mix.delay, mix.reverb, mix.master_params],
  );
  const drag = useParamDrag("mix", send, all);
  const live = (params: ParamView[]) => params.map((p) => ({ ...p, value: drag.valueOf(p) }));
  const { gain, limiter } = splitMaster(mix.master_params);

  const lanes = automatedIn === null ? undefined : song.sections[automatedIn]?.lanes;
  const automated = (id: string) =>
    lanes?.some((l) => l.track === "mix" && l.param === id) ?? false;
  const highlight = (id: string): Highlight => highlights.get(`mix.${id}`) ?? null;

  const panel = (title: string, params: ParamView[], inactiveNote: string | null) => (
    <FxPanel
      title={title}
      params={live(params)}
      inactiveNote={inactiveNote}
      onParam={(id, v, commit) => (commit ? drag.commit(id, v) : drag.change(id, v))}
      highlight={highlight}
      automated={automated}
      onHelp={setHelp}
    />
  );

  return (
    <section aria-label="Effects and master" className="flex flex-col gap-3">
      <h2 className="font-label text-lg leading-none text-silkscreen/80">Effects</h2>
      {panel("Delay", mix.delay, mix.delay_active ? null : UNUSED)}
      {panel("Reverb", mix.reverb, mix.reverb_active ? null : UNUSED)}
      {panel("Master", gain, null)}
      {panel("Limiter", limiter, mix.master === "limit" ? null : LIMITER_OFF)}
      <HelpStrip spec={help} />
      <p className="font-value text-xs text-silkscreen/70">
        {mix.master === "limit" ? "Master: limiter" : MASTER_NOTE}
      </p>
      <p className="font-value text-xs text-silkscreen/70">{duckText(mix.duck_source)}</p>
    </section>
  );
});
