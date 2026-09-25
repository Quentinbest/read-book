# Phase 3 status — navigation (macOS)

- **Last updated:** 2026-09-25
- **Started with one design input missing:** G10 (the cheat sheet) did not exist. It is built as a provisional design and waits for approval (`docs/pending-approvals.md`, item 2). The image view (G8) and B13 were in place.
- **Evidence:** the in-app suite (`LINEN_SPIKE=r`): 59 of 59 checks pass (`docs/spikes/raw/e2e-reader.json`); unit tests 202 of 202 (`pnpm test`); `cargo test`.

## Done-when (plan §5, Phase 3)

| Item | Status | Evidence |
|---|---|---|
| Contents is correct for the corpus (nav, NCX-only and no-TOC books), and damaged chapters show the marker | Done | e2e: N6-contents-nav (Standard Ebooks, nav), N6-contents-ncx (Gutenberg EPUB 2, NCX only), N6-contents-headings (no TOC: “Generated from headings”), N6-contents-damaged (2 of 3 chapters missing, both marked). Unit: `contents.test.ts` (fallback chain, depth, “You are here”) |
| The footnote peek meets every N9 rule in an end-to-end test, including focus return and placement near the page foot | Done | e2e: N9-footnote-peek (below, or above near the page foot, never over the marker's line, for every marker on the page; never navigates; long note scrolls within 50%; marker ring; Tab into the peek; Esc returns focus; referenced footnote asides out of the flow), N9-peek-actions (Copy to the pasteboard, which is restored afterwards; Open note in place with Back), N9-endnote-peek (endnote in another chapter). Fixture `notes-and-images.epub` |
| Go to lands on the stated location for percent, chapter and page-list targets, and Back returns | Done | e2e: N8-goto-percent (31%, preview names the chapter, Back), N8-goto-chapter, N8-goto-page (Children's Literature page list: page 175 is on the page shown; unknown pages are refused; Print page only when a page list exists), N7-scrubber |
| Every registry command is reachable from ⌘K, the macOS menu bar and the ⋯ menu, and shows its shortcut; the cheat sheet lists every shortcut | Done (⋯ menu contents and the cheat sheet design provisional) | e2e: K-commands-reachable (every available command in all three, with the same shortcut label), K9-cheat-sheet (every working chord, alternative key and single key listed; `?` from ⌘K), K9-palette, K1-chapter-keys. Unit: `cheatsheet.test.ts`, `fuzzy.test.ts` |
| The S2 Esc order holds with the Navigator docked plus a floating layer open | Done | e2e: S2-esc-order (Go to closes first, then the Navigator), S2-esc-navigator; the Modal now owns Esc, so closing ⌘K never also closes a layer behind it |
| Visual baselines for Screens 04, 16 and 17 are approved | **Waiting for the owner** | `docs/visual/phase3-review.html`; `docs/pending-approvals.md` item 1 |

## Work items

| Item | Status |
|---|---|
| Navigator shell (S2, S3, S13, V8) | Done: docks from 1100 px, floats below; slides in 220 ms while the column moves by transform, then the page reflows; header with the window buttons, Library, cover, title, author and progress; docked state remembered per book. Search and Notes tabs join in Phases 4 and 5 |
| Contents (N6, E1, E3) | Done: nav → NCX → headings → spine; “You are here” with focus (K3, C2); damage markers |
| Chapter keys (K1), scrubber and tooltip (N7) | Done; the track is also a keyboard slider |
| Go to (N8) | Done: from the bottom-bar label, the Navigator's % or ⌘J |
| Footnote peek (N9), links (N10), image view (N11) | Done. External links open in the system browser through a core command limited to http(s) and mailto, with the URL shown on hover. The note is shown as safe structure (pending item 5) |
| Command palette (K9, M3, S8) | Done: Recently closed (each message once; a book's messages leave with it), Reading, chapters by name, fuzzy match, ↑ ↓ ↵, `?` |
| Cheat sheet (G10) and ⋯ menu | Built, provisional (pending items 2 and 3) |

## Found and fixed along the way

- foliate-js counted hidden footnote asides as “on the page”, which moved page turns; its visible-range search now skips them (patch), as it does chunk-hidden blocks.
- Copy used WebKit's clipboard API, which needs a user gesture; it is now a native pasteboard command.
- A synthetic Esc never cancels a `<dialog>`, and a real one also reached the reader behind it; the Modal now handles Esc itself and stops it.
- The reader's “Resumed in …” message outlived its book (its action pointed at a closed reader) and piled up in Recently closed; it is withdrawn when the book closes, and Recently closed lists identical messages once.
- The first e2e run of the external-link change opened the hostile test book's canary link in the system browser. Test builds now record external links instead of opening them.
