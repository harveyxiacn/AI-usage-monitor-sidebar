import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [sveltekit()],

  // Pre-bundle everything a lazily loaded dashboard tab or the Tauri bridge
  // pulls in. Discovered late, Vite re-optimises and *reloads the page*, which
  // drops the first interaction in `tauri dev` and failed whichever e2e tests
  // ran first against a cold dev server.
  optimizeDeps: {
    include: ["chart.js", "@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/plugin-opener"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
