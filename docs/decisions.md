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

## M1 — How the memory budget is measured (2026-09-25)

The owner asked for the best call on the two questions raised by the memory investigation (`docs/phase2-status.md`, “Memory”).

| Question | Decision |
|---|---|
| RSS or physical footprint | **Both must be under 400 MB.** §6.4 names resident memory, so RSS stays. RSS alone is unreliable on a Mac under memory pressure: macOS compresses idle pages out of it, so the same run read 206–420 MB. The physical footprint (Activity Monitor's “Memory”, compressed pages included) is stable to within a few MB and is the stricter of the two, so a pass on both means the app really fits. |
| One run or many | **Median and p95 of 20 runs, as §6.4 says**, each in a fresh launch with a fresh data folder (`scripts/perf-memory.sh`). The budget holds when the p95 of both measures is under 400 MB. A single `LINEN_SPIKE=m` run stays as the quick check. |

The GPU process's fixed ~90 MB (window compositing, present before a book opens) counts, as §6.4 says “all processes”.

## Approved 2026-09-25 (Phases 3–6 review)

The owner reviewed the pending approvals and approved them as recommended. The designs and values below are no longer provisional; the PROVISIONAL notes in the code refer to this record.

| # | Approved |
|---|---|
| Baselines | Phases 3–6 visual baselines (see `docs/visual/APPROVAL.md`). |
| G10 | The `?` cheat sheet as built. |
| ⋯ menu | Every command, grouped, with shortcuts; unavailable ones at 40%; extension items join in Phase 7. |
| Footnote peek | Notes are copied as safe structure (paragraphs, emphasis), not in the book's own styles. |
| G5 colours | The derived Sepia and Night values for panels, segments, popovers, tooltips and the selection bar, and the Paper segment ring in secondary ink. |
| E3 card | The damaged-book card, built in Phase 6. |
| G11 | Re-attach: a pill (“Select the passage for …” · Cancel), then a bar with Attach here and Cancel. |
| Phase 5 details | Icons in the selection bar in every theme; Delete in place of Search for a clicked highlight; the note dot in the underline colour; the 3 px jump pulse; the note status wording; a steady 2 px caret. |
| D2 | One library folder is the whole backup; export of all highlights and notes as W3C JSON; restore by putting the folder back; uninstalling leaves the folder, and Settings says so. |
| G4 | Remove is immediate with Undo; the file and annotations go at the next launch; duplicates open the existing book; the item menu on right-click and a hover/focus button; a native sort menu. |
| G3 | The book info sheet as built. |
| G2 | Settings sections: General, Reading, Library, Extensions, Shortcuts, About. |
| G12 | The Aa code-and-tables hint and the fixed-layout line. |
| S14 transition | The cover-grow transition and a warm book after the library move to Phase 8. |

## Approved 2026-09-26 (Phases 7–8 review)

The owner reviewed items 9, 10 and 21–28 and approved them as recommended, with item 28 settled as below. The PROVISIONAL notes for these in the code refer to this record.

| # | Approved |
|---|---|
| D4 (21) | Extension packages: a `*.linenext` zip with `manifest.json` at its root; no signing in the MVP (“Not verified by Linen”); the API reference in `docs/extensions/`; English only. |
| G1 (22) | Install, update and remove sheets as built. |
| G7 (23) | Extension surfaces as built: Navigator tab after Notes (“More” beyond one), the selection “⋯” menu, pinned commands in the ⋯ menu, built-ins listed and not removable, the suspended box, frames on the extension's own origin. |
| Baselines (24) | Phase 7 baselines: Screen 11 with extensions, Screen 12's extension failure (see `docs/visual/APPROVAL.md`). |
| P6 memory (25) | Accepted for the MVP: a Worker's memory cannot be measured in WebKit; CPU is budgeted through the heartbeat. |
| D1 (26) | No telemetry. A crash log on this Mac only (`~/Library/Logs/app.linen.reader/crash.log`), revealed from Settings › About. |
| D6 (27) | Tauri's updater, checked once a day, updates signed with the Linen update key and served from GitHub Releases; downloaded and installed in the background with a quiet “Update ready · Restart” line; a minor release about every six weeks. |
| Signing (28) | No Apple Developer ID yet. Builds and signing need only meet what GitHub requires: the release workflow on GitHub builds ad-hoc-signed DMGs and signed updater archives. Developer ID signing and notarisation switch on when their secrets are added. |
| X3 re-check (9), oldest Mac (10) | Approved by the owner without a recorded run. |
| File associations | Tried by the owner on the installed app: Open With and a book dropped on the Dock icon both work (2026-09-26). |
| Update source (29) | The owner made `Quentinbest/read-book` public (2026-09-26), so installed apps can reach its releases. The update address stays `https://github.com/Quentinbest/read-book/releases/latest/download/latest.json`; no separate releases repository is needed. |
| Screen 08 (30) | The Notes tab with the Phase 7 export footer is the new baseline (see `docs/visual/APPROVAL.md`). |

## Decided 2026-09-29 (immersive reading, like macOS Books)

The owner asked for immersive reading to match the macOS Books app. These override the plan where they differ; the code cites this record.

| Rule | Decided |
|---|---|
| S9 (top edge) | The top edge reveals the top bar alone, naming the book only (no chapter). It stays while the pointer is on the bar or in its 64 px zone and hides as soon as the pointer leaves; the 3 s delay (S10) applies only to the full controls (Tab, ⌘J, Aa), which still show both bars and the chapter. |
| S9, S11 (bottom edge) | No bottom trigger: the pointer at the bottom edge reveals nothing, so the Dock exclusion of S11 no longer applies. The bottom bar comes only with the full controls. |
| L9, B2, G12 (location line) | Immersive reading shows no location line. The chapter and “N min left” are in the bottom bar's labels, and so are a fixed-layout book's real pages. The “Opening …” line of a slow open (G8) stays. |

## Decided 2026-10-01 (edge reveal brings the progress bar back)

Testing 0.1.1, the owner found the progress bar missing when the toolbar is revealed, and chose to restore the bottom edge. This replaces the S9 and S9/S11 rows of 2026-09-29; the location-line row stands.

| Rule | Decided |
|---|---|
| S9 (edge reveal) | Either edge, after the 150 ms dwell, reveals both bars, as before 2026-09-29. The top bar names the book only. The bars stay while the pointer is in an edge zone or on a bar and hide as soon as it leaves. The full controls (Tab, ⌘J, Aa) are unchanged: chapter in the title, 3 s delay. |
| S11 (bottom zone) | With the Dock hiding at the bottom (or in full screen), the bottom zone stays but leaves out the 6 pt strip where the Dock slides in, instead of switching off; otherwise the bottom edge could not reveal anything on a Mac with an auto-hiding Dock. |

## Approved 2026-10-01 (release 1.1, and item 31)

The owner approved items 33–40 as recommended (“Go ahead”, then “Approve and fix”), then items 31 and 41 the same day. The details are in `docs/pending-approvals.md` and `docs/release-1.1-status.md`; the code cites this record.

| # | Approved |
|---|---|
| 1.1 Font, Page width (33) | Aa and Settings › Reading, all books: Book, Literata, Sans, OpenDyslexic (bundled, OFL); Narrow 56 / Normal 66 / Wide 74 ch. |
| C5 Publisher styles (34) | Full, Balanced (default), Off in Settings › Reading; Simplify Styles for This Book (Off for one book), with Undo. |
| Dictionary peek (35) | Look Up in the selection bar, the context menu and ⌃⌘D, from this Mac's dictionaries; Open in Dictionary; Search the book. Extension peek providers wait for a Host API addition. |
| Library list view (36) | Covers / List switch in the library header, remembered. |
| Reading sessions (37) | Permission `reading.sessions`, activation `onReadingSessions`, `linen.reading.on('sessionEnded')`; the title only with `book.metadata`; 5-minute idle end, 10-second minimum. |
| C4 remapping (38) | Settings › Shortcuts as built: modifiers required (function keys alone allowed), the system's chords reserved, extensions never take core shortcuts, a core command can take another's. This lifts the MVP's “remapping is deferred” rule (§2.8). |
| B6 search options (39) | Whole words and regular expressions under the search field. This replaces B6's 2026-09-24 “no space reserved”. |
| Baselines (40) | The 1.1 captures become baselines: `01-library`, `03-more-menu`, `05-navigator-search`, `06-selection-bar`, `09-reading-settings`, `12-damaged-book`, `12-extension-failure`, `14-night-selection`, `g10-cheat-sheet` (see `docs/visual/APPROVAL.md`). |
| Quitting with unsaved notes (31) | As built (review fixes, 2026-09-26): Quit and Restart wait for the reader's writes; after a failed save Linen stays open with “Some notes aren’t saved, so Linen stayed open · Quit Anyway”. Approved with its wording. |
| Settings captures (41) | `11-settings-reading` and `11-settings-shortcuts` become baselines (see `docs/visual/APPROVAL.md`). |

## Decided 2026-10-02 (item 42, TypeSafe experiments)

The owner left item 42 to Claude ("Make your best calls"). The evidence is in `docs/experiments/typesafe/`.

| Rule | Decided |
|---|---|
| E6 (author sort) | Authors sort by the publisher's sort form of the first author (`opf:file-as`, or a `file-as` meta refining the creator), read at import and stored in `books.author_sort` (schema 4). Libraries from before schema 4 read it in the background at launch. Without one, the last word of the name is the surname, but a name already written surname first keeps its order and Jr., Sr. and numerals are not surnames. No model is involved. |
| TypeSafe | Not built. Linen has no server, and the repository and app are public, so there is no safe way to ship an API key; asking readers for their own key does not suit a quiet reader, and every call would leave the device (Screen 12). The ⌘K fallback (45/50 against 17/50) is the case to revisit if Linen ever has a server. The opening-position experiment needs heading-level candidates for Gutenberg books first. |
