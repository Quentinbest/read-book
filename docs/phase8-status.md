# Phase 8 status: hardening and release (macOS)

- **Last updated:** 2026-09-26
- **Before starting:**
  - The plan asks for D1, D3, D5 and D6 to be decided.
  - D3 and D5 were decided on 2026-09-24.
  - D1 (item 26) and D6 (item 27) are built to a written recommendation and marked provisional.
  - Signing (item 28) needs the owner's credentials.
- **Evidence:**
  - The in-app suite, run on Desktop 2 (`scripts/e2e.sh`).
  - Unit tests: `pnpm test` and `cargo test`.
  - The budget scripts.

## Done-when (plan §5, Phase 8)

The release checklist in §6.5 passes, on macOS only (the scope decision).

### The release checklist (§6.5), 2026-09-26

| Item | State | Evidence |
|---|---|---|
| Every phase's Done-when list still passes | **Passes** | <ul><li>**In-app suite:** 107/107 on Desktop 2, the last check being that nothing uncaught happened.</li><li>**Unit tests:** 259 in TypeScript and 48 in Rust.</li><li>**Visual:** of the 22 captures with an approved baseline, 16 match. The 6 that differ are explained: the inactive window on Desktop 2 (focus rings, grey selection, traffic lights) and the clock (“Opened today at …”). Screen 11's Extensions pane and the Notes tab's export footer are compared with pre-Phase 7 baselines, because the Phase 7 baselines wait for item 24.</li></ul> |
| Performance budgets hold | **Pass on this Mac** (M2, 8 GB, macOS 14.6) | <ul><li>**Cold start:** library p95 678 ms (budget 1000); book p95 658 ms (budget 1500); with 500 books.</li><li>**Memory:** RSS p95 376 MB, footprint p95 349 MB (budget 400); 20 fresh launches.</li><li>**Open and reflow:** `budget-open` and `budget-reflow` in the suite.</li></ul> The oldest reference machine (macOS 13 with Safari 16.4) is still to be run (item 10). |
| The accessibility target is met, with manual runs recorded | **Automated part passes; the manual run waits** | axe WCAG 2.2 AA, X5 and X6 pass. The VoiceOver run needs a person (item 9). |
| Hostile corpus results are unchanged | **Pass** | Rust `corpus_outcomes` and `every_corpus_file_imports_or_is_refused`; e2e `E-hostile-in-product` and `P2-hostile-extension` |
| The backup and restore round-trip passes | **Pass** | Rust `a_library_folder_restored_elsewhere_still_opens_its_books`; e2e `A9-w3c-roundtrip` for export |
| Builds are signed and notarised; file associations work | **Waits for the owner** | <ul><li>**Signing:** needs the credentials (item 28).</li><li>**What passes:** `scripts/release-macos.sh --unsigned` builds the app and the DMG. The bundle declares `.epub` with `org.idpf.epub-container` (Viewer), and `LSMinimumSystemVersion` is 13.0.</li><li>**Not tried:** Open With and Dock drop on the installed app. They would open the real library on this Mac, so they are for the owner, with the signed build.</li><li>**The updater (D6)** is not built (item 27).</li></ul> |

**Phase 8 is not done.** Everything that does not need the owner is done and verified. Six items wait on the owner: 9, 10, 24, 26 (provisional, built), 27 and 28.

## Work items

| Item | Status |
|---|---|
| Accessibility pass against D5 (WCAG 2.2 AA; X1 AAA body contrast) | **Automated part done.** **axe:** WCAG 2.2 A and AA over the library, the reader, its layers, and every Settings section, in Paper and Night (`X-axe-states`). **Zoom:** 200% and 400% (`X6-zoom`). **Text spacing:** WCAG 1.4.12 (`X5-text-spacing`). **Manual:** a VoiceOver run needs a person (pending item 9). NVDA, Orca and Windows high contrast (X7) are deferred with their platforms. |
| EPUB edge cases: vertical writing (I16), RTL, huge books, broken CSS and fonts | Done. The details are below. |
| The P§22 failure table | Done. Every row maps to a check; see below. |
| Performance budgets on the reference machines | Open |
| Backup and restore (D2); uninstall | Done; see below. Restoring the folder elsewhere used to break every book; fixed. |
| Packaging: signing and notarisation, file associations, auto-update (D6) | **Partly done.** **File associations:** Open With and Dock drop, through the `.epub` association and the `org.idpf.epub-container` type; the minimum macOS is 13.0. **Release script:** `scripts/release-macos.sh`. **Waiting:** signing needs the owner's credentials (item 28); the updater is not built, pending D6 (item 27). |
| D1: crash reporting | Built to the recommendation (item 26): a crash log on this Mac only; see below |

