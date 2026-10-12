// End-to-end tests of the browser preview build (D-021): the real screens over the in-memory
// demo core with fabricated cases. Nothing here talks to the Rust core or to any network
// service; the page is served from `dist-preview` by `vite preview` on loopback.
// Build it first: `pnpm build:preview`, then `pnpm e2e`.
import { defineConfig, devices } from "@playwright/test";

// Node's `process`, without pulling Node's whole type package into the app's typecheck.
declare const process: { env: Record<string, string | undefined> };

const PORT = 4173;
const CI = Boolean(process.env.CI);

export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*.spec.ts",
  fullyParallel: true,
  forbidOnly: CI,
  retries: CI ? 1 : 0,
  ...(CI ? { workers: 2 } : {}),
  reporter: CI ? [["list"], ["github"]] : "list",
  timeout: 60_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    locale: "he-IL",
    timezoneId: "Asia/Jerusalem",
    // The app is laid out for a desktop window (the preview scales down below 1180px).
    viewport: { width: 1280, height: 900 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 900 } } }],
  webServer: {
    command: `pnpm exec vite preview --mode preview-web --host 127.0.0.1 --port ${PORT} --strictPort`,
    url: `http://127.0.0.1:${PORT}/preview.html`,
    reuseExistingServer: !CI,
    timeout: 30_000,
  },
});
