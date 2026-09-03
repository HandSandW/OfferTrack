import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    rolldownOptions: {
      input: { main: "index.html", help: "help.html" },
    },
    target:
      process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
  },
  test: {
    // Large jsdom screens contend on Windows/CI; serialize files rather than
    // relaxing assertion/test timeouts or retrying failures automatically.
    maxWorkers: 1,
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    css: true,
  },
});
