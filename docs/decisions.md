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

## Signed off 2026-09-24 (found during Phase 0–1 work)

The owner approved all four sign-offs. The values below are no longer provisional.

| # | Finding | Approved decision | Evidence |
|---|---|---|---|
| C7 | The approved Night highlight tints (S4, 16% alpha) give highlighted text 8.2–8.8:1, under X1's 9:1. | Night tints at 11% alpha (≥ 9.2:1 for all four colours, underlines unchanged). Sepia tints (not drawn in S4) use the Paper tints at 75% over the Sepia ground; the Sepia search outline is `#8C5A45` (3.9:1). Night search marks are derived from the Night accent. All marked PROVISIONAL in `src/lib/theme/tokens.ts`. | `src/lib/theme/tokens.test.ts` |
| B3-guard | B3 treats the same OPF identifier with different bytes as an updated file. Identifiers are sometimes reused or left as placeholders, so a different book would silently replace another. | An updated file must also have the same package title (case and spacing ignored); otherwise it imports as a separate book. | `src-tauri/src/import.rs` test `a_reused_identifier_with_a_different_title_is_a_new_book` |
| D-E1 | Plan §7.1 isolates book iframes by sandboxing them without `allow-scripts`. On WebKit that breaks every parent event listener (WebKit bug 218086), which foliate-js needs. | Isolate with the app CSP (`script-src 'self'`, `frame-src blob:`, `form-action 'none'` …), which WebKit applies to book blob documents, plus a `default-src 'none'` meta CSP injected into each book document. Spike E blocked every probe with this design. | `docs/spikes/e-content-isolation.md` |
| D7-WebKit | foliate-js's paginator uses a regex lookbehind, which WebKit parses only from Safari 16.4. macOS 13 shipped with Safari 16.0–16.3, so the reader engine fails to load on an un-updated macOS 13. | Minimum: macOS 13 **with Safari 16.4 or later installed**. Say so in the system requirements; on older WebKit the app should show an “update Safari” message rather than a blank reader (Phase 8). Found 2026-09-24 while preparing Spike B for a macOS 12 MacBook Air (Safari 15.6.1). | `node_modules/foliate-js/paginator.js:655` |
| D-D1, D-X1 | Spike D page turns across chapters (27 ms against 16 ms); XML entity hang in WebKit. | See `docs/spikes/decision.md`. | `docs/spikes/` |

## Recommended defaults adopted 2026-09-24

The owner chose the recommended defaults for the open behaviour questions (plan §10.3, §10.5). B11, B12 and B13 use the plan's own recommendations. The plan gave none for the others; the defaults below were proposed with this record and accepted on the owner's instruction to use recommended defaults. Any of them can be revisited.

| # | Question | Decision |
|---|---|---|
| B1 | What a “page” number means | Dynamic pages across the whole book at the current layout, used by the location line and the “Back to page N” chip (N2). Estimated from chapter sizes and shown with “≈” until pagination settles in idle time (L10, L16). Print page-list numbers appear only in Go to › Print page (N8), labelled “print page”. |
| B2 | “N min left in chapter” | Words left in the chapter ÷ the reader's pace. Pace is a rolling median words-per-minute over the last 30 minutes of page dwell times (each dwell counted only between 2 s and 2 min), kept as one number in `settings` on this device, with no per-session log. It starts at 250 wpm, and nothing is shown until 5 minutes have been read or when under 1 minute is left. |
| B4 | Highlight edge cases | Overlaps are allowed: selecting an existing highlight's exact range changes its colour, and a partial overlap creates a separate highlight (both drawn). A highlight lies within one chapter; a selection cannot cross a chapter boundary in Pages mode. No length limit beyond the chapter. Copy copies the plain text only. |
| B5 | Undo scope | Deleting a highlight, deleting a note and changing a highlight's colour are undoable (⌘Z, the Undo message, ⌘K › Recently closed). Typing inside a note uses the text field's own undo. |
| B6 | Search limits | The scan is not capped (Spike F: 19,959 hits for “the” in 24 ms). The Search tab renders 100 results per chapter, then “Show all N”. No space is reserved for regular-expression or whole-word options. |
| B7 | Windows and books | One window, one open book (decided earlier). |
| B8 | Scroll mode | One continuous scroll per chapter. At the end of a chapter, scrolling on moves into the next one, so reading is continuous without laying out the whole book. The location line and scrubber show the book-wide position, as in Pages. |
| B9 | DRM-protected files | Detected at import (`META-INF/encryption.xml` with any algorithm other than font obfuscation) and not added: ““file.epub” is protected by DRM and can't be opened in Linen.” No attempt to remove DRM. |
| B10 | UI localisation | Every UI string lives in one English message catalogue from the start, so translation is possible later. There is no translation or right-to-left UI mirroring in the MVP; book content keeps its own direction (I15). |
| B11 | What Esc from the Navigator returns to | The chrome state from before the Navigator opened (plan recommendation; implemented in `src/lib/reader/state.ts`). |
| B12 | Spread breakpoint | Measured on the width left after a docked Navigator, applied after the Navigator slide (plan recommendation). |
| B13 | History and Recently closed | Back history per book, in memory, last 50 jumps; Recently closed for the session, last 20 (plan recommendation; implemented). |
| T3 | Caret browsing (F7) on WebKit | Our own caret browsing in the book frame: F7 shows a caret, and ⇧ + arrows extend the selection with `Selection.modify`, which WebKit supports. |

Still needed before Phase 2 starts: Spikes B and C (owner at the machine) and the G8 designs (Scroll mode, two-page spread, fixed-layout zoom and pan, RTL and vertical writing, loading state and “≈” locations). G4 (library and import states) is needed by Phase 6, and its import states by Phase 1 exit; the import messages stay provisional until then.

**2026-09-24:** the owner approved the G8 designs (`docs/design/g8/APPROVAL.md`), which closes the last Phase 2 design input.
