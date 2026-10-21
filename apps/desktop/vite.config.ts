import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Port 1420 is the origin `tune serve` accepts from the vite dev server. Set
// VITE_STUDIO_WS (e.g. ws://127.0.0.1:4410/ws) to talk to a running `tune serve`.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
