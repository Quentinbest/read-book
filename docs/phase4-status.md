# Phase 4 status — search (macOS)

- **Last updated:** 2026-09-25
- **Before starting:** B6 decided (no scan cap; 100 results shown per chapter, then “Show all N”).
- **Evidence:** the in-app suite (`LINEN_SPIKE=r`, checks `F-*`), unit tests (`src/lib/search/*.test.ts`), `cargo test` (schema v2 migration).

## Done-when (plan §5, Phase 4)

| Item | Status | Evidence |
|---|---|---|
| A golden-result test gives the expected hit count and positions for each corpus query set, including diacritic, CJK and quoted-phrase queries | Done | e2e F-golden-results: the product search gives exactly Spike F's counts (whale 1,688 in 120 chapters; Queequeg 252 in 42; harpoon 256 in 66; “Call me Ishmael” quoted 1; unquoted 1; xyzzy 0; *etait* 15 in 3 in *Sous le vent*; 智に働けば 1; 山 103 in 13 in *草枕*). Positions: F5-results-land checks every “water” result's range |
| Every result lands on its match in the page, and marks meet the F5 contrast values | Done | e2e F5-results-land: every result (↵ through all of them) is on the page shown in the right chapter, its range reads “water”, the active mark is the stronger tint with a 2 px accent outline, the others a soft tint with a 1 px outline, in the theme's approved colours (contrast in `tokens.test.ts`); marks go when Search closes |
| The Esc-return rule (F7) passes an end-to-end test | Done | e2e F7-esc-return: Esc after browsing returns to the original page; after choosing a result you stay, and Back returns |
| Search timing holds the Spike F budget in the app | Done | e2e F3-F8-streaming-persisted: from the saved index, “the” (19,959 results, the worst query) is searched and painted in about 160 ms (budget 300 ms) |
| The Screen 05 visual baseline is approved | **Waiting for the owner** | `docs/visual/phase4-review.html`; `docs/pending-approvals.md` item 8 |

## Work items

| Item | Status |
|---|---|
| Extraction and normalisation in a worker; offset maps back to DOM ranges; persisted index (F8) | Done. DOMParser is not available in workers, so chapters are extracted on the main thread (current chapter first, yielding between chapters) and normalised and scanned in a worker (`src/lib/search/session.ts`, `search.worker.ts`). Extracted text is saved per book in `search_text` (schema v2), stamped with the file's content hash |
| Incremental search (F1–F4) | Done: ⌘F, the Navigator's Search tab, or ⌘K; a selection pre-fills; 150 ms debounce; 2 characters (1 for CJK); results stream in F3 order with “Searching N of M chapters” and “so far”; grouped by chapter with counts, the current chapter (or the first with results) open; 100 per chapter, then “Show all N” (B6) |
| Marks (F5), keys (F6), Esc-return (F7) | Done; the location line reads “Result N of M · chapter” while Search is open |

## Found and fixed

- **The importer crashed the app on a Japanese book.** The XML entity check cut the text at 64 KB inside a three-byte character and panicked; the panic could not unwind through the WebView's native callback, so the whole app aborted. Fixed (cut at a character boundary), with a unit test; parsing is now wrapped so any parser panic rejects that one file; a new test imports all 65 corpus files.
- Streaming results redrew the page marks for every chapter; marks now redraw at most once a frame and only when the page's marks change.
- Results were deep reactive state, making thousands of matches into proxies; they are now raw state (replaced whole). Search of “the” went from several seconds of work after the scan to one frame.
- When the current chapter has no results, the first chapter with results opens, so there is always something to choose.
