import { useState } from "react";
import { cn } from "../lib/utils";
import type { ParamSpecView } from "../lib/param";
import { Fader } from "./Fader";
import { Knob, type Highlight } from "./Knob";
import { MuteSolo } from "./MuteSolo";

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
  /** The strip param is automated in the current section (a release there is not written). */
  automated?(id: string): boolean;
  /** Called with the spec on focus and with null on blur (drives a help strip). */
  onHelp?(spec: ParamSpecView | null): void;
  /**
   * Why this track cannot duck (it is the duck source, or there is none): the duck knob is left
   * out and this short reason shows in its place.
   */
  noDuck?: string | null;
  /** CSS colour of the fader fill and the knob arcs (the track colour). */
  color?: string;
  /** The narrow vertical strip used by the Mixer tab. */
  compact?: boolean;
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

export function TrackStrip({
  track,
  onMix,
  strip,
  onParam,
  highlight,
  automated,
  onHelp,
  noDuck = null,
  color,
  compact = false,
}: TrackStripProps) {
  // Live values while a knob is held; cleared on commit so the prop takes over again.
  const [gain, setGain] = useState<number | null>(null);
  const [pan, setPan] = useState<number | null>(null);
  const sends = strip && (
    <div
      role="group"
      aria-label={`${track.id} sends`}
      className={cn("flex items-start", compact && "flex-col items-center")}
    >
      {STRIP_KNOBS.map((id) => {
        if (id === "duck.depth" && noDuck) {
          return (
            <span key={id} className="w-14 pt-3 text-center text-[11px] leading-tight text-ink3">
              {noDuck}
            </span>
          );
        }
        const spec = strip.find((p) => p.id === id);
        if (!spec) return null;
        return (
          <div key={id} onPointerEnter={() => onHelp?.(spec)} onPointerLeave={() => onHelp?.(null)}>
            <Knob
              spec={spec}
              size="sm"
              color={color}
              value={spec.value}
              highlight={highlight?.(id) ?? null}
              automated={automated?.(id) ?? false}
              onHelp={onHelp}
              onChange={(v) => onParam?.(id, v, false)}
              onCommit={(v) => onParam?.(id, v, true)}
            />
          </div>
        );
      })}
    </div>
  );
  return (
    <div
      role="group"
      aria-label={`Track ${track.id}`}
      data-compact={compact ? "true" : undefined}
      className={cn(
        "flex items-start gap-3",
        compact && "w-[92px] flex-col items-center gap-2 text-center",
      )}
    >
      <Fader
        label="Gain"
        value={gain ?? track.gain_db}
        min={-60}
        max={12}
        defaultValue={0}
        unit="dB"
        color={color}
        onChange={setGain}
        onCommit={(v) => {
          setGain(null);
          onMix({ gain_db: v });
        }}
      />
      <div className={cn("flex flex-col gap-1", compact && "items-center")}>
        <div className="flex items-center gap-2">
          <span className="text-xs font-bold text-ink">{track.id}</span>
          <MuteSolo
            track={track.id}
            mute={track.mute}
            solo={track.solo}
            onMute={(v) => onMix({ mute: v })}
            onSolo={(v) => onMix({ solo: v })}
          />
        </div>
        <div className={cn("flex items-start", compact && "flex-col items-center")}>
          <div onPointerEnter={() => onHelp?.(PAN)} onPointerLeave={() => onHelp?.(null)}>
            <Knob
              spec={PAN}
              onHelp={onHelp}
              size="sm"
              color={color}
              value={pan ?? track.pan}
              onChange={setPan}
              onCommit={(v) => {
                setPan(null);
                onMix({ pan: v });
              }}
            />
          </div>
          {sends}
        </div>
      </div>
    </div>
  );
}
