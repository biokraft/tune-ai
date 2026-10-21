/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** WebSocket of a running `tune serve`, for `pnpm dev` (e.g. ws://127.0.0.1:4410/ws). */
  readonly VITE_STUDIO_WS?: string;
}