## EPUB edge cases

- **Vertical writing (I16).** Pages run right to left, and ruby is kept. ← turns forward. **Scroll now runs sideways:** the engine uses foliate-js's scrolled flow inside the page box, one chapter at a time. Wheel, ↓ and Space move along the line of text. Past a chapter's end, the next chapter loads. A vertical book remembered in Scroll reopens sideways; before this change, vertical books stayed in Pages. Check `I16-vertical`, with 草枕 from the corpus.
- **Right to left.** Checks `I15-rtl-keys`, `I15-rtl-margin` and `I15-rtl-progress`.
- **Huge books.**
  - Long chapters: checks `L16-long-chapter`, `L16-chunk-traversal` and `L16-deep-jump-restore`.
  - A library of 500 books: `scripts/perf-coldstart.py`, which uses the `seed500` spike.
- **Broken CSS.** The sanitiser (L14) is covered by the unit tests `src/reader/content.test.ts`. In the product, the check `E-hostile-in-product` is a book whose styles try scripts, network requests and fixed positioning.
- **Broken fonts.** Check `L15-font-fallback` covers the 1.5 s timeout and missing glyphs.

## Carried over from earlier phases

- **The warm book (S14) and the cover grow (V8).**
  - After the library, the last book stays open behind it: hidden with `opacity: 0` and `inert`, not unloaded. Opening it again is instant, at the same place.
  - While it waits, it takes no keys, wheel or commands, and extensions do not see it. It also does not count library time as reading time (B2).
  - Opening another book lets it go, and so does removing it or replacing its file.
  - Library → book is a 240 ms cover grow: a copy of the cover grows to the window's height and fades over the opening reader. There is no movement under reduced motion.
  - The check is `S14-warm-book`. The other checks open books cold (`noWarm`), so restore paths stay tested.
- **Scroll-mode restore (B8) is robust.**
  - Phase 7 found the reopened position 2% early after one extra `await`. The cause: a relayout that lands while the stack is still being built re-navigated to `location`, which at that moment can belong to a neighbouring chapter.
  - The fix pins the place a navigation asked for until the reader moves. Relayouts and late height changes return to that place, and an older stack build gives way to a newer one.
  - `B8-scroll-mode` now forces relayouts at 0, 40, 120, 300 and 700 ms into the open. Without the fix it failed at 120 ms: 4.8% against 7.1%.
- **The “update Safari” message (D7-WebKit).** On WebKit older than Safari 16.4 (no regex lookbehind), opening a book shows “Books need a newer Safari”, with “Open Software Update”, instead of a blank reader. Check `D7-webkit-message`, with the old engine simulated.

## Backup, restore and uninstall (D2)

- **The folder is the backup.** `~/Library/Application Support/app.linen.reader` holds the database, the books, the covers and the extensions. Extension storage is in the database. Nothing is in WebView storage.
- **Restore.** Put the folder back, then open Linen.
  - The store recorded absolute paths, so a folder restored under another user name (a new Mac) found no book files. Books and covers are now found by name inside the library as it is now. Nothing outside it is served.
  - Covered by the Rust test `a_library_folder_restored_elsewhere_still_opens_its_books`.
- **Export.** Settings › Library › Export all highlights and notes writes W3C Web Annotation JSON; covered by `A9-w3c-roundtrip`.
- **Uninstall.** Moving Linen to the Bin leaves the folder, and Settings › Library says so. To remove everything, also delete the folder above and `~/Library/Logs/app.linen.reader`.

## Crash log (D1, provisional)

