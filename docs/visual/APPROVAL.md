# Visual baseline approval

| Screens | Baselines | Engine | Status | Date | Approver |
|---|---|---|---|---|---|
| 02 Reader immersive, 03 Reader controls, 10 Reading themes (Paper, Sepia, Night), 14 Night controls (chrome only) | `baselines/webkit-19618/*.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1 (reference machine) | Approved | 2026-09-24 | Project owner |

Approved with the differences listed in `review.html` as expected (publisher styles, book metadata, B2 minutes) or later phase (hidden controls, scrubber details, highlights and the selection bar).

**How the baselines are used (plan §6.1)**

- `LINEN_SPIKE=v` recaptures the app into `docs/visual/app/`.
- `node tests/visual/compare.mjs` diffs the recapture against these baselines. It fails when more than 0.1% of pixels change, and writes diff images to `docs/visual/diff/`.
- An intended visual change needs a new review and approval here. A new WebKit version gets its own baseline folder.
