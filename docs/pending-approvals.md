# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

Approved so far; the record is in `docs/decisions.md`:
- items 1–8 and 11–20, on 2026-09-25;
- items 9, 10 and 21–30, on 2026-09-26. Item 28 as the owner settled it: builds need only meet what GitHub requires until there is a Developer ID. Item 29 by making `read-book` public.
- items 31, 33–41, on 2026-10-01 (release 1.1). Item 32 was built as recommended the same day.

Nothing is waiting for the owner now. Items 53 and 54 were approved on 2026-10-06 (reader review). The items below are kept for their details. (Open pull requests #7–#9 add items 42–52; numbers may need adjusting when they merge.)

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

## Reader review (2026-10-06)

53. **A minimum window size (owner's item 3).** The minimum belongs to the main window, which holds the library and the reader. Below 1100 px the reading area is the whole window, because the Navigator floats there (L8). Today the window has no minimum. The Settings window already has one: 720 × 480. Measured (`docs/reader-review-2026-10-06.md`, captures in `docs/visual/survey/`):
    - **Reader:** clear down to 640 × 480. From 600 px the top bar's title runs over its buttons, because it does not truncate.
    - **Text measure:** L1's 56 ch at 19 px holds from 591 px.
    - **Library:** wide layout from 721 px; at 720 px and below (its X6 narrow form) “Library” touches the window buttons.
    - **Height:** L9 already treats 480 px as the line; at 480 px the page holds 10 lines at 19 px, and Aa and Go to fit.
    - **Apple Books:** library window 1001 × 530, measured; its book window was not measured (that needs a book opened in your Books library).

    Options; the height is 480 in each:
    - **A. 760 × 480.** L8's narrow breakpoint, the width Screen 15 is drawn at. Everything measured is clean, and nothing else needs to change.
    - **B. 721 × 480.** The smallest width at which the library keeps its wide layout.
    - **C. 640 × 480.** The smallest clean reader measured. It needs two fixes first: the top-bar title must truncate, and the library's narrow header must clear the window buttons.

    *Recommendation:* A. Once agreed: `minWidth`/`minHeight` on the `main` window in `src-tauri/tauri.conf.json` and `tauri.spikes.conf.json`, plus an in-app check that resizing below them is refused and that the bars still work at that size. The X6 narrow forms stay: zoom (200–400%) still produces narrow CSS widths.
    **Approved 2026-10-06: option A, built as recommended.** A real drag of the window's corner stops at 760 × 480. At that size the bars, Aa and Go to fit, and the bars' buttons work (`L8-minimum-size`). The check drags the corner because AppKit's minimum limits a person's resize, not a programmatic one.

54. **Hands-on acceptance of the reader fixes (owner's items 1, 2 and 4).** They were reproduced and verified with real posted input in the in-app suite (`N6-contents-jumps`, `F6-result-jumps`, `S2-navigator-wheel`, `S9-edge-reveal-modes`). They have not been tried by hand, with a real trackpad's momentum, or with your own books.
    - **Jumps in Scroll mode:** they now go blank briefly while the chapter loads (the target shows 20–110 ms after the click in the runs here), as Pages jumps already do, instead of drawing it at the wrong place first.
    - **Edge zones in Scroll mode:** they are the same 64 px as in Pages, at the top and the bottom.

    *Recommendation:* try the steps in `docs/reader-review-2026-10-06.md` (Contents and search in both modes; the wheel over Contents at its ends; the pointer at the top and bottom in Scroll) and accept, or say what still differs.
    **Approved 2026-10-06** (“Go ahead”). The owner's reply records no hands-on run of these steps.

## Scroll bars (2026-10-07)

55. **Overlay scroll bars when macOS asks for visible ones.** The recording you sent shows an overlay scroll bar: hidden at rest, shown while scrolling, brighter under the pointer, faded after a second. Linen's scroll bars are WebKit's own and already do this whenever macOS uses overlay scroll bars, Scroll mode included (`docs/scrollbar-survey-2026-10-07.md`, `scrollbar-survey`). On the Mac used here they are always visible because a USB mouse is attached and **Show scroll bars** is **Automatically**: macOS then asks every app for visible bars, and a plain AppKit app does the same. Whether the knob stays up while the pointer rests on it could not be checked with posted input.

    Options:
    - **A. Follow macOS (no change).** Overlay bars with a trackpad or **When scrolling**; visible bars with a mouse under **Automatically**, or with **Always**. To get the recording's behaviour on this Mac: System Settings › Appearance › Show scroll bars › When scrolling.
    - **B. Overlay bars in Linen whatever the setting.** A custom indicator drawn by the reader in place of WebKit's bars, with its own hover, drag and timing. It overrides the choice of mouse users and of people who set **Always** (an accessibility setting on other platforms), and adds code to the reader's scroll path.

    *Recommendation:* A. If you try **When scrolling**, say whether the bar under a resting pointer behaves as you expect (the one step not verified).
    **Approved 2026-10-07: option A.** Linen keeps WebKit's own scroll bars and follows the system setting; no product change. The owner's reply records no hands-on check of a resting pointer on the knob.
