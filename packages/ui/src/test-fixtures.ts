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

export const sendDelay: ParamSpecView = {
  ...base,
  id: "send.delay",
  label: "Delay",
  unit: "dB",
  min: -60,
  max: 0,
  curve: "linear",
  default: -60,
  value: -60,
};
export const sendReverb: ParamSpecView = {
  ...sendDelay,
  id: "send.reverb",
  label: "Reverb",
};
export const duckDepth: ParamSpecView = {
  ...base,
  id: "duck.depth",
  label: "Duck",
  unit: "dB",
  min: 0,
  max: 24,
  curve: "linear",
  default: 0,
  value: 0,
};
export const duckRelease: ParamSpecView = {
  ...base,
  id: "duck.release",
  label: "Duck release",
  unit: "ms",
  min: 20,
  max: 400,
  curve: "linear",
  default: 80,
  value: 80,
};
export const delayTime: ParamSpecView = {
  ...base,
  id: "delay.time",
  label: "Time",
  unit: "16ths",
  min: 1,
  max: 8,
  curve: "linear",
  default: 3,
  value: 3,
};
export const delayFeedback: ParamSpecView = {
  ...base,
  id: "delay.feedback",
  label: "Feedback",
  unit: "%",
  min: 0,
  max: 90,
  curve: "linear",
  default: 35,
  value: 35,
};
/** The strip params of a track, as `TrackView.strip` lists them. */
export const stripParams: ParamSpecView[] = [sendDelay, sendReverb, duckDepth, duckRelease];
