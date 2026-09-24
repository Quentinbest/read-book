# Phase 0 decision: Tauri 2 + foliate-js

**Recommendation: GO for Tauri 2 + foliate-js on macOS**, with three design changes that need owner sign-off (D-E1, D-D1, D-X1 below). Spikes B and C still need a person at the machine. Windows and Linux criteria are deferred under the macOS-only scope.

- **Date:** 2026-09-24
- **Machine:** Apple M2, macOS 14.6.1, WebKit 19618.3.11.11.5 (`reference-machines.md`)
- **Status:** GO approved by the owner on 2026-09-24, with D-E1, D-D1 and D-X1 as recommended

## Spike verdicts

| Spike | Verdict on macOS | Report |
|---|---|---|
| A — rendering | pass (no split lines in 60 layouts); parity across engines deferred | `a-rendering.md` |
| B — input | Pass through a native `NSEvent` bridge: wheel (I1) 81/81 rolls, trackpad (I2, I3, I5) 73/73 gestures with no momentum turns, activating click (I11). The WebView alone cannot classify the device. Scripted gestures not run. | `b-input.md` |
| C — accessibility | Pass on macOS: VoiceOver reads continuously and the page follows. foliate does not report the move, so the adapter must detect it, snap to whole pages and update the location (X3). NVDA deferred. | `c-accessibility.md` |
| D — engine fidelity | Pass. Chapter-crossing turns (27–57 ms) fixed in Phase 2 by pre-laid-out neighbour views: p95 1 ms (D-D1) | `d-engine-fidelity.md` |
| E — content isolation | pass with a design change (every probe blocked); **the §7.1 sandbox design fails on WebKit** | `e-content-isolation.md` |
| F — persistence and search | pass (write p95 < 0.1 ms, 200/200 crash runs, search p95 114 ms) | `f-persistence-and-search.md` |

## Failed criteria and the fallback taken

| # | Failed criterion | Plan's fallback | Taken instead (recommended) | Why |
|---|---|---|---|---|
| D-E1 | E: book iframes sandboxed without `allow-scripts` (§7.1) | “Isolate book frames in a separate origin with no IPC capability, and re-run” | App CSP (`script-src 'self'`, `frame-src blob:`, `form-action 'none'`, …) plus a `default-src 'none'` meta CSP injected into each book document | WebKit bug 218086 makes parent event listeners fail without `allow-scripts`. A separate origin would cut foliate-js off from the frame's DOM, which it needs for layout. The CSP design blocked every probe (script, IPC, network, files, framing) in both variants. |
| D-D1 | D: page turn < 16 ms p95 (chapter-crossing turns: 27 ms) | “Evaluate Readium ts-toolkit before building anything in Phase 2” | Keep foliate-js; lay out neighbouring sections in advance in the adapter (I6, L17) and re-measure in Phase 2. Evaluate Readium only if that fails. | The engine is fast within chapters (p95 3 ms). The cost is loading on demand, which I6 and L17 already require us to avoid. Switching engines would repeat Spikes A, D and E. |
| D-X1 | E, finding 2: billion-laughs XML hangs WebKit's `DOMParser` | (not anticipated) | The Rust core refuses any OPF, NCX, nav or XHTML that declares entities, at import (done in Phase 1: `src-tauri/src/epub.rs`) and before serving content (Phase 2) | Book XML must never reach the WebView unchecked. |

## Other outcomes that change later phases

- **Links (N10):** the adapter must cancel every foliate-js `external-link` event; foliate-js would otherwise `window.open` even `javascript:` URLs from the app window.
- **Turn queue (I6):** foliate-js drops turns for 100 ms after each turn; the adapter queues one instead.
- **Two columns (L8):** foliate-js defaults to a two-column spread; set `max-column-count` to 1 below 1480 px.
- **Throttling:** occluded WKWebView windows are throttled hard. Spike and performance runs use `tauri.spikes.conf.json` (throttling off, window on top). The product keeps the default.
- **Budgets:** opening the 98 MB book takes p95 292 ms, half of it the whole-file IPC read, which L17 removes.

## Still to do before Phase 2 can start (plan §5 Phase 2 “Before starting”)

1. ~~The owner signs off D-E1, D-D1 and D-X1.~~ Done 2026-09-24.
2. ~~Spikes B and C~~ done 2026-09-24 (B via the native bridge; C with adapter work for X3).
3. ~~B2, B8, B11, B12 and T3 are decided~~ (done 2026-09-24, `docs/decisions.md`); the G8 designs exist.
