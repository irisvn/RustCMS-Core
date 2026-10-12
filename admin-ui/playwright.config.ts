import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: "http://localhost:8123",
    trace: "retain-on-failure",
  },
  // PLAYWRIGHT_CHANNEL=chrome uses an installed Google Chrome instead of the bundled Chromium.
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"], channel: process.env.PLAYWRIGHT_CHANNEL || undefined } },
  ],
  webServer: {
    command: "./e2e/serve.sh",
    url: "http://localhost:8123/health",
    reuseExistingServer: false,
    timeout: 120_000,
    stdout: "pipe",
  },
});
