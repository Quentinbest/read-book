# Decision record

Owner decisions taken after plan revision D. IDs refer to `docs/implementation-plan.md` §10. The plan text is not edited; this file is the record.

| # | Decision | Date |
|---|---|---|
| Scope | Develop and verify on **macOS only** for now. Windows and Linux work (WebView2, WebKitGTK, NVDA, Orca, Windows high contrast, Linux packaging) is deferred, not dropped. Spike criteria that compare engines are recorded as deferred. CI builds macOS only. | 2026-09-24 |
| D3 | **Direct download:** a signed, notarised DMG, not the Mac App Store. No App Store sandbox, so the import pipeline and extensions can use normal file access. | 2026-09-24 |
| B3 | **Import semantics.** Books are copied to `~/Library/Application Support/app.linen.reader/Books/`. Same content hash → already in the library: no second copy, open the existing book. Same OPF `unique-identifier` with a different hash → an updated file: replace the copy and re-anchor annotations (anything unplaceable goes to “Couldn’t place”, A8). | 2026-09-24 |
| T5 | **UI font:** the platform font (`system-ui`, SF Pro on macOS), as V3 states. Literata (OFL) is bundled as the reading fallback. Instrument Sans is not shipped. | 2026-09-24 |
| B7 | **One window, one open book** in the MVP. | 2026-09-24 |
| D5 | **Accessibility target:** WCAG 2.2 AA, plus the design’s AAA 7:1 body-text contrast (X1). | 2026-09-24 |

Still open before Phase 1 exits: B1, B4, B9, B10, G4 import states, G6 (macOS-only scope reduces G6 to the macOS menu bar).

## Open items found during Phase 0–1 work (awaiting the owner)

| # | Finding | Recommended default (applied provisionally) | Evidence |
|---|---|---|---|
| C7 | The approved Night highlight tints (S4, 16% alpha) give highlighted text 8.2–8.8:1, under X1's 9:1. | Night tints at 11% alpha (≥ 9.2:1 for all four colours, underlines unchanged). Sepia tints (not drawn in S4) use the Paper tints at 75% over the Sepia ground; the Sepia search outline is `#8C5A45` (3.9:1). Night search marks are derived from the Night accent. All marked PROVISIONAL in `src/lib/theme/tokens.ts`. | `src/lib/theme/tokens.test.ts` |
| B3-guard | B3 treats the same OPF identifier with different bytes as an updated file. Identifiers are sometimes reused or left as placeholders, so a different book would silently replace another. | An updated file must also have the same package title (case and spacing ignored); otherwise it imports as a separate book. | `src-tauri/src/import.rs` test `a_reused_identifier_with_a_different_title_is_a_new_book` |
| D-E1 | Plan §7.1 isolates book iframes by sandboxing them without `allow-scripts`. On WebKit that breaks every parent event listener (WebKit bug 218086), which foliate-js needs. | Isolate with the app CSP (`script-src 'self'`, `frame-src blob:`, `form-action 'none'` …), which WebKit applies to book blob documents, plus a `default-src 'none'` meta CSP injected into each book document. Spike E blocked every probe with this design. | `docs/spikes/e-content-isolation.md` |
| D-D1, D-X1 | Spike D page turns across chapters (27 ms against 16 ms); XML entity hang in WebKit. | See `docs/spikes/decision.md`. | `docs/spikes/` |
