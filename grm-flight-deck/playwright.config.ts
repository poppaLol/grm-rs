import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  testMatch: /.*\.visual\.spec\.ts/,
  fullyParallel: false,
  reporter: [["list"], ["html", { outputFolder: "playwright-report", open: "never" }]],
  outputDir: "test-results",
  use: {
    baseURL: "http://127.0.0.1:8081",
    trace: "retain-on-failure",
    screenshot: "only-on-failure"
  },
  webServer: {
    command: "npm run dev -- --strictPort",
    url: "http://127.0.0.1:8081",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000
  },
  projects: [
    {
      name: "chromium-system",
      use: {
        ...devices["Desktop Chrome"],
        launchOptions: {
          executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ?? "/snap/bin/chromium"
        },
        viewport: { width: 1440, height: 1000 }
      }
    }
  ]
});
