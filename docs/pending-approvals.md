# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

Approved so far; the record is in `docs/decisions.md`:
- items 1–8 and 11–20, on 2026-09-25;
- items 9, 10 and 21–30, on 2026-09-26. Item 28 as the owner settled it: builds need only meet what GitHub requires until there is a Developer ID. Item 29 by making `read-book` public.
- items 31, 33–41, on 2026-10-01 (release 1.1). Item 32 was built as recommended the same day.
- item 42, on 2026-10-02, left to Claude by the owner: (a) is built; (b) and (c) are not.

Waiting: items 45–50 (next-steps plan, 2026-10-02). Items 43 and 44 are set aside by the owner (2026-10-02: no Apple membership for now). The items below are kept for their details.

## Review fixes (2026-09-26)

31. **Quitting with notes that could not be saved (N5, E5).** Quit and Restart now wait for the reader's writes. If a save has failed (a full disk), Linen stays open and shows “Some notes aren’t saved, so Linen stayed open · Quit Anyway”, above the existing “Couldn’t save notes to disk · Retry”. Retry, then quitting again, quits normally; Quit Anyway leaves without the unsaved changes. The wording is marked PROVISIONAL in `src/lib/strings/en.ts`.
    *Recommendation:* approve the behaviour and the wording.

32. **An update this account cannot install (D6).** The updater replaces `Linen.app`. When the app or its folder is not writable (a standard account, an app an administrator put in /Applications, or an app run from the disk image), the updater would ask for an administrator's password in the middle of reading. It is no longer tried: the update is only logged, and nothing is shown.
    *Recommendation:* show the quiet line “Linen x.y.z is available · Download” once per version, opening the release page. **Built as recommended (2026-10-01):** the core emits `update-available` once per version (setting `updateAnnounced`); Download opens `…/releases/tag/v<version>`, an address the core builds (`updater::open_release_page`). Check `D6-update-available`.

## Release 1.1 (2026-10-01)

The owner asked for the recommended next steps except TTS, sync and Developer ID signing. **Approved 2026-10-01 as recommended** (record in `docs/decisions.md`); kept here for their details. Status and evidence are in `docs/release-1.1-status.md`.

