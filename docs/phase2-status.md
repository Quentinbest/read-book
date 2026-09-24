# Phase 2 status — reader core (macOS)

- **Last updated:** 2026-09-24
- **Started without the G8 designs,** on the owner's instruction to go ahead. Parts that need those designs are marked.
- **Evidence:** the in-app end-to-end suite (`src/spikes/e2e.ts`, run with `LINEN_SPIKE=r` against a throwaway data folder), the memory run (`LINEN_SPIKE=m`), and the unit tests (`pnpm test`, `cargo test`).
- **Latest suite result:** 30 of 30 checks pass, on three consecutive runs (`docs/spikes/raw/e2e-reader.json`).

## Done-when (plan §5, Phase 2)

| Item | Status | Evidence |
|---|---|---|
| Unit tests assert every I-rule threshold and L1–L10 | Done | `src/reader/layout.test.ts`, `src/lib/input/*.test.ts`, `src/reader/pace.test.ts` |
| Scripted end-to-end page turns by key, wheel, click and touch zone, LTR and RTL; clicks in the column and the activating click never turn a page | Done except touch zones | e2e: I8 keys (4 checks), I11 margins, activating click, column click, I1 wheel, I2/I5 trackpad, I15 RTL keys and margin. Touch zones: see below. |
| Quitting mid-chapter and reopening restores the same CFI; a new book opens at bodymatter | Done | e2e: N5-quit-save, N5-restore, progress-saved, N3-bodymatter |
| Resize, font change and text spacing keep the reading position | Resize done; font change covered by Spike D; the Aa popover arrives in Phase 6 | e2e: L10-resize; Spike D: 200/200 CFIs through font 16 → 24 → 16 px |
| A fixed-layout book scales, spreads and zooms | Opens and turns pages; zoom and the layout design wait for G8 | e2e: E2-fixed-layout |
| A 1 MB+ chapter opens within the open-book budget and shows “≈” until pagination completes | Done: 250–340 ms to the first laid-out page (was 2.1–2.35 s); page numbers in the chapter are marked approximate (“Back to page ≈N”, “About page N”). The visual treatment of “≈” waits for G8 | e2e: L16-long-chapter, L16-chunk-traversal (3 chunk boundaries forward and back, CFIs strictly increasing), L16-deep-jump-restore |
| §6.4 budgets that apply from Phase 2 | Open book, reflow, page turn and memory pass; cold start to the last book waits for Phase 6's library (“Resume reading”) | e2e: budget-open (p95 < 500 ms), budget-reflow (< 150 ms), I6-turn-budget (p95 5 ms within and 1 ms across chapters, 180/180 turns exact); memory run: 290–371 MB |
| Hostile corpus EPUBs still fail as in Spike E | Done | e2e: E-hostile-in-product (no script, IPC or network; the fixed overlay is neutralised) |
| Visual baselines for Screens 02, 03, 10 and 14 approved | Done: approved 2026-09-24 | `docs/visual/APPROVAL.md`, `baselines/webkit-19618/`. An independent recapture matches within 0.03% (`tests/visual/compare.mjs`) |

## Work items

| Item | Status |
|---|---|
| ReaderEngine adapter (open, render, paginate, CFI, reflow, §7.1 frames) | Done: `src/reader/engine.ts`. Content hooks (per-document CSP, L13/L14 sanitiser), links routed by the app (N10), one queued turn (I6), neighbour pre-layout (D-D1), on-demand loading (L17) |
| Canvas L1–L17 | Done (`layout.ts`, `styles.ts`, `fonts.ts`, `loader.ts`, `chunks.ts`). L16: chapters over 1 MB are laid out in chunks of about 60,000 characters; hidden blocks stay in the document (`display: none`), so CFIs, links and saved positions are unchanged. foliate-js carries one patch (`patches/`) so its visible-range search skips hidden blocks |
| Fixed layout (E2) | Basic open and turn; zoom (⌘+, pinch, I17) and the layout design wait for G8 |
| Immersive state and location line (L9, B2, S12) | Done |
| Chrome (S9–S11, S15) | Done: edge reveal with dwell, Tab, auto-hide, macOS full-screen and Dock edges (T8), overlay title bar with the window buttons in the top bar (Screens 02/03). Controls whose features arrive later are hidden. The 220 ms hide animation is still to do |
| Input router (I1–I15) | Done for keys, wheel and trackpad (native bridge), margins, RTL, turn queue and the rapid-turn chip. **Touch (I12–I14) not applicable on macOS** (no touch screens); deferred with the tablet release |
| Scroll mode (B8) and the two-page spread (L8, B12) | Layout supports the spread; both wait for the G8 designs |
| Progress persistence (N4, N5), Back (N1, N2) | Done |
| Screen-reader sync (X3), page-turn announcements | Done: external scrolls are followed while VoiceOver runs; announcements have a setting (`pageTurnAnnouncements`). Not yet re-verified with VoiceOver in the product |
| Night chrome (V2), motion (V8, V9) | Night chrome and chrome reveal done; the hide animation is still to do |

## Open items and what they need

1. ~~L16 long chapters~~: done (chunked layout). The “≈” visual treatment is part of G8.
2. **G8 designs:** provisional proposals for all eight variants are drawn inside the approved system and await the owner's review: `docs/design/g8/review.html` (frames in `proposals.html`, captured by `tests/visual/capture-g8.mjs`). Scroll mode, the spread and fixed-layout zoom are built once approved.
3. ~~Visual baselines~~: approved 2026-09-24.
4. **VoiceOver re-check** of the product's X3 sync: the harness is ready (`scripts/run-spikes.sh x3`, about 5 minutes). It starts three pages before a chapter end and records location, snapping, saving and announcements automatically; the person answers three questions. Results go to `docs/spikes/raw/x3-voiceover.json`.

## Fixes found by the L16 checks

- The open-time budget checks stopped at the previous reader's location; they now wait for the new reader's first laid-out page (budget-open p95 is still 87–102 ms).
- Reopening a book right after leaving it could read the position before the closing save landed; the reader now waits for pending writes (N5).
- Back chips outlived their book and could call a closed reader; the reader withdraws them when it closes.
