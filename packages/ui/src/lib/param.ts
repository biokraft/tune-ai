export type Curve = "linear" | "exponential";

/** Mirrors the Rust `ParamView` sent over the wire. */
export type ParamSpecView = {
  id: string;
  label: string;
  unit: string;
  min: number;
  max: number;
  curve: Curve;
  default: number;
  help: string;
  value: number;
  explicit: boolean;
};

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

/** Maps `normalized` (clamped to 0..1) to the plain range. Mirrors `ParamSpec::to_plain`. */
export function toPlain(spec: ParamSpecView, normalized: number): number {
  const n = clamp(normalized, 0, 1);
  if (n === 0) return spec.min;
  if (n === 1) return spec.max;
  return spec.curve === "linear"
    ? spec.min + n * (spec.max - spec.min)
    : spec.min * Math.pow(spec.max / spec.min, n);
}

/** Maps `plain` (clamped to min..max) to 0..1. Mirrors `ParamSpec::to_normalized`. */
export function toNormalized(spec: ParamSpecView, plain: number): number {
  const p = clamp(plain, spec.min, spec.max);
  return spec.curve === "linear"
    ? (p - spec.min) / (spec.max - spec.min)
    : Math.log(p / spec.min) / Math.log(spec.max / spec.min);
}

/** Formats a plain value for display next to a control. */
export function formatValue(spec: ParamSpecView, plain: number): string {
  switch (spec.unit) {
    case "Hz": {
      const whole = Math.round(plain);
      return whole >= 1000 ? `${(plain / 1000).toFixed(2)} kHz` : `${whole} Hz`;
    }
    case "%":
      return `${Math.round(plain)}%`;
    case "ms":
      return `${Math.round(plain)} ms`;
    case "dB": {
      const text = plain.toFixed(1);
      return `${text === "-0.0" ? "0.0" : text} dB`;
    }
    case "":
      return plain.toFixed(2);
    default:
      return `${plain.toFixed(2)} ${spec.unit}`;
  }
}
