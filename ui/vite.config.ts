import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The environment Vite runs in (Node's, untyped here).
const env =
  (globalThis as { process?: { env: Record<string, string | undefined> } })
    .process?.env ?? {};

// Tauri loads the dev server from a fixed port and prints Rust errors to the same terminal.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // The stage imports data/ui-conventions.json from the repository root.
    fs: { allow: [".."] },
    // The spec pages' live games (`just specs`): beryl_cli specs-serve.
    proxy: {
      "/spec-api": {
        target: `http://127.0.0.1:${env.BERYL_SPECS_PORT ?? "1422"}`,
        rewrite: (path) => path.replace(/^\/spec-api/, ""),
      },
    },
  },
  // CSS imported by modules reaches the page as <style> elements with an inline source map, by
  // which `by coverage` names the lines of each stylesheet a check used.
  css: { devSourcemap: true },
  build: {
    target: "safari16",
    // The app loads its bundle from disk, not over a network, so one large chunk costs nothing
    // and splitting it would gain nothing. The default limit of 500 kB only printed a warning.
    chunkSizeWarningLimit: 4096,
    // Rolldown printed a report of slow plugin callbacks after every build that nobody reads.
    rolldownOptions: { checks: { bundlerTimings: false } },
  },
});
