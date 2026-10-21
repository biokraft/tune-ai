import type { Command, StudioEvent } from "./protocol";

/** One transport to the studio: the WebSocket of `tune serve`, or the Tauri app's IPC. */
export interface StudioClient {
  /** Runs one command; resolves with its result, rejects with the studio's error. */
  call<T = unknown>(cmd: Command): Promise<T>;
  /** Every event the studio sends, starting with the current state. Returns the unsubscribe. */
  subscribe(h: (e: StudioEvent) => void): () => void;
  /** Called once when the connection ends for good (WebSocket only). Returns the unsubscribe. */
  onLost?(h: () => void): () => void;
}

/** The message every pending and later call fails with once the WebSocket closes. */
export const LOST_CONNECTION = "Lost connection to the studio";
