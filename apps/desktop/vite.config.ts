import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Tauri serves the built files from disk; nothing is loaded from the network.
export default defineConfig(({ mode }) => ({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ["VITE_"],
  // `vite build --mode preview-web`: the browser preview (fake core, fabricated data, D-021).
  base: mode === "preview-web" ? "./" : "/",
  build: {
    target: ["es2022", "chrome110", "safari16"],
    sourcemap: false,
    assetsInlineLimit: 0,
    // One stylesheet, in module order, for the main and the lazy chunks alike: component rules
    // override base rules by coming later, and a stylesheet per chunk would load in a different
    // order (an icon button lost its narrow padding).
    cssCodeSplit: false,
    ...(mode === "preview-web" ? { outDir: "dist-preview", rollupOptions: { input: "preview.html" } } : {}),
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
}));
