# Spike A: rendering (macOS part)

**Verdict: macOS criteria pass; cross-engine parity is deferred** (macOS-only scope). The WKWebView page counts are recorded as the baseline for the later comparison.

- **Date:** 2026-09-24
- **Machine:** see `reference-machines.md`
- **Raw results:** `raw/a-rendering-macos.json`
- **Harness:** `src/spikes/a-rendering.ts`

## Method

20 Moby-Dick chapters, spread evenly across the book, at 16, 19 and 24 px (60 layouts), 1280 × 800 px. The canvas is set up as L1–L3 in foliate-js terms:

- `max-inline-size` = 640 px × size / 19;
- one column below 1480 px (L8);
- line height 1.55.

For every layout the harness checks each text line box against the page edges, and computes the characters per line of long paragraphs.

## Results

| Criterion | Result | Verdict |
|---|---|---|
| No clipped or split lines at page boundaries | 0 line boxes crossing a page edge in 60 layouts | pass |
| Page count within 2% across WKWebView, WebView2 and WebKitGTK | WKWebView counts recorded | deferred |
| Screenshots differ only in font rasterisation | — | deferred |
| (Extra check) L1 measure, 56–74 characters per line | 58/60 layouts in range, averaging 69; 2 at 55, both in chapter 4 (front matter with short lines) | informational |

## Notes for Phase 2

- foliate-js lays out two columns (a spread) by default. `max-column-count` must be 1 below 1480 px (L8); the first harness run forgot this and measured 30–41 characters per line.
- The px approximation of L1 (“66 ch ≈ 640 px at 19 px”) gives about 69 characters per line of real text in Literata's fallback here. Phase 2 should derive the measure from the current font's metrics rather than a fixed px ratio, so the clamp holds for every book font.
- L3 (page height snapped to whole lines) is not something foliate-js does; Phase 2 implements it in the adapter.
