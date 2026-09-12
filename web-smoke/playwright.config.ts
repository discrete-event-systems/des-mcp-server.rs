import { defineConfig, devices } from '@playwright/test';

// Browser-smoke config for the discrete-event-systems org's LIVE public web
// surface (the GitHub Pages site). The only external dependency is the target
// site itself; there is no local web server to start. Retries absorb transient
// CDN/network blips so the scheduled job stays low-noise.
export default defineConfig({
  testDir: '.',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  timeout: 30_000,
  expect: { timeout: 15_000 },
  retries: 2,
  reporter: [['list']],
  use: {
    headless: true,
    ignoreHTTPSErrors: false,
    actionTimeout: 15_000,
    navigationTimeout: 20_000,
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
