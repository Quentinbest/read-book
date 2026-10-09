# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

Approved so far; the record is in `docs/decisions.md`:
- items 1–8 and 11–20, on 2026-09-25;
- items 9, 10 and 21–30, on 2026-09-26. Item 28 as the owner settled it: builds need only meet what GitHub requires until there is a Developer ID. Item 29 by making `read-book` public.
- items 31, 33–41, on 2026-10-01 (release 1.1). Item 32 was built as recommended the same day.
- item 56, on 2026-10-07, and item 57, on 2026-10-08 (UI localisation).
- items 58–76, on 2026-10-09 (Reading Lens). Item 60 with the MIT licence. Item 59 still needs the threshold values.

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

## UI localisation (2026-10-07)

56. **Extension diagnostics stay in English (L-10, Stage 2).** About 40 messages from the core's extension checks (`src-tauri/src/extensions/`: a manifest field that is missing or wrong, an unknown permission, a Host API version Linen lacks) and the theme-pack contrast report (`src/lib/extensions/themes.ts`: “secondary text is 3.9:1 (needs 4.5:1)”) appear inside ““Name” can’t be installed” and “Turned off: …”. They are written for extension authors, and translating them would put about 40 technical strings in every catalogue. The sentence around them is translated; the detail after it stays English. Errors that reach extension code (`ExtensionError` in `host.svelte.ts`) are developer-facing too.
    *Recommendation:* keep them English, as L-10 keeps extension-supplied text as authored.

57. **Chinese and Japanese typography in the UI (Stage 4, L-8).** In Chinese and Japanese, interface text that English sets in Literata (the damaged-book and old-Safari titles) uses the system font (PingFang or Hiragino); lines break between phrases (after 、。， and at spaces) rather than between any two characters; small-capital labels lose their letter-spacing. Book text keeps Literata and the book's own language. To see it: `LINEN_LOCALE=ja scripts/e2e.sh v` (or `zh-Hans`, `zh-Hant`), captures in `i18n-out/visual/<tag>/`; `docs/i18n-plan.md`, Stage 4.
    *Recommendation:* approve, so these screens stop being provisional; the native reviewers (L-6) may still adjust wording that breaks badly.

## Reading Lens (2026-10-09)

The open questions of `docs/reading-lens-plan.md` (§2, O1–O17), with what this branch (`reading-lens`) does meanwhile. Stage 2a is built against the plan's recommended defaults, marked (prov.) in its rules; the stages that wait are said so.

58. **What this run builds.** Stage 1 (the stuck-point diary) is yours and the readers'; it has no code. Stage 2b waits on a licence for Linen (item 60, O2); Stage 2d waits on Gate 0, which Stage 1 decides. So this branch builds Stage 2a only: the lookup peek, selection context and the Host API 1.1 additions it needs.
    *Recommendation:* approve the order; start Stage 1 when the thresholds (item 59) are set.
    **Approved 2026-10-09.**

59. **Gate thresholds (O1).** X and Y for G0, the severe-error bound for G2, the margins for G3. Not set; nothing in Stage 2a depends on them.
    *Recommendation:* set them before Stage 1 starts and before any data is seen, as the spec says.
    **Approved 2026-10-09.** The values are still to be given.

60. **A licence for Linen (O2).** The repository has none. Stage 2b needs one before taking an MDX parser dependency or porting BSD-3 code (DX12).
    *Recommendation:* choose one before Stage 2b; a permissive licence (MIT or Apache-2.0) keeps both DX12 sources open.
    **Approved 2026-10-09.** The owner chose MIT (`LICENSE`).

61. **The plan predates 1.1's Look Up.** The plan was drafted against 0.1.1. Since 0.2.0, Linen has a Look Up peek from the dictionaries on this Mac (item 35), bound to ⌃⌘D. Stage 2a therefore turns that peek into the lookup peek: “This Mac's dictionaries” is the first core provider, Linen's MDX dictionaries join it in Stage 2b, and extension lookups (Explain, the test provider) join through `lookups`. Open in Dictionary and Search the book stay in its footer.
    *Recommendation:* approve. O5 (“no shortcut; avoid ⌃⌘D”) is overtaken: ⌃⌘D was approved for Look Up on 2026-10-01 and stays.
    **Approved 2026-10-09.**

