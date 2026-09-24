# Spike C: accessibility — needs a person at the machine

**Status (2026-09-24): pass on macOS, with adapter work required for X3** (run 2). VoiceOver reads continuously and the page follows it; foliate does not report the move, so the adapter must detect it, snap to the page and update the location. Pass criterion (plan §5): VoiceOver reads continuously inside the paginated iframe across at least 3 page boundaries, and we record whether its reading position is observable so the visible page can follow it (X3, T2). NVDA is deferred with Windows.

## Why it is not automated

VoiceOver's reading position is not exposed to web content, and judging continuous reading needs a person listening. About 10 minutes with VoiceOver (⌘F5) on the harness page is needed. The fallback, if continuous reading fails, is in the plan: Pages mode offers “Read from here”, which switches to Scroll for the session.

## How to run it (about 10 minutes)

```sh
scripts/run-spikes.sh c
```

Follow the panel: turn on VoiceOver (⌘F5), start “read all” (VO + A) in the book text, and let it read across 3 page boundaries. Then answer two questions. The harness also records every `relocate` event while VoiceOver reads, which shows whether its position is observable (T2). Results go to `raw/c-accessibility.json`.

## Run 1 — 2026-09-24

VoiceOver was started (⌘F5 did not work on this keyboard; the owner used another route) and read the book with VO + A. Raw results are in `raw/c-accessibility.json`.

| Criterion | Result | Verdict |
|---|---|---|
| VoiceOver reads continuously across 3 page boundaries | Observer: yes | **pass** |
| Its position is observable, so the visible page can follow (X3, T2) | The harness said pass, but that verdict was **wrong**. All 3 `relocate` events came in the first 390 ms, all at the same CFI: the initial page load, before VoiceOver started. **No relocations occurred while VoiceOver read**, although the observer saw the page follow. | **not established** |

Interpretation: VoiceOver most likely scrolls the text into view through WebKit's own scrolling, which foliate-js does not track. The screen moves, but foliate's idea of the current page (and so the location line, the saved position and the next page turn) stays behind. If run 2 confirms this, X3 needs the adapter to watch the frame's scroll position and re-snap to whole pages around VoiceOver's position, or the plan's fallback: “Read from here” switching to Scroll mode.

The harness now counts relocations only after the observer presses “Reading started”, and samples every 500 ms the book frame's position on screen, the document's scroll and foliate's page number, so the disagreement becomes measurable.

## Run 2 — 2026-09-24 (instrumented)

The observer started VoiceOver's “read all” and marked “Reading started”; the harness then sampled every 500 ms (380 samples, about 3 minutes).

| Criterion | Result | Verdict |
|---|---|---|
| Continuous reading across 3 page boundaries | Observer: yes | **pass** |
| Position observable, so the visible page can follow | While VoiceOver read, the book frame moved left in page-sized steps (188 → −447 → −1,084 → −1,732 px on screen) and foliate's scroll-derived `page` went 1 → 2 → 3 → 4. The observer saw the page follow. **Foliate fired no `relocate` event** in that time, and `lastLocation` stayed on the first page. | **pass** (observable through the frame position and `renderer.page`, not through `relocate`) |
| NVDA | — | deferred (macOS-only scope) |

**What this means for Phase 2 (X3):**

1. WebKit scrolls foliate's paginator to keep VoiceOver's cursor in view, so the visible page does follow the screen reader. The plan's fallback (“Read from here” switching to Scroll mode) is not needed.
2. foliate does not notice these scrolls. The adapter must watch for them while a screen reader runs (T6 already detects VoiceOver) by sampling the frame position or `renderer.page` each animation frame. When the page changes, it recomputes the location, updates the location line, and saves progress (the 1 s debounce).
3. The steps were 635, 637 and 648 px, not exact page widths, so after each move the adapter must snap to the nearest whole page; otherwise a sliver of the neighbouring page can show at the edge.
4. Page-turn announcements (X3) must stay off while VoiceOver drives the page, or it would announce its own reading.

Raw results: `raw/c-accessibility.json`.
