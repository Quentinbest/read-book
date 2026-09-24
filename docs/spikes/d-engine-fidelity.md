# Spike D: engine fidelity (foliate-js in WKWebView)

**Verdict: 4 of 5 criteria pass, most by a wide margin. The page-turn budget fails for turns that cross into a new chapter** (p95 27 ms against 16 ms). Turns within a chapter take p95 3 ms. The fix belongs in the adapter (lay out the neighbouring chapters in advance, as I6 and L17 already require) rather than in a different engine; this needs owner sign-off (D-D1 in `decision.md`).

- **Date:** 2026-09-24
- **Machine:** see `reference-machines.md`
- **Build:** release, `--features spikes`, background throttling off
- **foliate-js:** 78914ae
- **Raw results:** `raw/d-fidelity.json` (open times, CFI, highlights), `raw/d-page-turns.json` (page turns, re-measured)

## Results

| Criterion | Budget | Result | Verdict |
|---|---|---|---|
| Moby-Dick opens to the first page | < 500 ms | p50 38 ms, p95 78 ms (10 runs), from reading the file over IPC to the first page painted | pass |
| A 100 MB book opens | < 1.5 s | p50 241 ms, p95 292 ms (5 runs, `large-100mb.epub`, 98 MB). The whole-file IPC read alone is p50 140 ms. | pass |
| 200 random CFIs round-trip after font 16 → 24 → 16 px and width 1280 → 800 → 1280 px | same text | 200/200 resolve to the same text; 200/200 targets stay on the visible page after every one of the 4 steps | pass |
| 100 highlights stay on their exact text after the same reflows | exact | 0 mismatches: after each step, every highlight's drawn rects equal the live `getClientRects()` of its range, and its text is unchanged | pass |
| Page turn | < 16 ms p95 | Within a chapter: p50 2 ms, p95 3 ms (n = 169). **Crossing a chapter: p50 17 ms, p95 27 ms, max 31 ms (n = 31).** All turns: p95 18 ms. | **fail** |

Page-turn time runs from calling `next()` to the last `relocate` event of the turn, when the new page is in place. The next painted frame follows within one vsync.

## Why chapter-crossing turns are slow

foliate-js keeps one section in its single iframe. Turning past the last page loads the next section into a new iframe, lays it out and scrolls, all in the turn. Turns inside a section are just a scroll (≤ 4 ms).

Two more foliate-js behaviours matter for Phase 2:

- After every turn, `next()` holds a 100 ms lock and ignores turns in the meantime. I6 needs “at most one pending turn” to be queued, not dropped.
- The first measurement attempt stopped the timer at an early `relocate` from the old section (p95 1 ms, which was not credible). The re-measurement waits for the last event of the turn.

## Recommendation

Keep foliate-js. In the ReaderEngine adapter:

1. Lay out the next and previous sections ahead of time, in a second paginator or a pre-rendered off-screen view, so a chapter-crossing turn is a swap rather than a load. L17 already calls for the current spine item plus its neighbours to be loaded.
2. Queue one pending turn instead of relying on the 100 ms lock (I6).
3. Re-run this spike's page-turn check against the adapter in Phase 2 (a Done-when item there).

Evaluate Readium ts-toolkit (the plan's fallback for Spike D) only if the adapter cannot bring chapter-crossing turns under 16 ms.

## Budget notes

- Opening the 98 MB book spends about half its time on the whole-file IPC read. L17 (read from the zip on demand) will remove most of that once book content is served by the Rust core.
- Memory for a large book (§6.4, < 400 MB) was not measured here; it is a Phase 2 budget.

## Follow-up in Phase 2 — D-D1 resolved (2026-09-24)

The ReaderEngine (`src/reader/engine.ts`) now keeps the previous and next sections laid out in two hidden foliate views that share the book. A turn across a chapter boundary swaps which view is visible; the view that falls out of range is re-parked off the turn.

Measured in the product reader by the in-app end-to-end suite (`src/spikes/e2e.ts`, check I6-turn-budget, release build, reference machine):

| | Before (Spike D, spike harness) | Before (product engine, one view) | After (three views) |
|---|---|---|---|
| Within a chapter, p95 | 3 ms | 4 ms | 5 ms |
| Across chapters, p95 | 27 ms | 57 ms | **1 ms** |

All 180 turns (120 forward, 60 back, 27 of them crossing chapters) landed exactly on the next or previous page, or on the first or last page of the neighbouring section. The Readium fallback is not needed.

Lesson from the implementation: foliate-js ignores `goTo` while a view holds its 100 ms post-turn lock and still resolves the promise, so the engine records the section a view actually shows, not the one it asked for.
