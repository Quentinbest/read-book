import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'

// Unit tests only; Playwright runs tests/integration (pnpm test:integration).
// The Svelte plugin compiles `.svelte.ts` modules (runes state classes).
export default defineConfig({
  plugins: [svelte()],
  test: { include: ['src/**/*.test.ts'] },
})
