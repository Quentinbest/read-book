# Phase 6 status: settings and library (macOS)

- **Last updated:** 2026-09-25
- **Before starting:** D2 was not decided, and the G2, G3, G4 and G12 designs did not exist. Each is built to a written recommendation, marked provisional, and set aside for the owner (`docs/pending-approvals.md`, items 14–18).
- **Evidence:**
  - The in-app suite (`LINEN_SPIKE=r`, checks `L4-*`, `K8-*`, `L18-E2-*`, `E*`, `G2-*`).
  - Unit tests (`src/lib/library/*`, `src/reader/textSizes.test.ts`, the Rust store, command and extension tests).
  - `scripts/perf-coldstart.py`, Spike G (`LINEN_SPIKE=g`), and the visual spike.

## Done-when (plan §5, Phase 6)

| Item | Status | Evidence |
|---|---|---|
| Aa scope rules hold (size, theme and line spacing change all books; layout this book only) and persist across restarts | Done | e2e L4-aa-scope: size, Sepia and Loose carry to another book, while Scroll stays with its book. Each row names its scope. The choices come back on reopening, which reads them from the store, as a restart does. e2e K8-text-size covers ⌘+ ⌘− ⌘0. |
| Library search and sort give the expected order on a 500-book fixture | Done | `order.test.ts`: Recent, Title (without leading articles, numeric) and Author (surname) on 500 books, complete, stable and in order, in under 100 ms; search folds accents and case. e2e E6-library checks the real grid. |
| Removing a book follows G4, including what happens to its file and annotations | Done (G4 provisional) | **In the app:** e2e E7-remove-undo — Remove is immediate, and ⌘Z or the message's Undo brings the book back. **Rust:** a store test (hidden, restored, purged with its annotations and position) and a command test (at the next launch the library copy and cover are deleted; a file outside the library never is). |
| The cold-start budget holds with the 500-book fixture | Done | `scripts/perf-coldstart.py`, 20 launches each (`docs/spikes/raw/budget-coldstart.json`): the table below. |
| The Spike G report passes | Done | `docs/spikes/g-extension-network.md`: nothing reaches the network or IPC from an extension Worker or UI page. |
| Visual baselines for Screens 01, 09, the Screen 11 shell and the Screen 12 empty and damaged states are approved | **Waiting for the owner** | `docs/visual/phase6-review.html`; `docs/pending-approvals.md` item 19 |

Cold-start results, 20 launches each:

| Launch to | Median | p95 | Budget |
|---|---|---|---|
| Library | 556 ms | 651 ms | 1 s |
| Last book | 498 ms | 565 ms | 1.5 s |

## Work items

| Item | Status |
|---|---|
| Aa popover | Done: size in the twelve L4 steps (⌘+ on to 48 px), theme, line spacing and layout, each with its scope; ⌘+ ⌘− ⌘0 anywhere; the code-and-tables hint (L18) and the fixed-layout line (E2), both G12 provisional; “More in Settings…”. |
| Library (E6–E9) | Done: <ul><li>**Screen 01 layout:** Continue reading (the current book with its chapter and when it was opened; the next two), and All books with its count.</li><li>**Covers:** real ones, or generated in Screen 01's tints.</li><li>**Search and sort** (remembered).</li><li>**The item menu** (Book info, Show in Finder, Remove), with Remove and Undo.</li><li>**Return** resumes the current book.</li><li>**The damaged-book card** (E3).</li></ul> The 240 ms cover-grow transition and keeping the book warm after going to the library (S14) are **not built**: see below. |
| Book info sheet (E10, G3) | Done (G3 provisional): metadata, file size, the description, and the accessibility metadata in plain words |
| Preferences shell (G2) | Done (G2 provisional): a Settings window (⌘,) with General, Reading, Library, Extensions (shell), Shortcuts and About. It holds the single-key shortcut switch, the page-turn fade and the page-turn announcements. Changes reach the open book at once. Export of all highlights and notes (D2 provisional). |
| Spike G | Passed (see above) |

## Not built, and why

- **The 240 ms cover-grow transition, and a warm book after going to the library (S14).** Both need the reader kept alive behind the library. The reader is recreated for each book today (`{#key}`), and the cold-start measurements show that reopening costs about 500 ms. Moving to a kept-alive reader is a larger change than this phase's other items. I recommend doing it together with the transition, early in Phase 8 (polish), and have noted it in the approvals list.
- **The page-turn crossfade is a fade-in of the new page (120 ms).** A true cross-fade needs a picture of the old page, which a WebView cannot take cheaply. It is off by default, as V8 has it.
- **“Scaled by the OS text size” (L4).** macOS has no system-wide text size for apps to follow, apart from accessibility zoom, which already scales the whole window. There is nothing to read.
- **The Settings window is checked in the app's own page.** The e2e harness drives one WebView, so the check mounts the Settings page inside it; opening the real window is covered only by ⌘, reaching its command. One manual look at the real window is part of item 19.

## Found and fixed

- **Raster covers in the Navigator never showed.** They were requested from the zip by their extracted file's path. Covers are now served from the library's Covers folder (`/_cover/<id>` on the book scheme).
- **The library's first paint at 500 books had a slow tail** (p95 1.16 s). Tiles below the fold now skip layout and paint (`content-visibility`), covers load lazily, and the first IPC calls run together: p95 651 ms.
- **Generated covers now use Screen 01's tints,** tested for ≥ 7:1 lettering. The core and the UI share the list, and a Rust test keeps them in step.
- **A settings change could undo itself.** Each window heard its own changes come back as events. Pressing ⌘+ quickly could apply a stale echo after a newer size (found by K8-text-size in a full run). Changes now carry their source, and a window ignores its own.
- **The code-and-tables measurement kept running after its book closed.** It piled up across books in a long session and slowed everything (the first full run failed a dozen checks from it). It now starts 5 s after opening and stops when the book closes.
