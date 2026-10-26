# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last. Only questions for the owner belong here; other open work is a GitHub issue (since 2026-10-14).

Approved so far; the record is in `docs/decisions.md`:
- items 1–8 and 11–20, on 2026-09-25;
- items 9, 10 and 21–30, on 2026-09-26. Item 28 as the owner settled it: builds need only meet what GitHub requires until there is a Developer ID. Item 29 by making `read-book` public.
- items 31, 33–41, on 2026-10-01 (release 1.1). Item 32 was built as recommended the same day.
- item 42, on 2026-10-02 (the app icon).
- item 56, on 2026-10-07, and item 57, on 2026-10-08 (UI localisation).
- items 58–76, on 2026-10-09 (Reading Lens). Item 60 with the MIT licence; item 59's values on 2026-10-10.
- items 77–81, on 2026-10-10 (Reading Lens follow-ups and Stage 2b).
- item 82, on 2026-10-10, and items 83–87, on 2026-10-11 (Reading Lens Stages 2d and 3).
- items 88–96, on 2026-10-13 (compact selection bar).
- items 102, 105, 106 and 108–112, on 2026-10-17 (sync; 111 and 112 as (a)).
- items 113 (as (a)) and 114, on 2026-10-18 (the sync protocol).

Waiting for the owner: item 117 (Reading Lens, the offline check's items). Item 98 was approved on 2026-10-25, and items 99–101, 103, 104 and 107 (sync) on 2026-10-26. Item 97 was approved on 2026-10-14. Items 53 and 54 were approved on 2026-10-06 (reader review). The items below are kept for their details.

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

## The app icon (2026-10-02)

**Approved 2026-10-02 as recommended** (record in `docs/decisions.md`); kept here for its details.

42. **App icon on the macOS grid, and two artwork details.** The icon now sits on Apple's 824/1024 grid with the standard shadow (see `docs/decisions.md`, 2026-10-01/02), so it no longer looks about 5 % larger and flatter than its Dock neighbours. Two points in the accepted artwork are left as they are, because changing them means a new logo version: at icon size the pointed right page can read as a leaf (the mark as a sprout) as much as a page; and the left page's top-left corner is rounded while its bottom-left corner is square.
    *Recommendation:* approve the grid refit; keep the artwork, unless the leaf reading bothers you in the Dock, in which case a `linen-a-v003` could square the right page's tip and match the left page's corners.

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
    **Approved 2026-10-09.** Values set 2026-10-10 as recommended: G0 X 40%, Y 30% (with floors of 30 local stuck points from 3 readers); G2 upper 95% bound ≤ 5%; G3 per reader, strictly better (`docs/decisions.md`).

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
    **Approved 2026-10-10.**

78. **A Dictionaries section in Settings (O3, Stage 2b).** Settings gains Dictionaries (Canvas 14): Add dictionary…, the ordered list with on/off and Remove, and each dictionary's entries count and status. It amends G2's list of sections. Built as drawn (provisional) while Stage 2b proceeds.
    *Recommendation:* approve as drawn on Canvas 14.
    **Approved 2026-10-10.**

79. **Dictionary entries on a light card in every theme (O4, DX7, Stage 2b).** Dictionary CSS assumes a light page; at Night an entry would lose its own colours. The entry sits on a light card inside the dark peek. Built so (provisional).
    *Recommendation:* approve.
    **Approved 2026-10-10.**

80. **A nightly Rust toolchain for the parser fuzz target (§6.1).** `cargo fuzz` needs nightly and `cargo-fuzz`, which this Mac doesn't have; installing them changes your toolchains, so it was not done. Meanwhile a mutation fuzzer on stable (`fuzz_mutations` in `src-tauri/src/dictionaries/mdx.rs`) ran for 30 minutes (see `docs/reading-lens-status.md`). The cargo-fuzz target is ready in `src-tauri/fuzz/`.
    *Recommendation:* run `rustup toolchain install nightly && cargo install cargo-fuzz`, then `cd src-tauri/fuzz && cargo +nightly fuzz run mdx -- -max_total_time=1800`, before Stage 2b ships.
    **Approved 2026-10-10.**

81. **Dictionary baselines and strings (Canvas 11, 14).** Captures `rl-11-dictionary-entry-paper`, `rl-11-dictionary-entry-night` and `rl-14-settings-dictionaries` in `docs/visual/app/`. Differences from the canvas: reordering is by Move up and Move down buttons rather than a drag handle (keyboard and VoiceOver can use them); the row shows “MDX with MDD · N entries” without the file size; the entry frame has a fixed height and its own scroll bar (Spike I's fallback); Back returns to the first entry. The Settings › Dictionaries strings (`dictSettings`) and the new peek strings have machine drafts in the four languages.
    *Recommendation:* approve the screens and the English; send the drafts to the native reviewers with item 74's.
    **Approved 2026-10-10.**

82. **Stage 2d screens and strings (Canvas 12, 13).** Captures in `docs/visual/app/`: `rl-13-host-sheet`, `rl-12-settings-sends-to`, `rl-12-options-dialog` and `rl-09-error-keychain-paper` (a key the Keychain refused, with Open options). Differences from the canvas:
    - The key is typed into a native macOS dialog with a secure field, not the canvas's HTML sheet, so the value never enters the WebView (LK7). Same title and wording.
    - The host sheet quotes the extension's purpose as its words, and “Sends to” reads “what you choose to send”, because Linen can't know what an arbitrary extension sends; Explain's own page can say more (Stage 3).
    - The options dialog is the extension's frame and Done; the canvas's rows and Test connection belong to Explain's page.

    The new strings (`extAccess`) have machine drafts in the four languages.
    *Recommendation:* approve the screens and the English; send the drafts to the native reviewers with items 74 and 81.
    **Approved 2026-10-10.**

83. **“Not set up” and “‹name› options…” in the peek (Stage 3, LK10, LK11).** The spec's error list has a “Not set up” state with one action, and LK11's menu lists “Explain options…”; Host API 1.1 had neither. A lookup may now answer `error: "not_configured"`, shown as “‹name› isn't set up yet. Finish setting it up in its options.” with Open options; and the provider menu ends with “‹name› options…” for an extension with an options page. Both are experimental Host API 1.1 additions. The strings have machine drafts in the four languages. No capture yet: they will be taken with Explain's own screens.
    *Recommendation:* approve the behaviour and the English; send the drafts to the native reviewers with item 82's.
    **Approved 2026-10-11.**

84. **Explain's live check and notices (Stage 3, EX8).** Explain 0.1.0 is in the private repository `Quentinbest/linen-explain`, with contract tests passing on recorded responses. Two things wait on you:
    - **The live check:** in that repository, `DEEPSEEK_API_KEY=… npm run live -- deepseek --record` sends one fixed sentence about caches (never book text) and records DeepSeek's answer for the tests. The key stays in your shell.
    - **The notices:** `docs/providers.md` there lists each preset's notice and the policy page it comes from (checked 2026-10-11). What couldn't be confirmed (DeepSeek's API retention and training, Anthropic's retention period) is left out of the notices.

    *Recommendation:* run the live check, then approve the notices as written.
    **Live check passed 2026-10-11** (DeepSeek, 1.7 s, valid answer, recorded). **Notices approved 2026-10-11.**

85. **Where Stage 3's offline items come from (G2).** The plan's offline run needs at least 60 items drawn from Stage 1's real stuck points, scored blind by two bilingual reviewers. Gate 0 was waived, so there are none. Options: (a) run the Stage 1 diary now (`docs/reading-lens-stage1.md`), which is still the only source of real items; (b) collect stuck points from your own reading with Linen's rose highlight, coded the same way; (c) a synthetic set, which the plan rejects (spec 4.9: “items drawn from the diary, not a 68-item synthetic set”).
    *Recommendation:* (a), with you as one of the readers; G2 can't be decided before the items and reviewers exist.
    **Approved 2026-10-11: (a).**

86. **The peek's languages (LK11).** The peek offers extension lookups in Linen's UI language and English only. A Chinese-speaking reader with Linen in English can't choose “Explain · 简体中文”. The plan's audience is exactly that reader.
    *Recommendation:* add the languages from macOS's preferred languages (Settings › General › Language & Region) to the menu, after Linen's own; a small change in `readerLanguages`, with a unit test.
    **Approved 2026-10-11; built.**

87. **How the offline check is scored (G2, provisional).** The plan sets the bound (item 59) but not the rubric. Built as follows in `linen-explain` (`offline/`, `docs/offline-run.md`):
    - Each answer is scored blind by each reviewer: **verdict** `correct`, `partly` (right but vague or incomplete) or `wrong` (the meaning given is wrong for this passage); **severe** `y` when the answer is wrong *and* gives no sign of doubt (no qualifier, not “needs more context”), so a reader would likely take the wrong meaning; **term** `ok`, `wrong` or `na` for the Chinese term.
    - **Agreement first:** Krippendorff's α on `severe` (nominal) must be at least .67 before any rate is used; α on `verdict` is reported beside it.
    - **An answer counts as severe when either reviewer marks it** (the conservative reading); the rate when both mark it is reported too.
    - The rate and its Clopper–Pearson 95% upper bound are reported **per model**; G2 passes for a model when the bound is at most 5%.
    - Items: every local stuck point (L2, L3, L4, S1, S2) from the diary sheets, with the reader's name removed; at least 60.
    *Recommendation:* approve as built.
    **Approved 2026-10-11.**

## Compact selection bar (2026-10-13)

The owner asked for a shorter selection bar, with Explain as an icon (2026-10-12). Plan: `docs/selection-bar-plan.md` (rules SB1–SB6); status: `docs/selection-bar-status.md`. Each item was built as recommended while it waited, except item 95. **All approved 2026-10-13 as recommended** (`docs/decisions.md`).

88. **Icons in place of the bar's words (SB3; amends A1).** Note, Copy, Search (Delete for a clicked highlight) and Look Up show their icon only; the colour dots already do. Each has a tooltip (item 89) and a name for VoiceOver: Note, Copy, Search the book, Delete highlight, Look Up. A divider now sets Look Up (and item 91's button) apart from Note, Copy and Search. Attach here and Cancel, and the items inside “⋯”, keep their words; the right-click menu, the Edit menu, ⌘K and the cheat sheet keep naming every action. Delete can be undone. Measured in the captures: 475 → 298 px with no extension, about 518 → 332 px with Explain (its button, item 91), 524 → 347 px with “⋯” (Screen 12): a third shorter.
    New string: “Delete highlight” (Search reuses “Search the book”), with machine drafts in the four languages.
    *Recommendation:* approve; send the drafts to the native reviewers with item 74's.
    **Approved 2026-10-13.**

89. **The bar's tooltips (SB2).** X4 asks for tooltips on icon buttons; Linen's are the native `title`, which shows only on hover, after the system's delay, and can't keep clear of the selection. Proposed instead, for the bar: a tooltip after 500 ms of rest, then at once on the next button; at once on keyboard focus (F6, arrows); gone on leave, click, Esc or when the bar moves; on the side of the bar away from the selection; the tooltip colours already tested at 7:1 in every theme; never takes focus or changes the selection. Its text is the button's name and, where the command has one, its shortcut (“Look Up (⌃⌘D)”). Copy shows no ⌘C: ⌘C is the Edit menu's standard Copy, which may copy styled text, not Linen's plain-text Copy (B4); this wasn't checked in the app.
    *Recommendation:* approve SB2 as written. Separately, check ⌘C on a reader selection; if it pastes plain text, the Copy tooltip can show it.
    **Approved 2026-10-13.** The ⌘C check is still open.

90. **Hit areas on the bar (X4).** X4 gives icon buttons 44 px hit areas. The bar's buttons have been 32 px since Phase 5 (the dots 30 px), in approved baselines; they were labelled, so X4 didn't apply. Icon-only, it would. Options: (a) keep the 32 px faces with no gaps between hit areas, and record an exception to X4 for the selection bar; (b) 44 px per icon, which adds about 70 px and leaves the bar about 20% shorter instead of about 35%.
    *Recommendation:* (a). macOS toolbars use controls of this size; a pointer, not a finger, uses the bar (touch is deferred).
    **Approved 2026-10-13: (a).**

91. **Explain's button on the bar (SB4, SB5; amends LK1).** Explain moves from “⋯” to an icon button after Look Up, named and tooltipped “Explain in context” (the peek's approved words). The core can't name Explain (plan §1.2), so the rule is general: the first lookup that names an icon (item 92), whose condition holds and that isn't stuck, gets the button; others stay in “⋯”. It is hidden, never greyed, when it doesn't apply (over 40 words, a book not in English, fixed layout, safe mode). A stuck one goes back to “⋯” with Not responding and Restart, as Screen 12 draws. “⋯” shows only with something in it, so with Explain alone the bar ends at Explain. The peek's “Explain in context” opens the same lookup as the button. Choosing Explain still makes exactly one request, and selecting none (EP1).
    *Recommendation:* approve.
    **Approved 2026-10-13.**

