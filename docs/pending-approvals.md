# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

## Phase 3

1. **Phase 3 visual baselines (Screens 04, 16, 17).** Review `docs/visual/phase3-review.html`: the mocks against the app, with expected and later-phase differences marked. Approving records the app captures as the baselines (plan §6.1; Phase 3 Done-when).
   *Recommendation:* approve.

2. **G10 cheat sheet design (provisional).** The `?` sheet is built and tested (it lists every working shortcut). Its look is my proposal inside the approved system: see the G10 section of the review page. The plan wanted G10 before Phase 3; it did not exist.
   *Recommendation:* approve as the G10 design.

3. **⋯ menu contents (provisional).** No design says what the reader's ⋯ menu holds. Built: every command, grouped, with shortcuts; unavailable ones at 40%. See the review page.
   *Recommendation:* approve; extension items join it in Phase 7 (G7).

4. **Screens 03 and 14 baselines: the top bar.** *Updated in Phase 5:* the top bar now has every button Screen 03 draws except Aa (Phase 6), in the mock's places. Contents is beside Library; Search, Notes and ⋯ are on the right. The new captures are on the Phase 5 review page (`docs/visual/phase5-review.html`).
   *Recommendation:* approve the Phase 5 captures of 03 and 14 as the baselines.

5. **Footnote peek shows notes as plain structure, not in the book's own styles.** For security (§7: book markup is untrusted), the note is copied as paragraphs and emphasis only. Screen 17 says “rendered with the book's own styles”. The alternative is a sandboxed frame with the book's CSS: more work, the same look for most books.
   *Recommendation:* keep the safe copy.

6. **Derived colours for the new surfaces, and one contrast fix (provisional, like C7).** Screen 04 and 17 draw only Paper. Sepia and Night values for the Navigator panel, segment track, raised segment, popover surface, and tooltip are derived (G5), and all pass the contrast tests. One change to Paper itself: the selected segment's ring is drawn `#8F877B`, which is 2.7:1 on its track, under V7's 3:1; it uses secondary ink (4.4:1), as the Phase 1 segmented control already did. Values are in `src/lib/theme/tokens.ts`, marked PROVISIONAL.
   *Recommendation:* approve.

7. **The damaged-book card (E3).** Contents marks damaged chapters (the Phase 3 Done-when item). The card “Part of this book couldn't be opened · Read anyway / Show file / Remove” (Screen 12) is not built; Remove depends on G4 (what Remove deletes), a Phase 6 decision.
   *Recommendation:* build the card with Read anyway and Show file in Phase 6, together with Remove.

## Phase 4

8. **Phase 4 visual baseline (Screen 05).** Review `docs/visual/phase4-review.html`.
   *Recommendation:* approve.

## Phase 5

11. **Phase 5 visual baselines (Screens 06, 07, 08, 14 with the selection bar, and 15).** Review `docs/visual/phase5-review.html`: each mock beside the app in the same state, with the differences marked.
    *Recommendation:* approve.

12. **G11 Re-attach flow (provisional design).** The plan wanted G11 before Phase 5; it did not exist, so this is my proposal inside the approved system:
    - Re-attach on a “Couldn't place” row closes a floating Navigator and shows a pill at the bottom: “Select the passage for “…” · Cancel”.
    - The next selection shows the bar with just “Attach here” and “Cancel”.
    - Attach moves the highlight (with its note and colour) to the new passage, and says “Highlight re-attached”.

    See the review page.
    *Recommendation:* approve as the G11 design.

13. **Small choices the mocks do not settle (provisional).**
    - **Selection bar on Sepia:** Sepia's tooltip colours (G5-derived, like item 6).
    - **Screen 14:** its bar shows text-only actions, while Screen 06's has icons. The app shows the icons in every theme.
    - **Clicked highlight (A5):** its bar shows Delete where a new selection's shows Search.
    - **Margin dot (Screen 07):** drawn in the highlight's underline colour. The mock's `#8FA86A` is not a token.
    - **Jump pulse (Screen 08):** a 3 px accent outline at 40%, as the mock draws, fading after 1.2 s.
    - **Note status:** “Saving…”, then “Saved”; after a failed save, “Not saved yet” (the write queue's Retry message explains).
    - **Caret browsing (F7, T3):** a steady 2 px caret in the ink colour.

    *Recommendation:* approve.

## Phase 6

The plan wanted D2 decided and the G2, G3, G4 and G12 designs before Phase 6. None existed. What is below is built as recommended and marked provisional, so any of it can change on review.

14. **D2 — backup, export and restore; uninstall.** *Recommendation:*
    - **Where the data lives:** everything is in one folder (the library: the database, the books and the covers).
    - **Backup:** Time Machine or any copy of that folder is a full backup. Preferences › Library shows the folder with “Show in Finder”.
    - **Export:** “Export all highlights and notes…” writes one W3C Web Annotation JSON file (A9) per book, into a folder you choose.
    - **Restore:** means putting the folder back. Importing an exported JSON back is a Phase 7 extension-API job, not built now.
    - **Uninstall:** dragging the app to the Bin leaves the folder, as Mac apps do. Preferences › Library says so, and where it is.

15. **G4 — library states.** *Recommendation, built:*
    - **Remove** is immediate, with “Removed “…” · Undo” (⌘Z and ⌘K › Recently closed, as for highlights), and never asks to confirm. The book, its copied file and its annotations are deleted at the next launch, so Undo works until you quit.
    - **Duplicate import** opens the existing book; a book removed this session comes back as it was.
    - **Re-importing an updated file** replaces it and re-anchors highlights (Phase 5).
    - **Search with no results** says “No books match “…”” with a Clear button.
    - **The item menu** (Book info, Show in Finder, Remove) opens on right-click, or from a ⋯ button that shows on hover and focus.
    - **The sort menu** is a native menu (Recent, Title, Author).

16. **G3 — book info sheet (E10).** *Recommendation, built:* a modal sheet with:
    - the cover, title and authors;
    - publisher, publication date, language and identifier;
    - the file size and when the book was added;
    - the description;
    - the EPUB accessibility metadata, in plain words where known (“Readable as text”, “Has alternative text for images”…).

    It is read-only (Q4). Close with Esc or Done.

17. **G2 — Preferences sections.** *Recommendation:* one window with a sidebar, as Screen 11 draws for Extensions:
    - **General:** theme; open the last book at launch.
    - **Reading:** text size, line spacing, page-turn crossfade, page-turn announcements.
    - **Library:** the folder, export, and what uninstalling leaves (D2).
    - **Extensions:** Screen 11; the shell only until Phase 7.
    - **Shortcuts:** the single-key shortcut switch, and the cheat sheet's list.
    - **About.**

18. **G12 — Aa hints (L18, E2).** *Built, provisional:* one line under Layout for books over 30% code or tables (“Much of this book is code or tables. Scroll may read better.”). For fixed-layout books, size and spacing are hidden and one line explains why (“This book has fixed pages, so its text size and spacing can't change. ⌘+ and ⌘− zoom the page.”).

## Carried over (need a person, not a decision)

9. **VoiceOver re-check of the reader (X3).** About 5 minutes: `scripts/run-spikes.sh x3`, turn VoiceOver on, Control + Option + A, follow the panel.

10. **Oldest supported macOS reference machine (macOS 13 with Safari 16.4+).** Needed before the §6.4 budgets are signed off for release (`docs/spikes/reference-machines.md`).
