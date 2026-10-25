import { describe, expect, it } from "vitest";
import { instrumentTitle, trackColor, trackColorVar } from "./instruments";

describe("instruments", () => {
  it("maps every instrument to its design colour", () => {
    expect(trackColorVar("acid303")).toBe("--acid");
    expect(trackColorVar("kick")).toBe("--kick");
    expect(trackColorVar("clap")).toBe("--clap");
    expect(trackColorVar("hat_closed")).toBe("--hat");
    expect(trackColorVar("hat_open")).toBe("--ohat");
    expect(trackColorVar("ride")).toBe("--ride");
    expect(trackColorVar("future_synth")).toBe("--ink2");
    expect(trackColor("kick")).toBe("rgb(var(--kick))");
  });

  it("titles instruments for the dock chip", () => {
    expect(instrumentTitle("acid303")).toBe("Acid 303");
    expect(instrumentTitle("hat_closed")).toBe("Closed hat");
    expect(instrumentTitle("hat_open")).toBe("Open hat");
    expect(instrumentTitle("future_synth")).toBe("Future synth");
  });
});
