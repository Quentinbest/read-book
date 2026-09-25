# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

## Phase 3

1. **Phase 3 visual baselines (Screens 04, 16, 17).** Review `docs/visual/phase3-review.html`: the mocks against the app, with expected and later-phase differences marked. Approving records the app captures as the baselines (plan §6.1; Phase 3 Done-when).
   *Recommendation:* approve.

2. **G10 cheat sheet design (provisional).** The `?` sheet is built and tested (it lists every working shortcut). Its look is my proposal inside the approved system: see the G10 section of the review page. The plan wanted G10 before Phase 3; it did not exist.
   *Recommendation:* approve as the G10 design.

3. **⋯ menu contents (provisional).** No design says what the reader's ⋯ menu holds. Built: every command, grouped, with shortcuts; unavailable ones at 40%. See the review page.
   *Recommendation:* approve; extension items join it in Phase 7 (G7).

4. **Screens 03 and 14 baselines: two new top-bar buttons.** Contents and ⋯ now show in the top bar (46 pixels, within tolerance). The other buttons Screen 03 draws stay hidden until their phases.
   *Recommendation:* approve the new captures as the baselines.

5. **Footnote peek shows notes as plain structure, not in the book's own styles.** For security (§7: book markup is untrusted), the note is copied as paragraphs and emphasis only. Screen 17 says “rendered with the book's own styles”. The alternative is a sandboxed frame with the book's CSS: more work, the same look for most books.
   *Recommendation:* keep the safe copy.

6. **Derived colours for the new surfaces, and one contrast fix (provisional, like C7).** Screen 04 and 17 draw only Paper. Sepia and Night values for the Navigator panel, segment track, raised segment, popover surface, and tooltip are derived (G5), and all pass the contrast tests. One change to Paper itself: the selected segment's ring is drawn `#8F877B`, which is 2.7:1 on its track, under V7's 3:1; it uses secondary ink (4.4:1), as the Phase 1 segmented control already did. Values are in `src/lib/theme/tokens.ts`, marked PROVISIONAL.
   *Recommendation:* approve.

7. **The damaged-book card (E3).** Contents marks damaged chapters (the Phase 3 Done-when item). The card “Part of this book couldn't be opened · Read anyway / Show file / Remove” (Screen 12) is not built; Remove depends on G4 (what Remove deletes), a Phase 6 decision.
   *Recommendation:* build the card with Read anyway and Show file in Phase 6, together with Remove.

## Carried over (need a person, not a decision)

8. **VoiceOver re-check of the reader (X3).** About 5 minutes: `scripts/run-spikes.sh x3`, turn VoiceOver on, Control + Option + A, follow the panel.

9. **Oldest supported macOS reference machine (macOS 13 with Safari 16.4+).** Needed before the §6.4 budgets are signed off for release (`docs/spikes/reference-machines.md`).
