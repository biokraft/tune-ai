import type { Change, Json, ParamView, SongView } from "../client/protocol";
import { changedSteps } from "../store/changedSteps";
import { formatNumber, sentenceCase } from "./song";

const show = (v: Json): string =>
  v === null ? "none" : typeof v === "number" ? formatNumber(v) : String(v);

/** `old → new` with the param's unit: "500 → 820 Hz", "70 → 90%". */
function span(old: Json, next: Json, unit = ""): string {
  const tail = unit === "" ? "" : unit === "%" ? "%" : ` ${unit}`;
  return `${show(old)} → ${show(next)}${tail}`;
}

function findParam(song: SongView | null, track: string, id: string): ParamView | null {
  const t = song?.tracks.find((x) => x.id === track);
  if (!t) return null;
  return (
    t.params.find((p) => p.id === id) ??
    t.strip.find((p) => p.id === id) ??
    t.fx.flatMap((f) => f.params).find((p) => p.id === id) ??
    null
  );
}

/** Whether `id` is a strip param (send or duck) of `track`. */
const isStrip = (song: SongView | null, track: string, id: string) =>
  song?.tracks.find((x) => x.id === track)?.strip.some((p) => p.id === id) ?? false;

/** A shared-mix param by id (`delay.feedback`), with the label it carries in a sentence. */
function findMixParam(
  song: SongView | null,
  id: string,
): { param: ParamView; label: string } | null {
  const m = song?.mix;
  const param = [...(m?.delay ?? []), ...(m?.reverb ?? []), ...(m?.master_params ?? [])].find(
    (p) => p.id === id,
  );
  if (!param) return null;
  // "Feedback" alone is ambiguous across buses: lead with the group ("Delay feedback").
  const group = sentenceCase(id.slice(0, id.indexOf(".")));
  const label = sentenceCase(param.label);
  return { param, label: label.startsWith(group) ? label : `${group} ${label.toLowerCase()}` };
}

/** Whether a param on `track` is named without its track: the song's only 303 is implied. */
function implied(song: SongView | null, track: string): boolean {
  const acids = song?.tracks.filter((t) => t.instrument === "acid303") ?? [];
  return acids.length === 1 && acids[0]!.id === track;
}

const MIX = /^mix\.(.+)$/;
const TRACK = /^tracks\.([^.]+)(?:\.(.+))?$/;
const SECTION = /^sections\[(\d+)\]\.(bars|name|lanes|play\.(.+))$/;
const PATTERN = /^patterns\.(.+)\.steps$/;

/** One change in a few plain words, for the change notice. */
export function describeChange(c: Change, song: SongView | null): string {
  if (c.path === "song.bpm") return `Tempo ${span(c.old, c.new, "BPM")}`;
  if (c.path === "sections") return "Arrangement";

  const pattern = PATTERN.exec(c.path);
  if (pattern) {
    const id = pattern[1]!;
    if (typeof c.old !== "string" || typeof c.new !== "string") {
      return c.old === null ? `Pattern ${id} added` : `Pattern ${id} removed`;
    }
    const n = changedSteps(c.old, c.new).length;
    return `${id} pattern (${n} ${n === 1 ? "step" : "steps"})`;
  }

  const section = SECTION.exec(c.path);
  if (section) {
    const i = Number(section[1]);
    const name = song?.sections[i]?.name ?? `Section ${i + 1}`;
    switch (section[2]) {
      case "bars":
        return `${name} length ${span(c.old, c.new, "bars")}`;
      case "name":
        return `${show(c.old)} renamed ${show(c.new)}`;
      case "lanes":
        return `${name} automation`;
      default:
        return `${name} ${section[3]} ${span(c.old, c.new)}`;
    }
  }

  const mix = MIX.exec(c.path);
  if (mix) {
    const rest = mix[1]!;
    if (rest === "master") return `Master mode ${span(c.old, c.new)}`;
    if (rest === "duck_source") return `Duck source ${span(c.old, c.new)}`;
    const found = findMixParam(song, rest);
    if (!found) return `mix ${rest.replace(/\./g, " ")} ${span(c.old, c.new)}`;
    return `${found.label} ${span(c.old, c.new, found.param.unit)}`;
  }

  const track = TRACK.exec(c.path);
  if (track) {
    const [, t, rest] = track as unknown as [string, string, string | undefined];
    if (rest === undefined) return c.old === null ? `Track ${t} added` : `Track ${t} removed`;
    switch (rest) {
      case "gain_db":
        return `${t} gain ${span(c.old, c.new, "dB")}`;
      case "pan":
        return `${t} pan ${span(c.old, c.new)}`;
      case "mute":
        return c.new ? `${t} muted` : `${t} unmuted`;
      case "solo":
        return c.new ? `${t} soloed` : `${t} unsoloed`;
      case "dist.mode":
        return `${t} distortion ${span(c.old, c.new)}`;
    }
    const p = findParam(song, t, rest);
    if (!p) return `${t} ${rest} ${span(c.old, c.new)}`;
    const label = sentenceCase(p.label);
    // A send is never implied: "Delay" alone would read like the delay bus.
    const named = implied(song, t) && !isStrip(song, t, rest) ? label : `${t} ${label}`;
    return `${named} ${span(c.old, c.new, p.unit)}`;
  }

  return c.path;
}

/** "Cutoff 500 → 820 Hz, Resonance 70 → 90%", at most three, then "and N more". */
export function describeChanges(changes: Change[], song: SongView | null, max = 3): string {
  const shown = changes.slice(0, max).map((c) => describeChange(c, song));
  if (changes.length > max) shown.push(`and ${changes.length - max} more`);
  return shown.join(", ");
}
