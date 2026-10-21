import golden from "../../../../crates/tune-studio/tests/fixtures/protocol.json";
import { COMMAND_NAMES, EVENT_NAMES } from "./protocol";

const sorted = (xs: readonly string[]) => [...xs].sort();

describe("protocol", () => {
  it("every golden command has a cmd in COMMAND_NAMES", () => {
    for (const c of golden.commands) expect(COMMAND_NAMES).toContain(c.cmd);
  });

  it("every golden event has an event in EVENT_NAMES", () => {
    for (const e of golden.events) expect(EVENT_NAMES).toContain(e.event);
  });

  it("the name lists are exactly the golden sets", () => {
    expect(sorted(COMMAND_NAMES)).toEqual(sorted(golden.commands.map((c) => c.cmd)));
    expect(sorted(EVENT_NAMES)).toEqual(sorted(golden.events.map((e) => e.event)));
  });
});
