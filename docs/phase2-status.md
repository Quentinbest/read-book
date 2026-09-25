# Phase 2 status — reader core (macOS)

- **Last updated:** 2026-09-25
- **G8 designs approved** 2026-09-24 (`docs/design/g8/APPROVAL.md`) and built, except the image view (Phase 3) and pinch zoom (needs the native gesture bridge).
- **Evidence:** the in-app end-to-end suite (`src/spikes/e2e.ts`, run with `LINEN_SPIKE=r` against a throwaway data folder), the memory run (`LINEN_SPIKE=m`), and the unit tests (`pnpm test`, `cargo test`).
- **Latest suite result:** 38 of 38 checks pass, on two consecutive runs (`docs/spikes/raw/e2e-reader.json`). Real macOS scroll events are now posted to the app in tests (`spike_scroll_wheel`), so the wheel path through AppKit and WebKit is covered, not only the bridge's output.

## Done-when (plan §5, Phase 2)

| Item | Status | Evidence |
|---|---|---|
| Unit tests assert every I-rule threshold and L1–L10 | Done | `src/reader/layout.test.ts`, `src/lib/input/*.test.ts`, `src/reader/pace.test.ts` |
| Scripted end-to-end page turns by key, wheel, click and touch zone, LTR and RTL; clicks in the column and the activating click never turn a page | Done except touch zones | e2e: I8 keys (4 checks), I11 margins, activating click, column click, I1 wheel, I2/I5 trackpad, I15 RTL keys and margin. Touch zones: see below. |
| Quitting mid-chapter and reopening restores the same CFI; a new book opens at bodymatter | Done | e2e: N5-quit-save, N5-restore, progress-saved, N3-bodymatter |
| Resize, font change and text spacing keep the reading position | Resize done; font change covered by Spike D; the Aa popover arrives in Phase 6 | e2e: L10-resize; Spike D: 200/200 CFIs through font 16 → 24 → 16 px |
| A fixed-layout book scales, spreads and zooms | Done (G8): fits inside 24/64 px margins with a 4 px seam and page shadow; ⌘+ ⌘− ⌘0 zoom 150–400% with the 2 s chip; a zoomed page fills the window and pans by wheel, trackpad and drag; a turn returns to fit; the line shows real pages. Pinch waits for the gesture bridge | e2e: E2-fixed-layout, I17-fixed-zoom; app captures `docs/visual/app/g8-fxl-*.png` |
| A 1 MB+ chapter opens within the open-book budget and shows “≈” until pagination completes | Done: 250–340 ms to the first laid-out page (was 2.1–2.35 s). Pages are counted in idle time in a hidden view (B1); until every chapter is counted, and always inside chunked chapters, numbers show “≈” (“Back to page ≈N”, “About page N”). Moby-Dick settles in about 12 s | e2e: L16-long-chapter, L16-chunk-traversal (3 chunk boundaries forward and back, CFIs strictly increasing), L16-deep-jump-restore |
| §6.4 budgets that apply from Phase 2 | Open book, reflow, page turn and memory pass; cold start to the last book waits for Phase 6's library. Memory: after 50 pages of the 100 MB book, RSS 261–319 MB and physical footprint 334–340 MB over five runs (2026-09-25), after book media moved off blobs (see “Memory” below) | e2e: budget-open (p95 77–118 ms), budget-reflow (< 150 ms), I6-turn-budget (p95 5 ms within and 1 ms across chapters); memory run (`LINEN_SPIKE=m`), trace (`LINEN_SPIKE=mt`) |
| Hostile corpus EPUBs still fail as in Spike E | Done | e2e: E-hostile-in-product (no script, IPC or network; the fixed overlay is neutralised) |
| Visual baselines for Screens 02, 03, 10 and 14 approved | Done: approved 2026-09-24 | `docs/visual/APPROVAL.md`, `baselines/webkit-19618/`. A recapture on 2026-09-25 matches exactly (0 pixels differ, `tests/visual/compare.mjs`) |

## Work items