92. **Host API 1.2: a lookup's icon (SB4, SB6).** A lookup may name `"icon": "lightbulb"`, from a set the core draws; any other name is refused at install. Extensions never supply artwork for Linen's own bar. Linen 0.4.2 refuses manifests with fields it doesn't know, so Explain can name its icon only in a release that requires Host API 1.2 and ships after a Linen that has it. Older extensions (`^1.1`) install as before. The extension docs said lookups are “experimental until Host API 1.2”; they stay experimental in 1.2, whose only addition is `icon`.
    *Recommendation:* approve Host API 1.2 with `icon`, lookups still experimental; release Linen first, then Explain 0.2.0.
    **Approved 2026-10-13.**

93. **The two “Explain” entries.** Checked, not assumed. Your library (read only) has the harness's **Test Lookup** (`test.lookup`, installed 2026-10-09), whose lookup is also titled “Explain”, besides Explain (installed 2026-10-11). Extensions are listed in install order, so the first “Explain” in “⋯”, and the peek's “Explain in context”, open the test provider. They differ: the test provider answers from a script in its storage and may reach only `127.0.0.1:8765`; Explain sends to the provider in its options (DeepSeek is granted). Explain itself contributes one lookup, and the bar shows each lookup once.
    Built (provisional): lookups that share a title are told apart everywhere, “Explain (Test Lookup)” (SB1); the peek's button follows the bar's (SB5).
    *Recommendation:* remove Test Lookup in Settings › Extensions (your library; nothing was changed there), and approve SB1 and its string “‹title› (‹name›)”, with machine drafts in the four languages.
    **Approved 2026-10-13.** You remove Test Lookup in Settings › Extensions.

