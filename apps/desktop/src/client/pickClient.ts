import type { StudioClient } from "./StudioClient";
import { createTauriClient } from "./tauriClient";
import { createWsClient } from "./wsClient";

/** Tauri IPC inside the app; otherwise `tune serve`'s WebSocket (or VITE_STUDIO_WS in dev). */
export function pickClient(): StudioClient {
  if ("__TAURI_INTERNALS__" in window) return createTauriClient();
  const url = import.meta.env.VITE_STUDIO_WS ?? `ws://${location.host}/ws`;
  return createWsClient(url);
}
