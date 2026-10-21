import type { ParamSpecView } from "./lib/param";

const base = { help: "", default: 0, value: 0, explicit: false };
export const cutoff: ParamSpecView = {
  ...base,
  id: "cutoff",
  label: "Cutoff",
  unit: "Hz",
  min: 314,
  max: 2394,
  curve: "exponential",
  default: 500,
};
export const resonance: ParamSpecView = {
  ...base,
  id: "resonance",
  label: "Resonance",
  unit: "%",
  min: 0,
  max: 100,
  curve: "linear",
};
export const gain: ParamSpecView = {
  ...base,
  id: "gain",
  label: "Gain",
  unit: "dB",
  min: -24,
  max: 6,
  curve: "linear",
};
export const waveform: ParamSpecView = {
  ...base,
  id: "waveform",
  label: "Waveform",
  unit: "",
  min: 0,
  max: 1,
  curve: "linear",
};
export const decay: ParamSpecView = {
  ...base,
  id: "decay",
  label: "Decay",
  unit: "ms",
  min: 200,
  max: 2000,
  curve: "linear",
};
