# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

Approved so far; the record is in `docs/decisions.md`:
- items 1–8 and 11–20, on 2026-09-25;
- items 9, 10 and 21–30, on 2026-09-26. Item 28 as the owner settled it: builds need only meet what GitHub requires until there is a Developer ID. Item 29 by making `read-book` public.
- item 33, on 2026-10-02, left to Claude by the owner: (a) is built; (b) and (c) are not.

## Review fixes (2026-09-26)

31. **Quitting with notes that could not be saved (N5, E5).** Quit and Restart now wait for the reader's writes. If a save has failed (a full disk), Linen stays open and shows “Some notes aren’t saved, so Linen stayed open · Quit Anyway”, above the existing “Couldn’t save notes to disk · Retry”. Retry, then quitting again, quits normally; Quit Anyway leaves without the unsaved changes. The wording is marked PROVISIONAL in `src/lib/strings/en.ts`.
    *Recommendation:* approve the behaviour and the wording.

32. **An update this account cannot install (D6).** The updater replaces `Linen.app`. When the app or its folder is not writable (a standard account, an app an administrator put in /Applications, or an app run from the disk image), the updater would ask for an administrator's password in the middle of reading. It is no longer tried: the update is only logged, and nothing is shown.
    *Recommendation:* show the quiet line “Linen x.y.z is available · Download” once per version, opening the release page. Not built yet; until then, such installs do not update themselves.

## TypeSafe experiments (2026-10-01)

33. **Optional TypeSafe judgments, opt-in.** Four experiments in `docs/experiments/typesafe/` tried TypeSafe's `jev-1.13` model in place of heuristic code. The results: ⌘K understood natural phrasing ("bigger text", "dark mode", "my notes") 45/50, against 17/50 for fuzzy matching. Author sort got 28/30 hard names right (Le Guin, King Jr., Du Bois), against 12/30. A new book without a landmark opened at its first chapter in 104/123 books, against 0/123. Every call leaves the device, which goes against Screen 12 ("stay on this device") unless the reader opts in.
    *Recommendation:*
    - **(a) Now, no model:** read the publisher's `file-as` sort name (`opf:file-as`, and `<meta refines … property="file-as">`) at import, and sort authors by it.
    - **(b)** Add a setting, off by default, with the API key in the Keychain and all calls in the Rust core (extensions can't send the `Authorization` header). Then:
      - author sort for books without `file-as`, computed once at import and stored;
      - ⌘K: when fuzzy matching finds no plain substring, ask Jev for the command, after a pause in typing.
    - **(c) Not yet:** the opening position needs another experiment, with headings inside documents as candidates (Gutenberg books). The title fallback gains too little to justify it.
