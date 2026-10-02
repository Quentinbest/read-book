# Spikes A, D and E across engines (next-steps plan, Phase 10)

**Verdict:** no engine blocks a port.
- **Anchors** (CFIs, highlights) are identical across all three engines.
- **Content isolation** holds on WebView2 and WebKitGTK.
- **Pagination:** Linux (WebKitGTK) is at parity in practice; Windows (WebView2) lays out about 3% more pages, consistently. As written, the 2% page criterion fails on both, because it can't be met on short chapters (see below).

- **Date:** 2026-10-02 (Spike A); 2026-10-03 (D, DX, E; run 37062812749)
- **Raw results:** `docs/spikes/raw/*-linux.json`, `*-windows.json`, `dx-anchors-macos.json` (run 1 of each)
- **Workflow:** `.github/workflows/cross-engine.yml` (PR #7). Each engine runs twice, and both runs gave the same result.
- **Comparison:** `scripts/compare-spike-a.mjs`, against the macOS run from the same workflow (not the stored 2026-09-24 file).
- **Method:** Spike A as in `a-rendering.md`: 20 Moby-Dick chapters at 16, 19 and 24 px, 1280 × 800, one column.
  - **New:** the bundled Literata is now loaded into the book. Before this, Spike A named Literata but never loaded it, so each engine measured its own fallback serif. The `A-literata` check confirms the font on every engine.

| Engine | Version | Literata | Split lines | Identical page counts | Differences | Total pages vs macOS |
|---|---|---|---|---|---|---|
| macOS, WKWebView | GitHub `macos-latest` | yes | 0 | (baseline) | — | 663 |
| Linux, WebKitGTK 4.1 | Ubuntu 22.04, under `xvfb` | yes | 0 | 53 / 60 | 7, each ±1 page, both directions | 664 (+0.15%) |
| Windows, WebView2 | Edge 153 | yes | 0 | 42 / 60 | 18, all +1 page | 681 (+2.7%) |

Windows by size: +1.7% at 16 px, +2.3% at 19 px, +3.7% at 24 px.

## What this means

- **Text that crosses a page edge** passes on every engine, with no split lines in 180 layouts. That was the criterion with the most risk to readers.
- **The 2% per-chapter limit is the wrong test.** These chapters run 5 to 29 pages, so a single page is 3–20%, and any rounding difference fails.
  - *Proposed instead:* each chapter within ±1 page, and the total within 2%. On that test Linux passes and Windows narrowly misses (2.7%).
- **Windows is systematic, not noise:** the differences only go one way and grow with font size. That points to how Chromium rounds line heights, or breaks lines, relative to WebKit, which L3 (pages snapped to whole lines) would amplify. One more page per chapter is barely noticeable while reading. It would matter for page numbers shared between devices, which is why plan item D compares anchors (CFIs), not page numbers.
- **Plan fallback (plan §5, Spike A row):** neither engine fails in a way that triggers Readium or Electron.

## Anchors across engines (Spike D and DX)

| Engine | DX: the same seeded range picked | DX: the same CFI for the same text | D: CFI round-trips through reflow | D: highlights through reflow |
|---|---|---|---|---|
| macOS | 200 / 200 (baseline) | 200 / 200 | 200 / 200 | 100, 0 mismatches |
| Linux | 200 / 200 | 200 / 200 | 200 / 200 | 100, 0 mismatches |
| Windows | 200 / 200 | 200 / 200 | 200 / 200 | 100, 0 mismatches |

- **DX** (new, `spikeDx`) picks 200 seeded ranges in Moby-Dick on each engine and records each one's CFI and text. `scripts/compare-spike-dx.mjs` compares them across engines.
  - **Result:** every engine parses the book to the same DOM and makes the same CFI for the same text.
  - So a highlight or position saved on one engine lands on the same words on another, which is what sync (Phase 13) relies on.
- **Not yet covered:** NFC and NFD forms of the same word, and soft hyphens (Moby-Dick has few of either). Add a fixture book for them before sync.
- **Timing criteria are machine-bound and failed on the runners, macOS included.** They are recorded, not judged:
  - Page turns crossing a chapter: p95 41 ms on Windows, 95 ms on Linux, 174 ms on the macOS runner.
  - The 100 MB book opened in 1.75 s on Linux. The budgets belong to reference machines (§6.4).

## Content isolation (Spike E)

| Probe (CSP, and CSP with the book's meta tag) | Linux | Windows |
|---|---|---|
| Book scripts run, reach the DOM, post messages or call IPC | blocked | blocked |
| Network requests to a local canary server | none | none |
| Local files, `tauri://localhost` framed | none | none |
| Chrome stays above the book | yes | yes |
| Hostile archives (traversal, absolute paths, symlinks, XML bombs) | handled | handled |

- **Custom schemes on WebView2.** The CSP-based isolation (D-E1) holds there too, even though WebView2 serves the app's custom URI schemes as `http://<scheme>.localhost` origins. That was the main open question for Windows.
- **`E-sandbox-no-scripts`** (sandboxed iframes still deliver events):
  - It fails on Linux, as on macOS: WebKitGTK shares WebKit bug 218086, which is why D-E1 chose the CSP over iframe sandboxing.
  - It passes on Windows (Chromium).
  - Either way, isolation rests on the CSP, which passes on all three.

## Not covered yet

- Content Moby-Dick doesn't exercise (next-steps plan, Phase 10 edge cases): vertical writing, RTL, CJK, hyphenation, fixed layout, spreads, 200% zoom.
- Spikes B (trackpad) and C (NVDA, Orca) need hardware and a person.

## Found on the way

- **The corpus has drifted.** `gutenberg-2701-moby-dick-epub2.epub` no longer matches its pinned hash (Project Gutenberg changed the file). `scripts/corpus/fetch.py` reports it, and the cross-engine workflow tolerates it, because Spike A doesn't use that file. Re-pin it or pin an archived copy.
- **Portability fixes:** the product build and the `spikes` harness now compile on Windows and Linux.
  - `open_external`, `open_software_update` and `copy_text` called AppKit with no fallback; they now return an error elsewhere.
  - The harness's memory, capture, scroll and pasteboard helpers are gated to macOS.
  - The harness now resolves its page against `http://tauri.localhost/` when WebView2 reports no usable base address at startup.
