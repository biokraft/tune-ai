import type { Change, Json, KeptTarget, ParamView, SongView } from "../client/protocol";
import { changedSteps } from "../store/changedSteps";
import { formatNumber, sentenceCase } from "./song";

const show = (v: Json): string =>
  v === null ? "none" : typeof v === "number" ? formatNumber(v) : String(v);

/** `old → new` with the param's unit: "500 → 820 Hz", "70 → 90%". */
function spanOf(old: Json, next: Json, unit = ""): string {
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

const MODE = /^([^.]+)\.mode$/;

/**
 * "distortion" (or "distortion d2" for one with another id) when `rest` is the mode of a
 * distortion on `track`. Without the song, only the default id `dist` is known to be one.
 */
function distortionMode(song: SongView | null, track: string, rest: string): string | null {
  const id = MODE.exec(rest)?.[1];
  if (id === undefined) return null;
  const fx = song?.tracks.find((x) => x.id === track)?.fx.find((f) => f.id === id);
  const isDistortion = fx ? fx.kind === "distortion" : song === null && id === "dist";
  if (!isDistortion) return null;
  return id === "dist" ? "distortion" : `distortion ${id}`;
}

const MIX = /^mix\.(.+)$/;
const TRACK = /^tracks\.([^.]+)(?:\.(.+))?$/;
const SECTION = /^sections\[(\d+)\]\.(bars|name|lanes|play\.(.+))$/;
const PATTERN = /^clips\.(.+)\.steps$/;
const NOTE_CLIP = /^clips\.(.+)\.(notes|bars)$/;

/** One change in a few plain words, for the change notice. */
export function describeChange(c: Change, song: SongView | null): string {
  return describeWith(c, song, spanOf);
}

type Span = (old: Json, next: Json, unit?: string) => string;

function describeWith(c: Change, song: SongView | null, span: Span): string {
  if (c.path === "song.bpm") return `Tempo ${span(c.old, c.new, "BPM")}`;
  if (c.path === "song.version") {
    // Back to an older version: an undo of the migration, or the old text put back.
    if (typeof c.old === "number" && typeof c.new === "number" && c.new < c.old) {
      return `Restored the song file to format version ${show(c.new)}`;
    }
    return `Upgraded the song file to format version ${show(c.new)} (comments kept)`;
  }
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

  const noteClip = NOTE_CLIP.exec(c.path);
  if (noteClip) {
    return `Changed the ${noteClip[2] === "notes" ? "notes" : "length"} of ${noteClip[1]}`;
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
        // Placements that are not one clip over the section have no short form.
        if (Array.isArray(c.old) || Array.isArray(c.new)) {
          return `Changed what ${section[3]} plays in ${name}`;
        }
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
    if (rest === undefined) {
      if (c.old === null) return `Track ${t} added`;
      if (c.new === null) return `Track ${t} removed`;
      return `Changed the devices on ${t}`;
    }
    switch (rest) {
      case "gain_db":
        return `${t} gain ${span(c.old, c.new, "dB")}`;
      case "pan":
        return `${t} pan ${span(c.old, c.new)}`;
      case "mute":
        return c.new ? `${t} muted` : `${t} unmuted`;
      case "solo":
        return c.new ? `${t} soloed` : `${t} unsoloed`;
    }
    const mode = distortionMode(song, t, rest);
    if (mode !== null) return `${t} ${mode} ${span(c.old, c.new)}`;
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

const TARGET_TRACK = /^tracks\.([^.]+)(?:\.(instrument|fx|play))?$/;
const TARGET_CLIP = /^clips\.([^.]+)$/;
const TARGET_LANE = /^automation\.([^.]+)\.(.+)$/;

/** What a leaf target is called, without its change: describeChange's wording minus the span. */
function leafLabel(path: string, song: SongView | null): string {
  const steps = PATTERN.exec(path);
  if (steps) return `${steps[1]} pattern`;
  const flag = /^tracks\.([^.]+)\.(mute|solo)$/.exec(path);
  if (flag) return `${flag[1]} ${flag[2]}`;
  return describeWith({ path, old: null, new: null }, song, () => "").trim();
}

/** A target a run undo kept, in a few plain words: "Cutoff (by you)", "clip a1", "the sections". */
export function describeTarget(k: KeptTarget, song: SongView | null): string {
  const path = k.target;
  let what: string;
  const track = TARGET_TRACK.exec(path);
  const lane = TARGET_LANE.exec(path);
  if (track) {
    const [, t, part] = track as unknown as [string, string, string | undefined];
    what =
      part === "instrument"
        ? `${t}'s instrument`
        : part === "fx"
          ? `${t}'s effects`
          : part === "play"
            ? `a clip on ${t}`
            : `track ${t}`;
  } else if (TARGET_CLIP.test(path)) {
    what = `clip ${TARGET_CLIP.exec(path)![1]}`;
  } else if (lane) {
    what = lane[1] === "mix" ? `mix ${lane[2]} automation` : `${lane[2]} automation on ${lane[1]}`;
  } else if (path === "sections") {
    what = "the sections";
  } else {
    what = leafLabel(path, song);
  }
  if (k.by === "human") return `${what} (by you)`;
  if (k.by === "claude") return `${what} (another run)`;
  return what;
}
