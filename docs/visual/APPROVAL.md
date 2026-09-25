# Visual baseline approval

| Screens | Baselines | Engine | Status | Date | Approver |
|---|---|---|---|---|---|
| 02 Reader immersive, 03 Reader controls, 10 Reading themes (Paper, Sepia, Night), 14 Night controls (chrome only) | `baselines/webkit-19618/*.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1 (reference machine) | Approved | 2026-09-24 | Project owner |
| 03 and 14 top bar (updated), 04 Contents, 16 Command palette, 17 Go to and footnote peek, G10 cheat sheet, the ⋯ menu (phase 3 review); 05 Search (phase 4); 06, 07, 08, 14 selection, 15 (phase 5); 01 Library, 09 Aa, 11 Settings shell, 12 empty and damaged (phase 6) | `baselines/webkit-19618/*.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1 (reference machine) | Approved | 2026-09-25 | Project owner |

The 2026-09-25 approval covers `phase3-review.html` to `phase6-review.html`, with the differences they list. The 2026-09-24 approval covers the differences listed in `review.html` as expected (publisher styles, book metadata, B2 minutes) or later phase (hidden controls, scrubber details, highlights and the selection bar).

**How the baselines are used (plan §6.1)**

- `LINEN_SPIKE=v` recaptures the app into `docs/visual/app/`.
- `node tests/visual/compare.mjs` diffs the recapture against these baselines. It fails when more than 0.1% of pixels change, and writes diff images to `docs/visual/diff/`.
- An intended visual change needs a new review and approval here. A new WebKit version gets its own baseline folder.
