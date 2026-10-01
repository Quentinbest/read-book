# Release 1.1 status (macOS)

- **Last updated:** 2026-10-01
- **Asked for:** the owner, 2026-10-01: “Proceed with the recommended next steps, but leave out TTS, synchronization, and Developer ID signing for now.” That is: item 32, landing the logo, the plan's 1.1 list (§1.3) and the two deferred rules C4 (remapping) and B6 (search options).
- **Not built (by that instruction):** TTS and sync (plan §1.3 “1.2”), Developer ID signing and notarisation (item 28 stands).
- **Approvals:** items 33–40 approved by the owner on 2026-10-01, as recommended (`docs/decisions.md`). Item 31 is still waiting.
- **Branch:** `worktree-release-1-1`, draft PR #4.

## What was built

| Item | What | In-app check | Unit tests |
|---|---|---|---|
| Logo | `linen-a-v002` app icon (`src-tauri/icons/linen-a-v002/`), favicons, the wordmark in Settings › About. The brand record stays in `logo-project/`, outside the repository. | — | — |
| Item 32 | An install that can't replace itself shows “Linen x.y.z is available · Download” once per version; Download opens that release page (`updater.rs`). | `D6-update-available` | `updater::tests` |
| 1.1 Font, Page width (item 33) | Aa rows and Settings › Reading: Book, Literata, Sans, OpenDyslexic (OFL, bundled, loaded when chosen); Narrow 56 / Normal 66 / Wide 74 ch (`reader/typography.ts`). | `11-font-and-width`, `L4-aa-scope` | `typography.test.ts` |
| C5 Publisher styles (item 34) | Full, Balanced (default), Off; Simplify Styles for This Book, with Undo. Off disables the book's own sheets and style attributes in place (`engine.setPublisherStyles`). | `C5-simplify-styles` | `typography.test.ts` |
| Dictionary peek (item 35) | Look Up in the selection bar, the context menu and ⌃⌘D; Dictionary Services on this Mac (`dictionary.rs`); Open in Dictionary. | `11-look-up`, `A10-context-menu` | `dictionary::tests` |
| Library list view (item 36) | Covers / List switch, remembered (`libraryView`). | `11-library-list` | — |
| Reading sessions (item 37) | `reading.sessions` permission, `onReadingSessions`, `linen.reading.on('sessionEnded')` (`reader/sessions.ts`, extension host, worker, `docs/extensions/`). | `11-reading-sessions` | `sessions.test.ts`, `manifest::tests` |
| C4 remapping (item 38) | Settings › Shortcuts: Change, Remove, Reset, Reset All; overrides in the registry, so the menu bar, ⌘K, the cheat sheet and keys follow (`lib/commands/remap.ts`). | `C4-remap` | `remap.test.ts` |
| B6 search options (item 39) | Whole words; regular expressions (case ignored, text as written); an invalid pattern says so (`lib/search/search.ts`). | `B6-search-options` | `options.test.ts` |

## Verification

- **Local CI** (`scripts/ci-local.sh`): all green. 298 TypeScript tests (270 before), 66 Rust tests (62 before), clippy with `-D warnings`, rustfmt, Prettier, ESLint, svelte-check, the design export check.
- **GitHub CI on PR #4:** lint, format, type-check and design export; integration tests (Chromium, WebKit); the macOS build: all passed.
- **In-app suite, first full run (current desktop):** 112 of 116. The four failures:
  - `A10-context-menu` and `L4-aa-scope` expected the MVP's context menu and Aa rows; both now expect the 1.1 items (Look Up; Font, Page width).
  - `L16-long-chapter` (500 ms, budget < 500) and `F3-F8-streaming-persisted` (305 ms, budget 300) ran while `scripts/ci-local.sh` compiled on the same Mac; rerun alone, both pass.
- **In-app suite, second full run (current desktop):** 114 of 116. A10, L4, L16 and F3-F8 pass. Two failures:
  - `F5-results-land` (passed in the first run): rerun with the other search checks, all of F-golden, F2, F3-F8, F5, F6 and F7 pass. Runs on the active desktop are known to fail unrelated checks now and then (the live pointer and focus); Desktop 2 avoids that.
  - `D1-no-uncaught-errors`: one `TypeError` in `focusView`, logged about six minutes before the 1.1 checks ran: the reader focused the page on a frame after its paginator had lost its view. `ReaderEngine.focusPage` now returns when closed and ignores a paginator without a view; the rerun has no uncaught error.
- **Visual (Desktop 2, `LINEN_SPACE=2 scripts/e2e.sh v`, `node tests/visual/compare.mjs`):** 20 of 30 compared captures match. The 10 that differ:
  - **On purpose (item 40):** `01-library` and `12-damaged-book` (the Covers / List switch), `03-more-menu` (Simplify Styles for This Book), `05-navigator-search` (the options row), `06-selection-bar`, `14-night-selection` and `12-extension-failure` (Look Up in the selection bar), `09-reading-settings` (Font and Page width), `g10-cheat-sheet` (Look Up Selection, ⌃⌘D).
  - **Not 1.1:** `17-goto` differs only in the field's focus ring and the traffic lights (active window).
  - The new captures are in `docs/visual/app/`; diffs in `docs/visual/diff/` (not committed).

## After approval (2026-10-01)

- **Short windows:** the Aa popover now stops 12 px above the window's foot and its rows scroll; the arrow and the foot stay put. At full height it is pixel-identical to its baseline. Check `11-aa-short-window` (1100 × 560).
- **Esc and ⌘, are not remappable:** Settings › Shortcuts listed Close layer (Esc) with Change and Remove. Commands on a reserved chord are now left out, and the registry ignores a stored override for them (K14, G2; `remap.test.ts`).
- **Settings captures:** `11-settings-reading` and `11-settings-shortcuts` are new captures with no baseline yet (item 41).
- After these: `C4-remap`, `L4-aa-scope`, `11-font-and-width`, `11-aa-short-window` and `D1-no-uncaught-errors` pass; the visual comparison has only the known `17-goto` focus-ring difference.

## Known gaps

- **Recording a ⌘ chord in the separate Settings window** relies on WebKit taking the key before the menu bar. The harness records in the main window, so this is not verified in the real Settings window (item 38).
- **A pathological regular expression** can keep the search worker busy; JavaScript can't interrupt one. Only search waits (item 39).
- **Extension peek providers** (dictionary or translation from an extension) and **metadata providers** from the plan's 1.1 list are not built; they need a Host API addition.
- **Commit f9316d4 (C4)** does not type-check on its own: its strings landed in the next commit (7a4c9c7). The branch tip is fine.
