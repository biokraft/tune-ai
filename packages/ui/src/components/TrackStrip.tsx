import { useState } from "react";
import { cn } from "../lib/utils";
import type { ParamSpecView } from "../lib/param";
import { Knob, type Highlight } from "./Knob";

export type TrackStripProps = {
  track: { id: string; gain_db: number; pan: number; mute: boolean; solo: boolean };
  onMix(patch: { gain_db?: number; pan?: number; mute?: boolean; solo?: boolean }): void;
  /**
   * STRIP params; the strip shows knobs for send.delay, send.reverb and duck.depth. Omit to hide
   * them. The values are shown as given, so the caller overlays a held knob's live value.
   */
  strip?: ParamSpecView[];
  /** Live while dragging (commit false), written on release (commit true). */
  onParam?(id: string, value: number, commit: boolean): void;
  highlight?(id: string): Highlight;
};

/** The strip params that get a knob on the channel, in panel order. */
const STRIP_KNOBS = ["send.delay", "send.reverb", "duck.depth"];

const base = { help: "", value: 0, explicit: false, curve: "linear" as const };
const GAIN: ParamSpecView = {
  ...base,
  id: "gain_db",
  label: "Gain",
  unit: "dB",
  min: -60,
  max: 12,
  default: 0,
};
const PAN: ParamSpecView = {
  ...base,
  id: "pan",
  label: "Pan",
  unit: "",
  min: -1,
  max: 1,
  default: 0,
};

const toggle = cn(
  "h-7 w-7 rounded-sm border border-border font-label text-sm leading-none text-silkscreen",
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen",
);

export function TrackStrip({ track, onMix, strip, onParam, highlight }: TrackStripProps) {
  // Live values while a knob is held; cleared on commit so the prop takes over again.
  const [gain, setGain] = useState<number | null>(null);
  const [pan, setPan] = useState<number | null>(null);
  return (
    <div role="group" aria-label={`Track ${track.id}`} className="flex items-center gap-2">
      <span className="w-16 font-label text-base text-silkscreen">{track.id}</span>
      <Knob
        spec={GAIN}
        size="sm"
        value={gain ?? track.gain_db}
        onChange={setGain}
        onCommit={(v) => {
          setGain(null);
          onMix({ gain_db: v });
        }}
      />
      <Knob
        spec={PAN}
        size="sm"
        value={pan ?? track.pan}
        onChange={setPan}
        onCommit={(v) => {
          setPan(null);
          onMix({ pan: v });
        }}
      />
      {strip && (
        <div
          role="group"
          aria-label={`${track.id} sends`}
          className="flex items-center border-l border-silkscreen/20 pl-1"
        >
          {STRIP_KNOBS.map((id) => {
            const spec = strip.find((p) => p.id === id);
            if (!spec) return null;
            return (
              <Knob
                key={id}
                spec={spec}
                size="sm"
                value={spec.value}
                highlight={highlight?.(id) ?? null}
                onChange={(v) => onParam?.(id, v, false)}
                onCommit={(v) => onParam?.(id, v, true)}
              />
            );
          })}
        </div>
      )}
      <button
        type="button"
        title="Mute"
        aria-pressed={track.mute}
        className={cn(toggle, track.mute && "border-led bg-led text-chassis")}
        onClick={() => onMix({ mute: !track.mute })}
      >
        M
      </button>
      <button
        type="button"
        title="Solo"
        aria-pressed={track.solo}
        className={cn(toggle, track.solo && "border-slide bg-slide text-chassis")}
        onClick={() => onMix({ solo: !track.solo })}
      >
        S
      </button>
    </div>
  );
}
