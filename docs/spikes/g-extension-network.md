# Spike G: extension network isolation (§7.2)

- **Date:** 2026-09-25
- **Scope:** macOS (WKWebView), the only platform in scope now. WebView2 and WebKitGTK must be re-run before those platforms ship.
- **Run:** `LINEN_SPIKE=g` with a throwaway `LINEN_DATA_DIR`
- **Raw result:** `docs/spikes/raw/g-extension-network.json`
- **Result: pass.**

## Mechanism (built in `src-tauri/src/extensions.rs`)

- **Its own origin for each extension.** An installed extension's package is served on `linen-ext://<extension id>/`.
- **The host frame.** Our host page, `/_host.html`, runs on that origin, framed with `sandbox="allow-scripts allow-same-origin"`. It creates the extension's Worker (same origin) and only relays messages.
- **Worker policy.** Package scripts are served with `default-src 'none'; script-src 'self'; connect-src 'none'; worker-src 'none'; font-src 'none'; img-src 'none'`. That means:
  - no network of any kind;
  - scripts from the extension's own package only;
  - no nested workers.
- **UI pages** (Navigator tabs in Phase 7) get their own scripts and styles, and nothing else: no network, frames, workers or forms.
- **Other headers.** Everything is sent with `nosniff` and `Cross-Origin-Resource-Policy: same-origin`. Package paths are plain components inside the package folder, symlinks included; unknown file types are refused.

## What the probe tried

The probe extension's Worker tried each of these:
- `fetch` (cors and no-cors), `XMLHttpRequest`, `WebSocket`, `EventSource`;
- `importScripts` from a remote host, from another extension, and from `data:`;
- nested Workers: its own, remote, and `blob:`;
- `sendBeacon`, and `FontFace` with a remote URL;
- Tauri IPC over `ipc://` and `http://ipc.localhost`;
- the app origin, and the book scheme.

**All were blocked.** The canary server at 127.0.0.1:8765 got no request, and the IPC canary command was never called. The extension's own `importScripts` still works; that is the positive control.

Its UI page, framed `allow-scripts allow-same-origin`, tried each of these:
- `fetch`, a nested iframe, and a form post;
- an image, a stylesheet, and a prefetch;
- a Worker;
- `__TAURI_INTERNALS__`, WebKit's `ipc` message handler, and IPC over `ipc://`;
- the parent's DOM.

**Nothing reached the network, the app, or a Tauri command.**

## Findings for Phase 7

- **Opaque-origin UI frames don't work.** With `sandbox="allow-scripts"` alone (an opaque origin), WebKit refuses the page's own `script-src 'self'` script, so the page cannot run at all. That fails closed, so it's safe, but it's unusable. UI pages must be framed `allow-scripts allow-same-origin`. That is safe here, because their origin is the extension's own, never the app's.
- **The IPC handler reaches subframes, and Tauri refuses it.** WebKit's `ipc` message handler is present in subframes, and a forged message was posted from the UI page. Tauri refused it (the canary saw nothing). Phase 7 keeps this probe in its tests, and adds the permission check on every brokered call (P2).
- **`net.fetch` still to build.** It is the only way out, and it is Phase 7 work: the host performs fetches for declared hosts only.
