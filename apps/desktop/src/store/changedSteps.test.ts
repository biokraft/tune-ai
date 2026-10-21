import { changedSteps } from "./changedSteps";

describe("changedSteps", () => {
  it("compares acid patterns by token", () => {
    expect(changedSteps("C2 . D2", "C2 C2 D2")).toEqual([1]);
    expect(changedSteps("C2 C2! C3~ .", "C2 C2 C3~ .")).toEqual([1]);
  });

  it("compares drum patterns by character, ignoring spaces", () => {
    expect(changedSteps("x... x...", "x... X...")).toEqual([4]);
    expect(changedSteps("x...x...", "x... x...")).toEqual([]);
  });

  it("counts steps added or removed as changed", () => {
    expect(changedSteps("C2 D2", "C2 D2 E2 .")).toEqual([2, 3]);
    expect(changedSteps("x...", "x.")).toEqual([2, 3]);
  });
});
