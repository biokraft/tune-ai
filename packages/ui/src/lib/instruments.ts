const COLOR_VARS: Record<string, string> = {
  acid303: "--acid",
  kick: "--kick",
  clap: "--clap",
  hat_closed: "--hat",
  hat_open: "--ohat",
  ride: "--ride",
};

const TITLES: Record<string, string> = {
  acid303: "Acid 303",
  hat_closed: "Closed hat",
  hat_open: "Open hat",
};

/** The bare CSS variable name of an instrument's colour, for alpha mixes. Unknown instruments get ink2. */
export function trackColorVar(instrument: string): string {
  return COLOR_VARS[instrument] ?? "--ink2";
}

/** A CSS colour expression for an instrument, e.g. `rgb(var(--acid))`. */
export function trackColor(instrument: string): string {
  return `rgb(var(${trackColorVar(instrument)}))`;
}

/** A human title for the dock chip: known names, else the id with underscores as spaces, sentence case. */
export function instrumentTitle(instrument: string): string {
  const known = TITLES[instrument];
  if (known) return known;
  const words = instrument.replace(/_/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}
