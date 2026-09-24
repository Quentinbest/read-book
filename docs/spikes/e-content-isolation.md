# Spike E: content isolation

**Verdict: pass on macOS, with a design change to plan §7.1.** Book iframes cannot be sandboxed without `allow-scripts` on WebKit. Isolation relies on the app CSP instead, which WebKit applies to book documents; every probe was blocked. WebView2 and WebKitGTK are deferred (macOS-only scope, `docs/decisions.md`).

- **Date:** 2026-09-24
- **Machine:** Apple M2, 8 GB, macOS 14.6.1, WebKit 19618.3.11.11.5 (reference machine, see `reference-machines.md`)
- **Build:** release, `--features spikes`
- **foliate-js:** 78914ae
- **Raw results:** `raw/e-isolation.json`

## Method

The harness (`src/spikes/e-isolation.ts`) opens `corpus/generated/hostile-content.epub` (built by `scripts/corpus/generate.py`) in a `foliate-view` inside the real app window. It visits all 5 sections, clicks the dangerous links and the form button, and waits out a 3 s meta refresh. Every probe script, if it ran, would leave four independent signals:

1. a `data-pwned` mark on its document (read from the parent, which has same-origin access);
2. a `postMessage` to the top window;
3. a call to the spike-only Tauri command `spike_canary`, tried through `__TAURI_INTERNALS__` on its own window, its parent and top, through `webkit.messageHandlers.ipc`, and through `ipc://localhost`;
4. a request to a canary HTTP server on 127.0.0.1:8765, which also catches every remote-resource probe.

Two variants were run:

- **csp:** the app CSP only (`src-tauri/tauri.conf.json`);
- **csp+meta:** the same, plus a `<meta http-equiv="Content-Security-Policy">` with `default-src 'none'` injected into every book document through foliate-js's `transformTarget` hook.

The app CSP:

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' blob:; img-src 'self' blob: data:;
font-src 'self' blob: data:; media-src blob:; frame-src blob:; worker-src 'self' blob:;
connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'
```

## Results

| Criterion (plan §5, Spike E) | csp | csp+meta | Evidence |
|---|---|---|---|
| No book script runs: inline, `onload` and `onerror`, `javascript:` links, SVG script (inline, as a file, in the spine), external `.js`, module scripts, `srcdoc` and `data:` iframes, `<object>` and `<embed>`, `ontoggle` | pass | pass | No DOM marks, postMessages or IPC calls |
| No Tauri command can be called | pass | pass | `spike_canary` never invoked |
| No remote resource loads: `img`, `srcset`, `link rel=stylesheet`, `@import`, CSS `url()` (in a stylesheet and in a `style` attribute), `@font-face`, `video`, `audio`, `prefetch`, meta refresh, form submit | pass | pass | The canary server received 0 requests |
| No other book's files, `file:` URLs or traversal paths; the app origin cannot be framed | pass | pass | No image probe loaded; `tauri://localhost` did not load in a frame |
| `position: fixed` cannot cover the chrome | pass | pass | The chrome stays topmost; the overlay still covers the page area inside the frame, which the L14 sanitiser (Phase 2) must remove |
| Zip paths cannot escape the library | not applicable to the WebView | | foliate-js reads entries in memory and never extracts. The traversal, absolute-path and symlink archives opened without effect (≤ 41 ms). The Rust importer (Phase 1) must still reject them (§7.1). |
| Verified on WebView2 and WebKitGTK | deferred | | macOS-only scope |

### Finding 1: the §7.1 sandbox design does not work on WebKit

A parent `click` listener on a same-origin frame fired **only when the frame had `allow-scripts`**: without it, false; with it, true. This is WebKit bug 218086, still open as of February 2025. foliate-js relies on parent listeners for links, selection and annotation clicks, so a frame without `allow-scripts` is not usable.

**Consequence.** Plan §7.1 says: “Book iframes are sandboxed without `allow-scripts`, and the `book://` protocol sends `script-src 'none'`.” That is not achievable with foliate-js on macOS. The spike tested the plan's own fallback direction instead: frames keep foliate-js's sandbox (`allow-same-origin allow-scripts`) and script is blocked by CSP. WebKit applies the creating document's CSP to the blob-URL book documents, and that alone blocked every probe. The injected meta CSP (`default-src 'none'`) adds a second, independent layer inside each book document at no measured cost.

**Recommendation.** Adopt “app CSP + per-document meta CSP” as the §7.1 isolation design. Book frames get no `allow-scripts`-free sandbox and are not served from `book://`, because foliate-js needs same-origin access to lay them out. This changes a security design in the approved plan, so it needs owner sign-off (decision D-E1 in `decision.md`).

### Finding 2: XML entity expansion hangs the WebView

`xml-billion-laughs.epub` (nested entities in the OPF) pinned the WebContent process at **100% CPU with no end in sight** (observed for more than 90 s at 52 MB, then killed). The hang is in `DOMParser`, which foliate-js's `epub.js` calls on the OPF. The in-page timeout could not fire because the main thread was blocked. `xml-external-entity.epub` opened in 38 ms, and the external entity was not resolved.

**Consequence.** XML from a book must never reach the WebView unchecked. The Rust core must reject any OPF, NCX, nav or XHTML document that declares entities in a DTD internal subset, both at import (Phase 1, §7.1 “XML”) and before serving content (the Phase 2 L14 pipeline). Plan §7.1 already requires parsers that “reject external entities and cap entity expansion”; this spike shows the check has to run **before** foliate-js parses, not only in the Rust parsers.

### Finding 3: foliate-js would open `javascript:` links with `window.open`

foliate-js classifies `javascript:` hrefs as external links. Unless the `external-link` event is cancelled, it calls `globalThis.open(href, '_blank')` **from the app window**. The harness cancelled every link event, as the product adapter must: `external-link` is always cancelled, and only `http(s)` URLs go to the system browser (N10).

### Notes

- CSP violation events were not observable from the parent (0 reported), so the verdicts rest on the four probe signals, not on violation reports.
- Book styles still apply under this CSP (`style-src 'unsafe-inline' blob:`, with Tauri's style-src nonce injection turned off through `dangerousDisableAssetCspModification: ["style-src"]`). Otherwise Tauri's nonce would make browsers ignore `'unsafe-inline'` and book `style` attributes would stop working.