94. **Explain's icon.** An outline lightbulb in Linen's icon style (24 px grid, 1.6 px stroke, round ends, no fill, no rays), drawn at 16 px like Note and Copy, inverted with the bar at Night. It means “make this clear”: what the words mean here. Not a magnifier (Search), an open book (Look Up), a speech bubble (Note) or a question mark (help). A sparkle would say “AI”, how it works rather than what the reader gets; it is the fallback if you want the AI visible on the bar. Tooltip: “Explain in context”.
    *Recommendation:* approve the lightbulb.
    **Approved 2026-10-13: the lightbulb.**

95. **The extension's name (not built).** “Lens” is short, keeps the Reading Lens lineage, and reads well in “Lens options…” and “Restart Lens”; but it is generic, recalls Google Lens (camera search) and zoom, says nothing of what it does, and needs a name check before any public release. Whatever the name, the action stays “Explain”: Linen builds “‹title› in context”, “Look Up with ‹title›” and “‹title› · 简体中文” from the lookup's title, and “Lens in context” says nothing. Only the display name changes; the ID stays, so keys (Keychain account `<extension id>/<name>`), grants and storage carry over (checked in the code).
    Options: (a) **Reading Lens**: distinctive, says what it's for, matches the spec; its length costs little, because the name appears in Settings, sheets and status lines, never on the bar. (b) **Lens**, if brevity matters more. (c) **Gloss**: exactly “an explanation of a word in its passage”, but less familiar to the readers it is for (Chinese speakers reading English), and “gloss over” is negative.
    *Recommendation:* (a) Reading Lens, in Explain 0.2.0.
    **Approved 2026-10-13: (a) Reading Lens.** Applied in `linen-explain` 0.2.0.

