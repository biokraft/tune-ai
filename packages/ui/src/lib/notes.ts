/** One 16th step of an acid line; mirrors the Rust `AcidStepView`. */
export type AcidStep = { note: number | null; accent: boolean; slide: boolean };

export type DrumHit = "rest" | "hit" | "accent";

const NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"] as const;

export const PITCH_CLASSES = NAMES;

/** Pattern notation: sharps and scientific pitch, so 36 is "C2", 61 is "C#3" and 0 is "C-1". */
export function noteName(midi: number): string {
  return `${NAMES[((midi % 12) + 12) % 12]}${Math.floor(midi / 12) - 1}`;
}

/** Click order of a drum cell: rest, hit, accent, back to rest. */
export function nextHit(h: DrumHit): DrumHit {
  return h === "rest" ? "hit" : h === "hit" ? "accent" : "rest";
}
