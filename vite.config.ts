import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

const host = process.env.TAURI_DEV_HOST
// The Phase 0 spike harness page is built only for spike runs (docs/spikes/).
const spikes = !!process.env.LINEN_SPIKES

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [svelte()],
  // Tauri prints its own logs; keep Rust errors visible.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    // Minimum WebViews (plan D7): Safari 16 on macOS 13, evergreen WebView2.
    // LINEN_BUILD_TARGET overrides for spike bundles run on older WebKit (Spike B on macOS 12).
    target:
      process.env.LINEN_BUILD_TARGET ??
      (process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari16'),
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      input: {
        main: 'index.html',
        settings: 'settings.html',
        ...(spikes ? { spikes: 'spikes.html' } : {}),
      },
    },
  },
})
