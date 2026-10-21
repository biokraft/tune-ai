import { describe, expect, it } from "vitest";
import { cutoff, decay, delayTime, gain, resonance, waveform } from "../test-fixtures";
import { formatValue, toNormalized, toPlain } from "./param";

describe("toPlain / toNormalized", () => {
  it("maps the exponential midpoint to the geometric mean", () => {
    expect(Math.abs(toPlain(cutoff, 0.5) - Math.sqrt(314 * 2394))).toBeLessThan(1e-9);
  });

  it("maps the bottom of the range to 0", () => {
    expect(toNormalized(cutoff, 314)).toBe(0);
  });

  it("maps linear values", () => {
    expect(toPlain(resonance, 0.7)).toBe(70);
  });

  it("clamps like the Rust spec", () => {
    expect(toPlain(cutoff, -3)).toBe(314);
    expect(toPlain(cutoff, 7)).toBe(2394);
    expect(toNormalized(cutoff, 1)).toBe(0);
    expect(toNormalized(cutoff, 1e9)).toBe(1);
  });

  it("round-trips 11 points on both curves", () => {
    for (const spec of [cutoff, resonance]) {
      for (let i = 0; i <= 10; i++) {
        const x = i / 10;
        expect(Math.abs(toNormalized(spec, toPlain(spec, x)) - x)).toBeLessThan(1e-12);
      }
    }
  });
});

describe("formatValue", () => {
  it("formats Hz below and above 1000", () => {
    expect(formatValue(cutoff, 867.04)).toBe("867 Hz");
    expect(formatValue(cutoff, 1200)).toBe("1.20 kHz");
  });

  it("formats percent, ms, dB and unitless", () => {
    expect(formatValue(resonance, 70)).toBe("70%");
    expect(formatValue(decay, 400)).toBe("400 ms");
    expect(formatValue(gain, -3)).toBe("-3.0 dB");
    expect(formatValue(waveform, 0.5)).toBe("0.50");
  });

  it("formats 16ths", () => {
    expect(formatValue(delayTime, 3)).toBe("3/16");
    expect(formatValue(delayTime, 2.5)).toBe("2.50/16");
  });
});