62. **No entry in any dictionary (O8, DX14, Stage 2b).** *Recommendation:* “No entry for ‘…’ in your dictionaries”, plus “Explain in context” when Explain is installed. Today's Mac peek already says the first part.
    **Approved 2026-10-09.**

63. **A clicked highlight offers Look Up and lookups (O9, LK14).** Built as recommended: the highlight's bar gets the same lookup items; Delete keeps its place. *Recommendation:* approve.
    **Approved 2026-10-09.**

64. **Placement when neither side has room (O10, LK13).** Built as recommended: the side with more room, that room minus 16 px, and the answer scrolls; the selected line is never covered. *Recommendation:* approve.
    **Approved 2026-10-09.**

65. **What closes the peek (O11, LK12).** Built as recommended: besides Esc, a click outside, a page turn and a new selection, also scrolling, a resize, a text-size or Aa change, opening the Navigator, any jump and leaving the book; each cancels the request. A theme change keeps it open. *Recommendation:* approve.
    **Approved 2026-10-09.**

66. **Books with no language (O12, LK9).** Built as recommended: `book.lang` is `""` for a missing language or `und`. *Recommendation:* approve; Explain's condition should accept `""` as well as `en`.
    **Approved 2026-10-09.**

67. **Look Up in fixed-layout books (O13).** The plan recommends no lookups in fixed-layout books for v1. 1.1's Mac Look Up already works there, so taking it away would be a regression. Built: extension lookups are off in fixed-layout books; the core Mac dictionary keeps working there.
    *Recommendation:* approve this split; revisit extension lookups with fixed-layout zoom (E2, I17).
    **Approved 2026-10-09.**

68. **The provider label with one provider (O15, LK15).** Built as recommended: plain text, no chevron. *Recommendation:* approve.
    **Approved 2026-10-09.**

69. **Web links in dictionary entries (O14, DX15, Stage 2b).** *Recommendation:* inert text, as the plan says.
    **Approved 2026-10-09.**

70. **MDX encodings (O16, DX3, Stage 2b).** *Recommendation:* UTF-8, UTF-16LE, GBK, GB18030 and Big5 through a reviewed crate (`encoding_rs`, MIT or Apache-2.0), checked at review.
    **Approved 2026-10-09.**

71. **Local-model failure wording (O17, Stage 3).** Explain's own wording, in its private repository. *Recommendation:* as the plan drafts it.
    **Approved 2026-10-09.**

72. **Oxford data (O6).** Your call; nothing in this repository depends on it.
    **Approved 2026-10-09.**

73. **The selection bar's focus ring (O7).** 2.24:1 on Paper and 1.90:1 on Night, under 3:1. Not changed on this branch, because it changes today's bar and its baselines.
    *Recommendation:* a bar-specific ring colour, as a separate fix.
    **Approved 2026-10-09.**

74. **Peek strings in four languages.** The new lookup strings (`lens` in `src/lib/strings/en.ts`, PROVISIONAL, worded from the canvas) have machine drafts in zh-Hans, zh-Hant, ja and es so the shipped catalogues stay complete.
    *Recommendation:* approve the English; send the four drafts to the native reviewers (L-6) before Stage 2a ships.
    **Approved 2026-10-09.**

75. **Visual baselines for the lookup peek (Stage 2a Done-when).** Not captured yet. The peek states (pending, answer, More, What was sent, needs context, errors, the provider menu) in Paper and Night, built from the test provider, are to be captured with `scripts/e2e.sh v` and compared with Canvas 2–9 by eye.
    *Recommendation:* capture them in the next session and approve, or say which states differ from the canvas.
    **Approved 2026-10-09.**

