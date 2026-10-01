# Visual baseline approval

| Screens | Baselines | Engine | Status | Date | Approver |
|---|---|---|---|---|---|
| 02 Reader immersive, 03 Reader controls, 10 Reading themes (Paper, Sepia, Night), 14 Night controls (chrome only) | `baselines/webkit-19618/*.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1 (reference machine) | Approved | 2026-09-24 | Project owner |
| 03 and 14 top bar (updated), 04 Contents, 16 Command palette, 17 Go to and footnote peek, G10 cheat sheet, the ⋯ menu (phase 3 review); 05 Search (phase 4); 06, 07, 08, 14 selection, 15 (phase 5); 01 Library, 09 Aa, 11 Settings shell, 12 empty and damaged (phase 6) | `baselines/webkit-19618/*.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1 (reference machine) | Approved | 2026-09-25 | Project owner |
| 11 Settings › Extensions with extensions installed, 12 extension failure (phase 7) | `baselines/webkit-19618/11-settings-extensions.png`, `12-extension-failure.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1; captured on Desktop 2 (inactive window) | Approved | 2026-09-26 | Project owner |
| 08 Notes tab with the Phase 7 export footer (“Export as Markdown · via Markdown Export extension”) | `baselines/webkit-19618/08-navigator-notes.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1; captured on Desktop 2 (inactive window) | Approved | 2026-09-26 | Project owner |
| 02, 07, 08, 10 (Sepia, Night), 12 extension failure: no location line in immersive reading (owner decision, `docs/decisions.md` 2026-09-29). The only changed pixels are the removed line (y 758–768). | `baselines/webkit-19618/02-reader-immersive-paper.png`, `07-margin-note.png`, `08-navigator-notes.png`, `10-reader-sepia.png`, `10-reader-night.png`, `12-extension-failure.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1; captured on Desktop 2 (inactive window) | Approved | 2026-09-29 | Project owner |
| Release 1.1 (items 33–36, 38, 39): the library's Covers / List switch (01, 12 damaged), Simplify Styles in the ⋯ menu (03), the search options (05), Look Up in the selection bar (06, 14 Night selection, 12 extension failure), Font and Page width in Aa (09), Look Up Selection on the cheat sheet (G10) | `baselines/webkit-19618/01-library.png`, `03-more-menu.png`, `05-navigator-search.png`, `06-selection-bar.png`, `09-reading-settings.png`, `12-damaged-book.png`, `12-extension-failure.png`, `14-night-selection.png`, `g10-cheat-sheet.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1; captured on Desktop 2 (inactive window) | Approved | 2026-10-01 | Project owner (item 40) |
| 11 Settings › Reading (Font, Page width, Publisher styles) and Settings › Shortcuts (remapping), release 1.1 | `baselines/webkit-19618/11-settings-reading.png`, `11-settings-shortcuts.png` | WKWebView, WebKit 19618.3.11.11.5, macOS 14.6.1; captured on Desktop 2 (inactive window) | Approved | 2026-10-01 | Project owner (item 41) |

The 2026-09-25 approval covers `phase3-review.html` to `phase6-review.html`, with the differences they list. The 2026-09-24 approval covers the differences listed in `review.html` as expected (publisher styles, book metadata, B2 minutes) or later phase (hidden controls, scrubber details, highlights and the selection bar).

**How the baselines are used (plan §6.1)**

- `LINEN_SPIKE=v` recaptures the app into `docs/visual/app/`.
- `node tests/visual/compare.mjs` diffs the recapture against these baselines. It fails when more than 0.1% of pixels change, and writes diff images to `docs/visual/diff/`.
- An intended visual change needs a new review and approval here. A new WebKit version gets its own baseline folder.
- **Where captures are taken (from 2026-09-25).** On Desktop 2, by the owner's instruction: `scripts/e2e.sh v`, which sets `LINEN_SPACE=2`. The capture is the window server's own capture of the app's window (`CGSHWCaptureWindowList`), because `screencapture` refuses windows on a desktop that isn't showing. It gives the same pixels: most baselines compare at 0 changed pixels.
- **What differs on Desktop 2.** The window there is never the active window, so WebKit draws no focus rings and uses the inactive selection colour.
  - Screen 04's current Contents row loses its focus ring: a 0.12% difference.
  - Captures of states that show focus or a selection are reviewed with that in mind.
