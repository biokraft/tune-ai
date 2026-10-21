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
    t.fx.flatMap((f) => f.params).find((p) => p.id === id) ??
    null
  );
}

/** Whether a param on `track` is named without its track: the song's only 303 is implied. */
function implied(song: SongView | null, track: string): boolean {
  const acids = song?.tracks.filter((t) => t.instrument === "acid303") ?? [];
  return acids.length === 1 && acids[0]!.id === track;
}

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
    const named = implied(song, t) ? label : `${t} ${label}`;
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
