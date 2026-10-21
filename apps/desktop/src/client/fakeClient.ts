import golden from "../../../../crates/tune-studio/tests/fixtures/protocol.json";
import type { Command, ExternalChangeEvent, StateEvent, StudioEvent } from "./protocol";
import type { StudioClient } from "./StudioClient";

/** The golden file's events, typed. */
export const goldenState = golden.events[0] as unknown as StateEvent;
export const goldenExternal = golden.events[3] as unknown as ExternalChangeEvent;

/** A deep copy of the golden state event, optionally changed. */
export function stateEvent(patch: (s: StateEvent) => void = () => {}): StateEvent {
  const s = structuredClone(goldenState);
  patch(s);
  return s;
}

/** A StudioClient for tests: records calls, answers them from `results`, emits on demand. */
export class FakeClient implements StudioClient {
  calls: Command[] = [];
  results: Partial<Record<Command["cmd"], unknown>> = {};
  private handlers = new Set<(e: StudioEvent) => void>();
  private lost = new Set<() => void>();

  call<T = unknown>(cmd: Command): Promise<T> {
    this.calls.push(cmd);
    const r = this.results[cmd.cmd];
    return r instanceof Error ? Promise.reject(r) : Promise.resolve((r ?? null) as T);
  }

  subscribe(h: (e: StudioEvent) => void): () => void {
    this.handlers.add(h);
    return () => this.handlers.delete(h);
  }

  onLost(h: () => void): () => void {
    this.lost.add(h);
    return () => this.lost.delete(h);
  }

  emit(e: StudioEvent) {
    for (const h of [...this.handlers]) h(e);
  }

  loseConnection() {
    for (const h of [...this.lost]) h();
  }

  /** The calls apart from the store's `get_state` at start. */
  sent(): Command[] {
    return this.calls.filter((c) => c.cmd !== "get_state");
  }

  /** The calls with a given command name. */
  named<C extends Command["cmd"]>(name: C): Extract<Command, { cmd: C }>[] {
    return this.calls.filter((c): c is Extract<Command, { cmd: C }> => c.cmd === name);
  }
}
