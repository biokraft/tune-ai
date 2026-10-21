import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Command, StudioEvent } from "./protocol";
import type { StudioClient } from "./StudioClient";

/** The studio inside the Tauri app: one `studio` command, one `studio-event` stream. */
export function createTauriClient(): StudioClient {
  return {
    call<T = unknown>(cmd: Command): Promise<T> {
      return invoke<T>("studio", { command: cmd }).catch((e: unknown) => {
        throw e instanceof Error ? e : new Error(String(e));
      });
    },
    subscribe(h) {
      let off: UnlistenFn | null = null;
      let done = false;
      void listen<StudioEvent>("studio-event", (e) => h(e.payload)).then((un) => {
        if (done) un();
        else off = un;
      });
      return () => {
        done = true;
        off?.();
      };
    },
  };
}