| Item | Status |
|---|---|
| ReaderEngine adapter (open, render, paginate, CFI, reflow, §7.1 frames) | Done: `src/reader/engine.ts`. Content hooks (per-document CSP, L13/L14 sanitiser), links routed by the app (N10), one queued turn (I6), neighbour pre-layout (D-D1), on-demand loading (L17) |
| Canvas L1–L17 | Done (`layout.ts`, `styles.ts`, `fonts.ts`, `loader.ts`, `chunks.ts`). L16: chapters over 1 MB are laid out in chunks of about 60,000 characters; hidden blocks stay in the document (`display: none`), so CFIs, links and saved positions are unchanged. foliate-js carries one patch (`patches/`) so its visible-range search skips hidden blocks |
| Fixed layout (E2) | Done, per G8 (above). foliate-js's fixed-layout contents lack section indices; the engine maps documents to sections |
| Immersive state and location line (L9, B2, S12) | Done |
| Chrome (S9–S11, S15) | Done: edge reveal with dwell, Tab, auto-hide, macOS full-screen and Dock edges (T8), overlay title bar with the window buttons in the top bar (Screens 02/03). Controls whose features arrive later are hidden. The controls leave with a 220 ms fade (100 ms under reduced motion) |
| Input router (I1–I15) | Done for keys, wheel and trackpad (native bridge), margins, RTL, turn queue and the rapid-turn chip. **Touch (I12–I14) not applicable on macOS** (no touch screens); deferred with the tablet release |
| Scroll mode (B8) and the two-page spread (L8, B12) | Done, per G8. Spread from 1480 px: two 640 px pages and a 48 px gutter, measured on screen. Scroll mode: View › Scroll Mode / Pages Mode (the Aa control arrives in Phase 6), remembered per book (S13); chapters stack in one native scroller with the 124 px join and hairline, fades at both edges, Space = a screen less two lines, ↓ ↑ = 3 lines, margins inert. Wheel and trackpad drive it from AppKit's stream because WebKit does not pass the wheel from the book frames to the scroller (found with real posted events). Vertical-writing and fixed-layout books stay in Pages |
| Progress persistence (N4, N5), Back (N1, N2) | Done |
| Screen-reader sync (X3), page-turn announcements | Done: external scrolls are followed while VoiceOver runs; announcements have a setting (`pageTurnAnnouncements`). Not yet re-verified with VoiceOver in the product |
| Night chrome (V2), motion (V8, V9) | Done |

## Open items and what they need

1. ~~L16 long chapters~~: done (chunked layout). The “≈” visual treatment is part of G8.
2. ~~G8 designs~~: approved 2026-09-24 and built (app captures `docs/visual/app/g8-*.png` match the approved frames). Remaining: the image view (Phase 3) and pinch zoom (native gesture bridge).
3. ~~Visual baselines~~: approved 2026-09-24.
4. **VoiceOver re-check** of the product's X3 sync: the harness is ready (`scripts/run-spikes.sh x3`, about 5 minutes). It starts three pages before a chapter end and records location, snapping, saving and announcements automatically; the person answers three questions. Results go to `docs/spikes/raw/x3-voiceover.json`.
5. ~~Memory budget margin~~: fixed 2026-09-25 (below). Decision for the owner: whether §6.4's “resident memory” should be read as RSS, as today, or as physical footprint (see below).

## Fixes found by the L16 checks

- The open-time budget checks stopped at the previous reader's location; they now wait for the new reader's first laid-out page (budget-open p95 is still 87–102 ms).
- Reopening a book right after leaving it could read the position before the closing save landed; the reader now waits for pending writes (N5).
- Back chips outlived their book and could call a closed reader; the reader withdraws them when it closes.

## Also added with G8

- RTL books fill the progress bar from the right and swap its labels; an open over 500 ms shows one “Opening “Title”…” line.
- e2e: synthetic Esc now carries its key code (commands match physical keys), so the visual capture's “hide controls” really presses Esc.

## Memory (2026-09-25)

- **Measurement.** The check summed RSS. This Mac (8 GB) runs with about 9 GB of swap in use, and macOS compresses idle pages out of RSS, so one run's RSS said as much about system pressure as about the app: WebContent's RSS fell from 115 to 28 MB in ten seconds of idle while nothing was freed. `spike_memory` now also reports each process's physical footprint (Activity Monitor's “Memory”, compressed pages included), which varied by only 6 MB across five runs. `LINEN_SPIKE=mt` traces both, plus live blob URLs, while reading and at idle.
- **Cause.** Footprint showed WebKit's Networking process holding 110–180 MB while the book's live blob URLs totalled 41–51 MB. WebKit keeps blob data in that process, and each image existed as an IPC buffer, our loader's Blob and foliate's Blob; revoked blobs are released only after a garbage collection in the page, which rarely comes because the bytes are not on the JavaScript heap. Idle page counting added churn when it laid out the image chapters.
- **Fix.** Book images, audio and video now load straight from the zip through the `linen-book:` URL scheme (`commands::serve_book_media`), so they never become blobs. It serves media types only (never documents, styles, scripts, SVG or fonts, which keep the sanitiser and the font decoder), only for the book that is open, with `nosniff` and a sandbox CSP; the per-document and app CSPs allow it for `img-src` and `media-src` only. foliate-js's loader takes a `urlFor` hook (patch), skipped for encrypted (obfuscated) entries.
- **Result.** Networking 7–13 MB (was 110–180); footprint after 50 pages 334–340 MB (was 394–408), and about 348 MB after 10 s of idle (was about 445); RSS 261–319 MB (was 330–416), five of five under 400 MB. `Cache-Control: no-store` on the scheme was tried and made it worse (each view decoded its own copy), so WebKit's cache stays on.
- **Method gap still open.** §6.4 asks for the median and p95 of 20 runs on the reference machines; the check is a single run. The GPU process holds about 90 MB before any book opens (window compositing), a fixed cost inside the budget.
