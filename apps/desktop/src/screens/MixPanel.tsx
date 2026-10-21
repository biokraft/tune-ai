import { FxPanel, type Highlight, type ParamSpecView } from "@tune/ui";
import { memo, useMemo, useState } from "react";
import type { Command, ParamView, SongView } from "../client/protocol";
import type { HighlightKind } from "../store/studioStore";
import { HelpStrip } from "./HelpStrip";
import { useParamDrag } from "./useParamDrag";

export type MixPanelProps = {
  song: SongView;
  highlights: Map<string, HighlightKind>;
  /** The section at the playhead (or cursor): its lanes mark knobs as automated. */
  automatedIn: number | null;
  send(cmd: Command): Promise<unknown>;
};

/** Said on a bus nobody sends to: its knobs work, but only a release builds the bus. */
const UNUSED = "No track sends here yet — sends start after the first release";

const MASTER_NOTE = 'Master: soft clipper (set mix.master = "limit" in the song for the limiter)';

/** The shared effects and the master: delay and reverb buses, master gain, ceiling and release. */
export const MixPanel = memo(function MixPanel({
  song,
  highlights,
  automatedIn,
  send,
}: MixPanelProps) {
  const { mix } = song;
  const [help, setHelp] = useState<ParamSpecView | null>(null);
  const all = useMemo(
    () => [...mix.delay, ...mix.reverb, ...mix.master_params],
    [mix.delay, mix.reverb, mix.master_params],
  );
  const drag = useParamDrag("mix", send, all);
  const live = (params: ParamView[]) => params.map((p) => ({ ...p, value: drag.valueOf(p) }));

  const lanes = automatedIn === null ? undefined : song.sections[automatedIn]?.lanes;
  const automated = (id: string) =>
    lanes?.some((l) => l.track === "mix" && l.param === id) ?? false;
  const highlight = (id: string): Highlight => highlights.get(`mix.${id}`) ?? null;

  const panel = (title: string, params: ParamView[], inactive: boolean) => (
    <FxPanel
      title={title}
      params={live(params)}
      inactiveNote={inactive ? UNUSED : null}
      onParam={(id, v, commit) => (commit ? drag.commit(id, v) : drag.change(id, v))}
      highlight={highlight}
      automated={automated}
      onHelp={setHelp}
    />
  );

  return (
    <section aria-label="Effects and master" className="flex flex-col gap-3">
      <h2 className="font-label text-lg leading-none text-silkscreen/80">Effects</h2>
      {panel("Delay", mix.delay, !mix.delay_active)}
      {panel("Reverb", mix.reverb, !mix.reverb_active)}
      {panel("Master", mix.master_params, false)}
      <HelpStrip spec={help} />
      <p className="font-value text-xs text-silkscreen/70">
        {mix.master === "limit" ? "Master: limiter" : MASTER_NOTE}
      </p>
      <p className="font-value text-xs text-silkscreen/70">
        {mix.duck_source ? `Ducking follows: ${mix.duck_source}` : "Ducking: no kick to follow"}
      </p>
    </section>
  );
});
