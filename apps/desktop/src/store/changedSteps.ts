const DRUM = /^[xX.\s]*$/;

/**
 * Indices of the steps that differ between two pattern texts (`clips.<c>.steps` values).
 * Acid patterns compare by token (`C2 C2! . D#2~`); drum patterns by character with spaces
 * dropped (`x... X...`). Steps only one side has count as changed.
 */
export function changedSteps(oldSteps: string, newSteps: string): number[] {
  const drum = DRUM.test(oldSteps) && DRUM.test(newSteps);
  const split = (s: string) =>
    drum ? [...s.replace(/\s+/g, "")] : s.trim().split(/\s+/).filter(Boolean);
  const a = split(oldSteps);
  const b = split(newSteps);
  const out: number[] = [];
  for (let i = 0; i < Math.max(a.length, b.length); i++) if (a[i] !== b[i]) out.push(i);
  return out;
}
