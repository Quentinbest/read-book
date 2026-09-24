# Phase 2 status — reader core (macOS)

- **Last updated:** 2026-09-24
- **Started without the G8 designs,** on the owner's instruction to go ahead. Parts that need those designs are marked.
- **Evidence:** the in-app end-to-end suite (`src/spikes/e2e.ts`, run with `LINEN_SPIKE=r` against a throwaway data folder), the memory run (`LINEN_SPIKE=m`), and the unit tests (`pnpm test`, `cargo test`).
- **Latest suite result:** 27 of 28 checks pass (`docs/spikes/raw/e2e-reader.json`).

## Done-when (plan §5, Phase 2)

| Item | Status | Evidence |
|---|---|---|
| Unit tests assert every I-rule threshold and L1–L10 | Done | `src/reader/layout.test.ts`, `src/lib/input/*.test.ts`, `src/reader/pace.test.ts` |
| Scripted end-to-end page turns by key, wheel, click and touch zone, LTR and RTL; clicks in the column and the activating click never turn a page | Done except touch zones | e2e: I8 keys (4 checks), I11 margins, activating click, column click, I1 wheel, I2/I5 trackpad, I15 RTL keys and margin. Touch zones: see below. |
| Quitting mid-chapter and reopening restores the same CFI; a new book opens at bodymatter | Done | e2e: N5-quit-save, N5-restore, progress-saved, N3-bodymatter |
| Resize, font change and text spacing keep the reading position | Resize done; font change covered by Spike D; the Aa popover arrives in Phase 6 | e2e: L10-resize; Spike D: 200/200 CFIs through font 16 → 24 → 16 px |
| A fixed-layout book scales, spreads and zooms | Opens and turns pages; zoom and the layout design wait for G8 | e2e: E2-fixed-layout |
| A 1 MB+ chapter opens within the open-book budget and shows “≈” until pagination completes | **Open**: 2.1–2.2 s against 500 ms | e2e: L16-long-chapter. Needs virtual sections (L16) and the G8 loading-state design |
| §6.4 budgets that apply from Phase 2 | Open book, reflow, page turn and memory pass; cold start to the last book waits for Phase 6's library (“Resume reading”) | e2e: budget-open (p95 < 500 ms), budget-reflow (< 150 ms), I6-turn-budget (p95 5 ms within and 1 ms across chapters, 180/180 turns exact); memory run: 290–371 MB |
| Hostile corpus EPUBs still fail as in Spike E | Done | e2e: E-hostile-in-product (no script, IPC or network; the fixed overlay is neutralised) |
| Visual baselines for Screens 02, 03, 10 and 14 approved | **Needs the owner**: screenshots to be prepared for sign-off | — |

## Work items

| Item | Status |
|---|---|
| ReaderEngine adapter (open, render, paginate, CFI, reflow, §7.1 frames) | Done: `src/reader/engine.ts`. Content hooks (per-document CSP, L13/L14 sanitiser), links routed by the app (N10), one queued turn (I6), neighbour pre-layout (D-D1), on-demand loading (L17) |
| Canvas L1–L17 | L1–L15 and L17 done (`layout.ts`, `styles.ts`, `fonts.ts`, `loader.ts`); **L16 open** |
| Fixed layout (E2) | Basic open and turn; zoom (⌘+, pinch, I17) and the layout design wait for G8 |
| Immersive state and location line (L9, B2, S12) | Done |
| Chrome (S9–S11, S15) | Done: edge reveal with dwell, Tab, auto-hide, macOS full-screen and Dock edges (T8). Controls whose features arrive later are hidden. The 220 ms hide animation is still to do |
| Input router (I1–I15) | Done for keys, wheel and trackpad (native bridge), margins, RTL, turn queue and the rapid-turn chip. **Touch (I12–I14) not applicable on macOS** (no touch screens); deferred with the tablet release |
| Scroll mode (B8) and the two-page spread (L8, B12) | Layout supports the spread; both wait for the G8 designs |
| Progress persistence (N4, N5), Back (N1, N2) | Done |
| Screen-reader sync (X3), page-turn announcements | Done: external scrolls are followed while VoiceOver runs; announcements have a setting (`pageTurnAnnouncements`). Not yet re-verified with VoiceOver in the product |
| Night chrome (V2), motion (V8, V9) | Night chrome and chrome reveal done; the hide animation is still to do |

## Open items and what they need

1. **L16 long chapters:** virtual sections so the first page shows before the whole chapter is laid out, with saved positions staying valid. Engineering plus the G8 loading-state design.
2. **G8 designs:** Scroll mode, two-page spread, fixed-layout zoom and pan, RTL and vertical writing, image view, loading state and “≈” locations.
3. **Owner sign-offs:** visual baselines for Screens 02, 03, 10 and 14.
4. **VoiceOver re-check** of the product's X3 sync (about 5 minutes, like Spike C).
