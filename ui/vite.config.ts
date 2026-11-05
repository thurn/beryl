import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri loads the dev server from a fixed port and prints Rust errors to the same terminal.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // The UI imports data/cards.json from the repository root.
    fs: { allow: [".."] },
  },
  build: {
    target: "safari16",
  },
});
