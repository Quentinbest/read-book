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
