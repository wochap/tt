import { existsSync } from "node:fs";

import { defineConfig, devices } from "@playwright/test";

// End-to-end against a real tt-server serving the built bundle
// (`tt-server serve --web-dir web/dist`); see e2e/serve.sh.
const PORT = Number(process.env.TT_E2E_PORT ?? 8123);
// Playwright's own Chromium when installed (`pnpm exec playwright install chromium`),
// otherwise a system Chrome (`TT_E2E_CHROME`, or google-chrome on PATH-less NixOS).
const chrome = process.env.TT_E2E_CHROME ?? ["/run/current-system/sw/bin/google-chrome", "/usr/bin/google-chrome"].find(existsSync);

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: [["list"]],
  use: {
    ...devices["Desktop Chrome"],
    baseURL: `http://127.0.0.1:${PORT}`,
    viewport: { width: 1440, height: 900 },
    timezoneId: "Europe/Berlin",
    trace: "retain-on-failure",
    launchOptions: chrome && !process.env.TT_E2E_BUNDLED_BROWSER ? { executablePath: chrome } : {},
  },
  webServer: {
    command: "bash e2e/serve.sh",
    url: `http://127.0.0.1:${PORT}/api/health`,
    reuseExistingServer: false,
    timeout: 600_000,
    env: { TT_E2E_PORT: String(PORT) },
    stdout: "pipe",
  },
});
