import { defineConfig, devices } from "@playwright/test";

const PORT = 4173;

/**
 * Runs against the production bundle, not the dev server: the console is
 * compiled into berth and served by the node, so that is the artifact worth
 * asserting on.
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? "line" : "list",
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // --host 127.0.0.1 on purpose: vite preview otherwise binds "localhost",
    // which resolves to ::1 here, and the IPv4 health check never passes.
    command: `npm run build && npx vite preview --host 127.0.0.1 --port ${PORT} --strictPort`,
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
});