96. **Selection bar captures (items 88–94).** In `docs/visual/app/`, from `scripts/e2e.sh v` on the current desktop (active window), 2026-10-13:
    - Changed: `06-selection-bar` and `14-night-selection` (icons, tooltips' names, the divider before Look Up), `12-extension-failure` (the same bar; “⋯” with the stuck Dictionary, Not responding and one Restart, unchanged).
    - New: `sb-bar-lookup-paper` and `sb-bar-lookup-night` (the test provider's lightbulb after Look Up, no “⋯” because nothing else applies to that passage), `sb-bar-tooltip-paper` (“Explain in context” above the bar, clear of the selection).
    - Not changed by this work, though they differ from their baselines: `03-more-menu`, `04`, `05`, `16`, `17-goto` (as item 77 records) and `rl-05-menu-*` (the macOS languages item 86 added to the peek's menu after its baseline). Their captures were not kept.
    *Recommendation:* approve the six as baselines once items 88, 91 and 94 are decided; recapture if any of them changes.
    **Approved 2026-10-13.** Copied into `docs/visual/baselines/webkit-19618/`.

## How changes are decided (2026-10-14)

97. **What would undo Gate 0's waiver (Reading Lens).** Gate 0 was waived on 2026-10-10, not passed, and Stage 2d shipped under the waiver in 0.4.0: optional hosts, Keychain keys and options pages, as experimental Host API 1.1 (`docs/decisions.md`). The diary (Stage 1) is still running, so G0 can still be measured. The record does not say what happens to Stage 2d if it fails.
    - **(a) Stage 2d stays; G0 governs Explain only.** The 2d features are general extension plumbing, not Explain's: any extension may use them, and removing them would break installed extensions. A failed G0 stops Stages 3–5 (Explain stays private and is not offered to others) and is recorded beside the waiver.
    - **(b) Stage 2d is withdrawn if G0 fails.** Host API 1.1's hosts, keys and options pages are removed in the next release, and Explain stops working.
    - **(c) No consequence.** The waiver stands whatever the diary shows; G0 is no longer measured.
    *Recommendation:* (a). It keeps the waiver honest (a G0 result still decides something) without breaking shipped API.
    **Approved 2026-10-14: (a).**

## Sync across devices (2026-10-16, #73)

The plan is `docs/sync-plan.md`; its rules are SY1–SY34. Nothing is built yet. Stage 1 is built now, and shipping sync waits for a second platform (owner, 2026-10-14 and 2026-10-17). Each item names the stage that needs it.

98. **The new permissions (SY1, SY2; Stage 2).** The design says extensions get no file system (P§19, plan §7.2), so writing into these folders needs a brokered permission. Proposed: `sync.provider` lets an extension offer a sync provider; it is shown at install, strongly warned (“Can store all your books, highlights, notes and settings”). `folder:icloud-drive` and `folder:google-drive` let it read and write a `Linen` folder at the top of that storage, and nothing else; they are consented to in Linen's setup sheet when the reader turns sync on, which names the folder. Bodies reach the provider as handles it cannot read (SY3), so the two built-in providers never see a book or a note.
    *Recommendation:* approve.
    **Approved 2026-10-25: as recommended.**
99. **Where sync is set up (SY6, SY31; Stage 3).** No screen in the design has sync. Proposed: a new Settings section, Sync, after Library (amends G2): a storage menu (None, iCloud Drive, Google Drive and its account), the setup sheet, one status line, Sync Now, Turn Off and Recently deleted…. The two providers are also listed as Built-in in Settings › Extensions.
    *Recommendation:* approve; built provisional and captured for approval in Stage 3.
    **Approved 2026-10-26: as recommended.**
100. **Where the reader decides a conflict (SY13; Stage 5).** Proposed: a “Changed on two devices” group at the top of the Notes tab, as “Couldn't place” is (A8). Each entry shows both versions, with the device and time, and Keep this under each. After the sync that finds them, one quiet message: “2 highlights changed on two devices · Review”. Until the reader chooses, the version changed last shows in the text and the other is kept.
    *Recommendation:* approve.
    **Approved 2026-10-26: as recommended.**
101. **Where deleted things are restored, and removed-book conflicts decided (SY18, SY19; Stage 5).** Proposed: Settings › Sync › Recently deleted…, a sheet listing the books, highlights and notes deleted in the last 7 days, newest first, with the device, the age and Restore. A book whose removal is in conflict (item 102) is listed at the top of that sheet on every device it concerns (“Removed on ‹device›, changed on ‹device›” · Keep the book · Remove), whatever the week, until the reader decides. On a device that changed it, the book also stays in the library, marked “Removed on ‹device›”, until then. The message after the sync that finds such a conflict opens the sheet. Otherwise the library and the Notes tab don't change.
    *Recommendation:* approve. Deleting is rare, and one place for restoring keeps the reading screens as they are.
    **Approved 2026-10-26: as recommended.**
102. **A change and a deletion made apart (SY13, SY20; Stage 1).** One device deletes a highlight (or removes a book); another changed it before they synced. For a book, any change counts: a highlight or note added or changed, a new edition imported, reading further, a per-book setting. Proposed: a conflict the reader decides (item 100), but only when the second device changed it after the last sync it shared with the deleting device. Otherwise the deletion applies without asking. This also covers a device away for more than a week: what was deleted elsewhere never comes back unless the reader keeps it.
    *Recommendation:* approve. It keeps both of your rules: the reader decides between changes, and nothing deleted returns on its own.
    **Approved 2026-10-17: as recommended.**
103. **Which settings sync (SY15; Stage 3).** Proposed: theme, text size, line spacing, font, page width, publisher styles, single-key shortcuts, remapped shortcuts (without extension commands), page-turn crossfade and announcements, the UI language, the library's sort and view, the reading pace, and per book its Pages or Scroll and Simplify styles. Device-only: open at launch, pinned extension commands (extensions don't sync), the docked Navigator tab (it depends on the window's width), safe mode and the sync setup. A new setting stays on the device until it is added to the list. Text size and margins between a Mac and an iPad (the tablet uses 20 px text, P§4) are decided with #80.
    *Recommendation:* approve.
    **Approved 2026-10-26: as recommended.**
104. **A newer place for the open book (SY12; Stage 3).** Opening a book uses the newest place (#73). If the book is already open, proposed: it never moves under the reader; a message (M1) says “Your place on ‹device›: ‹chapter› · Go there” (it may be further on or earlier, SY12), and Back returns (N1).
    *Recommendation:* approve.
    **Approved 2026-10-26: as recommended.**
105. **Merge rules other than highlights and notes (SY12, SY14, SY16; Stage 1).** Proposed: the newest place wins, even when it is earlier in the book; each setting, last opened and finished: the newest wins; a merge never undoes adding a book (only a deletion removes one); when two devices import different updated files of one book while apart (B3), the newest import wins, highlights re-anchor and unplaceable ones go to “Couldn't place”.
    *Recommendation:* approve.
    **Approved 2026-10-17: as recommended.**
106. **Safety values (SY10, SY17, SY24–SY28; Stage 1).** All provisional.
    - **Clocks.** A version from a clock more than 5 minutes ahead names its device in Settings › Sync (“‹device›'s clock is ahead”), because between changes made apart the device whose clock is ahead wins a place or a setting. More than 24 h ahead: set aside; once that device's clock is corrected, it rotates to a fresh clock and its wrong-clock versions are offered for Keep or Discard (SY10), rather than waiting for time to catch up. Deleted content leaves storage once every enrolled device's own measured age for the deletion reaches the week (a device silent for 30 days is named, with Remove).
    - **Guards for changes from other devices, over a rolling 24 hours and in total since the reader last answered (asked at most once a week for the totals).** Deletions past 10 books, 5 GB of book files, a quarter of the library, or 50 highlights and notes ask “Other devices removed … today · Remove · Stop syncing”. Additions past 20 books or 5 GB of book files (asked before downloading the file that would cross it), 200 highlights and notes, or 50 MB of them to one book, and rewrites past 5 books' files, 20 books' places or reading states (added, rewritten or deleted), or 50 highlights, notes and settings ask “Other devices changed … today · Apply · Stop syncing”. Stop syncing puts back everything counted since the reader last answered (the last 24 hours, or longer for the totals), from an undo journal kept on the device for 7 days or until the reader answers.
    - **Backups.** Before the first sync, and before a pass that applies a remote deletion or replaces or removes a book file: a consistent copy of the database (SQLite's backup API) and of `Books/` (APFS clones, no space until files change) in the library folder's `Backups/`. At most one an hour, unless the hour's copy lacks something the pass could remove; 10 kept for 7 days, plus the newest and any one a guard keeps. A library restored from one starts with sync paused (“This library was restored from a backup · Resume · Turn off sync”). Resume asks whether to bring back only what was removed or replaced, or to make this copy win.
    - **Limits.** Per pass: 100,000 names listed, 32 devices, 256 MB read apart from books, 60 s; 1,000 names set aside before sync pauses. Per object: a note or quote 4 MB (a longer local note stays on this device), a book's highlights and notes 64 MB in all, a book's metadata 1 MB, a place 8 KB, a reading state 1 KB, a setting 64 KB, a change file or shard 8 MB. Book files received: 5 GB a day per device; one that arrives damaged twice in a day is quarantined until Try again. Everything sync writes on the device keeps at least 5 GB or 10% of the disk free, and more than 2 GB or 50 books arriving at once asks first.
    *Recommendation:* approve.
    **Approved 2026-10-17: as recommended.**
107. **When it syncs (SY32; Stage 3).** Proposed (provisional): at launch, 10 s after the last local change, every 5 minutes while Linen is open, on quit (at most 2 s), and on Sync Now.
    *Recommendation:* approve.
    **Approved 2026-10-26: as recommended.**
108. **Encrypting what is in storage (Stage 1: it shapes the protocol's formats).** Linen would store books, highlights and notes in iCloud Drive or Google Drive as they are; Apple or Google keep them under their own encryption. Linen could add a passphrase, but a forgotten passphrase loses the synced copy, and the two built-in providers can't read the bodies anyway (SY3). The cost of not encrypting: a lost or sold Mac still signed in to the account can read what is in the folder, including what is added after it was removed from sync; only encryption with a key replaced on each removal would stop that, so the Remove sheet tells the reader to sign that Mac out or erase it.
    *Recommendation:* no encryption in this plan; decide again before any provider other than the two folders is allowed.
    **Approved 2026-10-17: as recommended.**
109. **One storage at a time (SY29; Stage 1, its screens Stage 3).** Proposed: one storage per library. Switching keeps the old copy where it is (the reader may delete its `Linen` folder) and joins the new storage as a first sync, with no duplicates. Both devices must choose the same storage to see each other; Settings › Sync names it.
    *Recommendation:* approve.
    **Approved 2026-10-17: as recommended.**
110. **How a new device is trusted, and removed (SY34; Stage 1, its screens Stage 3).** Anyone who can write to the reader's iCloud Drive or Google Drive folder (another app on a Mac signed into it, or someone with the account) could otherwise add a device and forge deletions. Proposed: each device signs its changes, and other devices apply only the changes of enrolled devices. The first device enrols itself. A new device is enrolled when the reader confirms, on any enrolled device, a short code shown on both (“Allow ‘iPad’ to sync? Code 472 913 · Allow · Not now”), or enters the library's recovery code, shown once on the first setup sheet for the reader to keep. A device never adopts a library someone else's device created without this enrollment, even one that appeared first in the folder. Joining with the recovery code lists the library's devices for the reader to keep or remove before anything from them is applied. If every enrolled device and the code are lost, the reader starts a new library from that device; the old one stays in storage, readable but not joined. Settings › Sync lists the enrolled devices, each with Remove (for a lost or sold Mac); removing one needs the recovery code, so a lost device can't remove the others, also removes any device the removed one enrolled (the reader can keep one with the code), and replaces the code, and that device's later changes are refused everywhere.
    *Recommendation:* approve. The cost is one confirmation per new device. The alternative, trusting whatever is in the folder, leaves the reader's books and notes only as safe as every app and account that can write there.
    **Approved 2026-10-17: as recommended.**
111. **A deletion's week on a device back late (SY17; Stage 1).** Your rule: what was deleted is kept for a week and can be restored during it, and a device away for more than a week does not bring it back. Devices can't compare clocks they don't trust, so each reports how long ago it saw a deletion. Reviews pulled two ways. A device back late should not offer a fresh week; but if other devices' reports alone could end the week, a compromised device could hide Restore for anything it deletes, and it can enrol extra keys to agree with itself. Options:
    - **(a)** Other devices' reports never shorten this device's Restore. A device back late offers Restore for its own week, labelled with the reported age (“Deleted on ‹device› about 10 days ago · Restore”), and holds the content meanwhile. Nothing comes back unless the reader chooses Restore.
    - **(b)** Two devices whose enrollment is independent (neither enrolled the other, directly or through others) can end the week for a third. A device back late shows the deletion as past its week. A compromised first device could still enrol two independent-looking keys, so this is weaker.
    *Recommendation:* (a). Restoring stays the reader's explicit choice, so “does not bring it back” holds in the sense that nothing returns by itself; and no device, however compromised, can hide another device's Restore.
    **Approved 2026-10-17: (a).**

## Sync Stage 0 (2026-10-17, #91)

112. **Where the sync device ID is kept (SY7; Stage 1).** SY7 plans a *ThisDeviceOnly* item in the data-protection keychain, so Migration Assistant and backups can't copy the ID to another Mac. Stage 0 found that an ad-hoc-signed program, as Linen's releases are, can't use that keychain (`-34018`, missing entitlement), and that the login keychain ignores the *ThisDeviceOnly* class (`docs/spikes/sync-folders.md`). Options:
    - **(a)** Keep the ID in the login keychain for now. Since the writer token and high-water mark would migrate with it, also record the Mac's hardware UUID with the ID. On a mismatch (a Mac set up by Migration Assistant), the device takes a new ID. A same-Mac Time Machine restore (the keychain and the library both older) is caught by storage holding changes under this ID newer than the device's own mark. Move to the *ThisDeviceOnly* item only once a built release is shown to carry the keychain entitlement, which needs a provisioning profile as well as a Developer ID (#53).
    - **(b)** Wait for a Developer ID before building Stage 1.
    *Recommendation:* (a). The hardware UUID can't migrate, and Stage 1 tests both cases: a migrated copy and a restored older Mac.
    **Approved 2026-10-17: (a).**

## Sync protocol (2026-10-17, #92)

The protocol is `docs/sync-protocol.md` (SP1–SP37), the first part of Stage 1, written for its own security review. The owner reviewed it and merged it (#101) on 2026-10-18.

113. **A device that missed a removal (#86; SP31, SP32; Stage 1).** If the storage, or whoever controls what one device sees of it, hides a removal from that device and holds the removed device's key, everything the device sees is correctly signed, and nothing in the folder can show it the removal. An outside source (a time server, a Linen service) would send something without the reader choosing it, which D1 and P§20 rule out. What already bounds it: the device keeps undo data for every change the others haven't acknowledged, so it reverts all of them once the removal reaches it; SY26 and SY27 cap and back up what it applies meanwhile; and the device that removed names each device that hasn't caught up yet (SP31). Options:
    - **(a)** Also SP32: when a device has heard nothing for 7 days (prov.) from any enrolled device other than the writer and the devices the writer enrolled, that writer's deletions, book-file replacements and changes to highlights and notes wait, and Settings › Sync asks once (“Linen hasn't heard from ‹device› for 9 days. ‹device› removed 3 notes · Apply · Not now”). The reader at the device is the outside source, and nothing leaves it. Additions, places and settings still apply. It asks needlessly when a third device just sits unused for a week.
    - **(b)** Only SP31: no waiting, and §8's risk 5 stays a known limit.
    - **(c)** Either of the above, and also keep the undo data and local copies for every deletion and replacement from another device for 30 days rather than SY26's 7 (item 106), so a removal hidden for up to a month can always be undone, whoever the device hears from. Costs disk space on each device for a month of deleted content. (Raised by the PR's Codex security review: two old devices that only hear from each other count as each other's witnesses under (a).)
    *Recommendation:* (a). It catches the likely case (you removed a lost Mac on one device and keep using it, while the storage hides that from another) for one question a week at most; once you answer, it applies what (b) would.
    **Approved 2026-10-18: (a).**
114. **The new device confirms the code too (SP24, SY34; Stage 1, its screens Stage 3).** Item 110 asks the reader to confirm the six-digit code only on the enrolled device. The new device, though, can't tell a real library from one a stranger made in the folder, and a stranger could answer it and enrol it at once in a library of their own, whose deletions it would then apply to the reader's books and notes. Proposed: the new device shows the code with “Does ‹device› show this code? · It matches · Cancel”, and joins only after It matches.
    *Recommendation:* approve. It is one more click when adding a device, and without it the code protects only the library, not the device joining it.
    **Approved 2026-10-18.**

## Sync Stage 1b (2026-10-20, #92)

115. **When SY27's Resume is built (Stage 1b or later).** A library restored from a backup starts paused (built in Stage 1b.3b). Resume offers two choices: bring back what was removed or replaced while keeping other newer changes, or make this copy win. The first must know which deletions and replacements *this device* applied after the backup was taken. That comes from the device's signed frontier in storage (SP29, Stage 1c) and from what storage holds (Stage 2). The restored database can't tell on its own, because those changes happened in the database it replaced.
    - **(a)** Build Resume in Stage 2, once frontiers (1c) and storage (2) exist, with SY27's restore check (“A backup restored after a sync removed one book… after Resume the book is back on every device”).
    - **(b)** Build a simpler Resume now: “make this copy win” only, re-versioning every record that differs from what storage sends.
    *Recommendation:* (a). Choice (b) alone would undo every change made elsewhere since the backup, which SY27's first choice exists to avoid. Until then a restored library stays paused, so nothing is lost.
    **Approved 2026-10-20: (a).**

## Sync Stage 1c.4c (2026-10-24, #131)

116. **A confirm that joins sibling branches (SP20 (4a), (5); Stage 1c.4c).** A `confirm` counts a removal from a sibling branch as if it were made on the confirm's own head. That joins the two branches, so the fork ends and the devices enrolled on the other branch after the fork stop being suspended, without the reader choosing Keep or Remove for each. A `merge` must record that choice (`keeps`); a confirm has no `keeps`. Options:
    - **(a)** A confirm that joins siblings keeps their post-fork devices suspended until a merge records `keeps` for them.
    - **(b)** A confirm that joins siblings must carry `keeps` like a merge, and counts only when it names every such device.
    *Recommendation:* (a). The confirm stays as SP19 defines it, the merge stays the one record where the reader decides each post-fork device, and nothing is trusted that the reader hasn't looked at. Until this is answered, it is built as neither: #131 tracks it, and sync has no caller yet.
    **Approved 2026-10-24: (a).**

## Reading Lens Gate 0 (2026-10-26, #52)

117. **Where the offline check's items come from (G2; Stage 3).** The offline run (G2) takes its at least 60 items from the coded diary sheets, and the diary gave one uncoded stuck point (Gate 0 not measured, 2026-10-26). Explain stays private either way.
    - **(a)** G2 waits. A later diary, with readers found first, gives the items; it becomes its own issue when the owner wants it.
    - **(b)** Build the items another way: the owner and the two reviewers pick passages from English technical books, coded with Report 1's types. This changes the plan's source for G2's items.
    - **(c)** Drop the offline run for now. G2 stays unmeasured and Explain stays private and unoffered.
    *Recommendation:* (a). It keeps G2 as planned and costs nothing while Explain is private; (b) is a quicker route but the items would be the reviewers' guesses at what is hard, not readers' stuck points.