Nothing is sent anywhere.
- **What is logged.** Panics, with a backtrace, and uncaught page errors go to `~/Library/Logs/app.linen.reader/crash.log`. The log starts again at 1 MB.
- **Where it shows.** Settings › About shows the privacy line, and “Show crash log” when a log exists.
- **Checks.**
  - `D1-crash-log`: an uncaught error and a rejection reach the log.
  - `D1-no-uncaught-errors`, the last check of every run: the log holds nothing else.
- **What it found at once.**
  - foliate-js threw from late callbacks for views it had already destroyed, about 20 times a run. It is guarded in the foliate patch.
  - A reader read its book after teardown; see “Found and fixed”.
  - WebKit's “ResizeObserver loop completed” notice is not logged: it is not an error.

## The P§22 failure table

| Failure | Where it is checked |
|---|---|
| Extremely long chapter | `L16-long-chapter`, `L16-chunk-traversal`, `L16-deep-jump-restore` |
| Missing metadata | Rust `import::tests::missing_metadata_falls_back_to_the_first_heading_and_a_generated_cover`. The recovery (editing title and author) is superseded: metadata is read-only (Q4). |
| Broken or missing TOC | `N6-contents-ncx`, `N6-contents-headings` (“Generated from headings”) |
| Invalid publisher CSS | `src/reader/content.test.ts` (the sanitiser: fixed positioning, remote loads, absolute font sizes); `E-hostile-in-product`. The contrast check is by construction: the reader styles give every element the theme's ink and a clear background (`src/reader/styles.ts`), so a low-contrast publisher colour never reaches the page. The recovery (the Publisher styles setting and “Simplify styles”) is in 1.1 (C5). |
| Corrupted book | `E3-damaged-card`, `N6-contents-damaged`; Rust `epub::tests` |
| Unsupported font | `L15-font-fallback` |
| Layout breaks after a font change | `P22-font-change-place`, which is new: through ⌘+ ×4, ⌘0, ⌘− ×2 and a line-spacing change, the reading position stays on screen. Also `A9-reflow`. v1 has no typeface choice (1.1), so the changes are size and spacing. |
| Slow search indexing | `F3-F8-streaming-persisted` |
| Annotation anchors break | `A9-reimport` (“Couldn’t place”, Re-attach), `A9-reflow`, `A9-w3c-roundtrip` |
| Plugin crash or hang | `P6-watchdog`, `P2-hostile-extension`, `P7-safe-mode` |
| Aggressive resizing | `L10-resize`, `budget-reflow` |
| Rapid page turns | `I7-rapid-chip`, `I6-turn-budget` |
| Trackpad momentum | `I2-I5-trackpad`, `I7-rapid-chip` (Back chip) |
| Save failure | `src/app/writes.test.ts` (one Retry message; nothing discarded while the disk is full); the store is SQLite in WAL mode with one transaction per write (Spike F: no committed write lost across 200 SIGKILLs) |

## Found and fixed

- **Restoring the library folder elsewhere (D2).** It used to lose every book; see above.
- **The Scroll-mode restore race (B8).** See above.
- **Late foliate-js callbacks (D1).** Uncaught errors from views that were already destroyed. The gate found them in `View.render` and `expand`, in `Paginator.render`, and in `setStyles`' deferred background.
- **A reader's last save could name the wrong book (found by the D1 gate).**
  - The reader read its `book` prop live. While a warm book was torn down because another book replaced it, the prop already named the new book, so the old reader's closing save would have written its place onto the new one.
  - The reader now takes its book once, since each reader is keyed to one book. `S14-warm-book` checks that both books keep their own places.
- **Display sleep stalls long runs.** A sleeping display stops WebKit's `requestAnimationFrame`, so a run longer than the 10-minute display-sleep delay stalled while nobody was at the Mac (one check waited 104 s). `scripts/e2e.sh` now runs the app under `caffeinate -di`.

- **axe findings (accessibility pass).**
  - The Contents panel had no tabpanel id.
  - Key caps were 3.9:1 in Night; they now use the `keyInk` token.
  - White on the Night accent was 2.4:1; it now uses the `onAccent` token.
- **X6.** At 400% zoom the library scrolled sideways. The library and Settings now have a narrow form.