33. **Font and page width (plan §1.3 “1.1”; P§9).** Two rows in the Aa popover, both “All books”, also in Settings › Reading.
    - **Font:** Book (the publisher's fonts, as today), Literata, Sans (the system sans), and OpenDyslexic as the dyslexia-friendly face. OpenDyslexic is bundled under the SIL OFL (four woff2 faces, about 440 KB) and loaded only when chosen. A chosen font replaces the book's fonts on text, never on code.
    - **Page width:** Narrow 56 ch, Normal 66 ch (the default), Wide 74 ch: the ends of L1's clamp, so L2 margins and L5's long-line bonus still apply.
    *Recommendation:* approve. The alternative to OpenDyslexic is Atkinson Hyperlegible, which suits low vision rather than dyslexia; the design names a dyslexia-friendly face.

34. **Publisher styles and Simplify styles (C5).** Settings › Reading › Publisher styles: **Full** keeps the publisher's line heights and alignment; **Balanced** (the default) is the MVP's behaviour; **Off** turns the book's own stylesheets and style attributes off in place, without reloading. Colours stay the theme's in every mode (L14). One book can be simplified with **Simplify Styles for This Book** (View menu, ⌘K, the ⋯ menu), which is Off for that book only, with an Undo message; running it again brings the styles back.
    *Recommendation:* approve.

35. **Dictionary peek (plan §1.3 “dictionary peek providers”).** **Look Up** in the selection bar, in the context menu (“Look Up “word””) and as ⌃⌘D (the macOS chord) shows the definition from the dictionaries installed on this Mac (Dictionary Services), in a peek styled like the footnote peek. Nothing leaves the Mac. **Open in Dictionary** hands the word to Dictionary.app; **Search the book** searches for it. Phrases up to 80 characters. Translation and extension-provided peek providers are not built.
    *Recommendation:* approve; extension peek providers wait for a Host API addition.

36. **Library list view (P§10 “list view in 1.1”).** A Covers / List switch in the library header, remembered. A row has a small cover, title and author, progress, and when it was last opened; the item menu and the context menu stay. Below 720 px the “opened” column hides.
    *Recommendation:* approve.

37. **Reading-session events (plan §1.3 “reading-statistics session events”).** A new permission, `reading.sessions` (consent at install: “Know when and how long you read”), and activation `onReadingSessions`. `linen.reading.on('sessionEnded', …)` delivers start and end times, active seconds, start and end fraction, and pages turned. The book's title and identifier come only with `book.metadata`. A session ends after 5 minutes without movement (at the last movement), when the book is put away, or at quit; sessions under 10 seconds without a page turn are dropped. Linen keeps no statistics itself. API docs: `docs/extensions/`.
    *Recommendation:* approve the permission, the payload and the 5-minute and 10-second limits.

38. **Shortcut remapping (C4).** Settings › Shortcuts lists every command (extension commands included) with Change, Remove and Reset, and Reset All Shortcuts. Rules:
    - A shortcut needs ⌘, ⌃ or ⌥ (function keys alone are allowed).
    - ⌘Q ⌘W ⌘H ⌥⌘H ⌘M ⌘, ⌘Tab ⌘Space and ⌘X ⌘C ⌘V ⌘A can't be taken; Esc and Tab stay Linen's.
    - An extension command can't take a core command's shortcut (P§12).
    - A core command can take another command's shortcut; that command loses it, and Settings says so.
    - The single-key shortcuts are not remapped; their switch stays.
    *Recommendation:* approve. **Not verified:** that the native menu bar never fires while a chord is being recorded in the separate Settings window (the harness records it in the main window).

39. **Search options (B6).** B6 (2026-09-24) reserved no space for these; they are now under the search field: **Whole words**, and **.\*** for a regular expression. A regular expression ignores case but matches the text as written (diacritics count), and an invalid one shows “This regular expression isn’t valid.” Options last for the open book's search.
    *Recommendation:* approve. A pathological pattern can keep the search worker busy (JavaScript can't interrupt a regular expression); only search waits, and closing the book ends it.

40. **Visual baselines for 1.1.** These captures change on purpose: `01-library` and `12-damaged-book` (the view switch), `03-more-menu` (Simplify Styles), `05-navigator-search` (the options), `06-selection-bar`, `14-night-selection` and `12-extension-failure` (Look Up), `09-reading-settings` (two more rows), `g10-cheat-sheet` (⌃⌘D). New captures are in `docs/visual/app/`, taken on Desktop 2; see `docs/release-1.1-status.md`. Settings › Reading and Settings › Shortcuts have no capture yet.
    *Recommendation:* review the captures and approve them as the new baselines.

41. **Settings captures for 1.1.** `docs/visual/app/11-settings-reading.png` (Font, Page width, Publisher styles) and `11-settings-shortcuts.png` (the remapping list) are new captures, taken on Desktop 2; neither screen had a capture before.
    *Recommendation:* approve them as baselines.

## TypeSafe experiments (2026-10-01)

42. **Optional TypeSafe judgments, opt-in.** Four experiments in `docs/experiments/typesafe/` tried TypeSafe's `jev-1.13` model in place of heuristic code. The results: ⌘K understood natural phrasing ("bigger text", "dark mode", "my notes") 45/50, against 17/50 for fuzzy matching. Author sort got 28/30 hard names right (Le Guin, King Jr., Du Bois), against 12/30. A new book without a landmark opened at its first chapter in 104/123 books, against 0/123. Every call leaves the device, which goes against Screen 12 ("stay on this device") unless the reader opts in.
    *Recommendation:*
    - **(a) Now, no model:** read the publisher's `file-as` sort name (`opf:file-as`, and `<meta refines … property="file-as">`) at import, and sort authors by it.
    - **(b)** Add a setting, off by default, with the API key in the Keychain and all calls in the Rust core (extensions can't send the `Authorization` header). Then:
      - author sort for books without `file-as`, computed once at import and stored;
      - ⌘K: when fuzzy matching finds no plain substring, ask Jev for the command, after a pause in typing.
    - **(c) Not yet:** the opening position needs another experiment, with headings inside documents as candidates (Gutenberg books). The title fallback gains too little to justify it.

    **Decided 2026-10-02** (left to Claude; record in `docs/decisions.md`): (a) is built (schema 4, `books.author_sort`). (b) is not built: Linen has no server, so there is no safe way to ship an API key. (c) as recommended.

## Next-steps plan (2026-10-02)

From `docs/next-steps-plan.md` §4. Release downloads so far (`docs/release-stats.md`): one Apple silicon DMG per release, no Intel DMG.

43. **(O1) Apple Developer Program.** Blocks Phase 9 signing and the whole iPad phase. *Recommendation:* join as an individual (99 USD a year).
44. **(O2) Host API 1.1:** `lookup.registerProvider` and `metadata.registerProvider`, rendered by Linen from structured results (Phase 11). *Recommendation:* approve.
45. **(O3) iPad channel.** *Recommendation:* TestFlight first.
46. **(O4) Extensions on iPad** (App Store guideline 2.5.2). *Recommendation:* keep them on TestFlight; decide before any App Store submission.
47. **(O5) A test iPad.** *Recommendation:* any iPad Tauri's iOS support runs on.
48. **(O6) Sync approach.** *Recommendation:* an adapter API plus a built-in sync folder (iCloud Drive).
49. **(O7) Gate W**, after Phase 10. *Recommendation:* no-go unless someone asks; so far nobody has.
50. **(O8) iPad before sync.** *Recommendation:* yes.