76. **The first-request notice has no shape in LK2 (Canvas 8, EX8).** Explain shows its own notice before the first request to a host (“Explain sends text to DeepSeek · Continue · Not now”), but LK2's fields cannot carry it, so the core renders nothing for it.
    *Recommendation:* add a `notice` status to LK2 (title, text, the host, Continue and Not now; the core draws the buttons), before Stage 3.
    **Approved 2026-10-09.**

## Reading Lens follow-ups (2026-10-09)

77. **Lookup peek baselines (item 75).** Captured with `scripts/e2e.sh v` on the current desktop (active window), in `docs/visual/app/`:
    - Paper: `rl-02-pending`, `rl-03-answer`, `rl-04-more`, `rl-05-menu`, `rl-06-sent`, `rl-07-needs-context`, `rl-08-notice`, `rl-09-error-offline`, `rl-09-error-key`, `rl-11-mac-dictionary`;
    - Night: `rl-03-answer`, `rl-05-menu`, `rl-11-mac-dictionary`; Sepia: `rl-03-answer`.

    They follow Canvas 2–11 in layout and wording, with the test provider's text. Two differences from the canvas: the pending line names the extension (“Asking Test Lookup…”), where the canvas names the model's provider (“Asking DeepSeek…”), because Explain's name is the extension's; and errors have no “Open options” or “Change key” until Stage 2d.
    Two approved screens change on purpose: `11-settings-extensions` (the sample Dictionary is 0.5.0 and adds “Free Dictionary” in the lookup peek) and `12-extension-failure` (the stuck Dictionary's lookup joins its action in “⋯”, with one Restart for both; the focus ring is the bar's ink, item 73).
    `03-more-menu`, `04-navigator-contents`, `05-navigator-search`, `16-command-palette` and `17-goto` also differ from their baselines (0.1–1.1% of pixels: a visible scroll bar, shortcut glyph edges, search snippets). This branch does not change them; their baselines were taken on Desktop 2 on 2026-10-08. Not verified against a capture from `main`.
    *Recommendation:* approve the 14 peek captures and the two changed screens as baselines (`docs/visual/APPROVAL.md`).


78. **A Dictionaries section in Settings (O3, Stage 2b).** Settings gains Dictionaries (Canvas 14): Add dictionary…, the ordered list with on/off and Remove, and each dictionary's entries count and status. It amends G2's list of sections. Built as drawn (provisional) while Stage 2b proceeds.
    *Recommendation:* approve as drawn on Canvas 14.

79. **Dictionary entries on a light card in every theme (O4, DX7, Stage 2b).** Dictionary CSS assumes a light page; at Night an entry would lose its own colours. The entry sits on a light card inside the dark peek. Built so (provisional).
    *Recommendation:* approve.

80. **A nightly Rust toolchain for the parser fuzz target (§6.1).** `cargo fuzz` needs nightly and `cargo-fuzz`, which this Mac doesn't have; installing them changes your toolchains, so it was not done. Meanwhile a mutation fuzzer on stable (`fuzz_mutations` in `src-tauri/src/dictionaries/mdx.rs`) ran for 30 minutes (see `docs/reading-lens-status.md`). The cargo-fuzz target is ready in `src-tauri/fuzz/`.
    *Recommendation:* run `rustup toolchain install nightly && cargo install cargo-fuzz`, then `cd src-tauri/fuzz && cargo +nightly fuzz run mdx -- -max_total_time=1800`, before Stage 2b ships.

81. **Dictionary baselines and strings (Canvas 11, 14).** Captures `rl-11-dictionary-entry-paper`, `rl-11-dictionary-entry-night` and `rl-14-settings-dictionaries` in `docs/visual/app/`. Differences from the canvas: reordering is by Move up and Move down buttons rather than a drag handle (keyboard and VoiceOver can use them); the row shows “MDX with MDD · N entries” without the file size; the entry frame has a fixed height and its own scroll bar (Spike I's fallback); Back returns to the first entry. The Settings › Dictionaries strings (`dictSettings`) and the new peek strings have machine drafts in the four languages.
    *Recommendation:* approve the screens and the English; send the drafts to the native reviewers with item 74's.

