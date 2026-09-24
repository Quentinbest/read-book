import { defineConfig, devices } from '@playwright/test'

// Frontend integration tests (plan §6.1) on Chromium and WebKit, IPC mocked
// where needed. Playwright's WebKit build is frozen on macOS 14 and no longer
// starts there, so WebKit runs where it is current (CI, macOS 15+) or when
// PW_WEBKIT=1 is set.
const webkit = !!process.env.CI || !!process.env.PW_WEBKIT

export default defineConfig({
  testDir: 'tests/integration',
  use: { baseURL: 'http://localhost:1420' },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    ...(webkit ? [{ name: 'webkit', use: { ...devices['Desktop Safari'] } }] : []),
  ],
  webServer: {
    command: 'pnpm dev',
    url: 'http://localhost:1420/gallery.html',
    reuseExistingServer: !process.env.CI,
  },
})
