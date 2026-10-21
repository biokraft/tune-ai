import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Command, StudioEvent } from "./protocol";
import type { StudioClient } from "./StudioClient";

/**
 * The studio inside the Tauri app: one `studio` command, one `studio-event` stream. Tauri does
 * not buffer events, so the host's first state can go out before `listen` is registered; the
 * store asks for it with `get_state`, and calls wait until every listener is in place so that
 * answer is never older than the events that follow it.
 *
 * Tauri runs `invoke`s concurrently, so calls are chained: each starts once the one before it
 * settled. A throttled live knob value can then never overtake the release that follows it.
 */
export function createTauriClient(): StudioClient {
  /** Resolves once every `listen` made so far is registered (or failed). */
  let listening: Promise<unknown> = Promise.resolve();
  /** Settles once the last call made so far has settled; never rejects. */
  let last: Promise<unknown> = Promise.resolve();
  return {
    call<T = unknown>(cmd: Command): Promise<T> {
      const run = Promise.all([last, listening]).then(() => invoke<T>("studio", { command: cmd }));
      last = run.catch(() => {});
      return run.catch((e: unknown) => {
        throw e instanceof Error ? e : new Error(String(e));
      });
    },
    subscribe(h) {
      let off: UnlistenFn | null = null;
      let done = false;
      const registered = listen<StudioEvent>("studio-event", (e) => h(e.payload)).then((un) => {
        if (done) un();
        else off = un;
      });
      // A failed listen must not hold every call back.
      listening = Promise.all([listening, registered.catch(() => {})]);
      return () => {
        done = true;
        off?.();
      };
    },
  };
}
