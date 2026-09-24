import { defineConfig } from 'vitest/config'

// Unit tests only; Playwright runs tests/integration (pnpm test:integration).
export default defineConfig({
  test: { include: ['src/**/*.test.ts'] },
})
