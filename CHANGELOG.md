# Changelog

Every Linen release, newest first. Each entry is the summary that goes into the release's notes on GitHub; the release workflow copies it from here (see `.github/workflows/release.yml`).

## v0.5.1 — 2026-10-13

Linen 0.5.1 fixes one thing in 0.5.0.

- **Highlights stay visible when you come back.** Going back to a highlight's page from the Notes tab after reading elsewhere, or turning pages into its chapter, could show the text with no highlight, underline or note dot. They now appear every time, in Pages and in Scroll.

No text changed, so the translations are the same as in 0.5.0.

## v0.5.0 — 2026-10-13

Linen 0.5.0 makes the selection bar shorter and quieter.

**Selection bar**

- **Icons:** Note, Copy, Search (Delete on a highlight) and Look Up now show their icons only. Each has a tooltip with its shortcut, when you rest the pointer on it or reach it with F6 and the arrow keys. VoiceOver still reads each action's name. The bar is about a third shorter.
- **Explain in context:** an extension's lookup can have its own button after Look Up, such as Explain's lightbulb. Selecting text still sends nothing until you choose it.
- **Clearer names:** two lookups with the same name now show which extension each comes from, for example “Explain (Test Lookup)”.

**Extensions** (Host API 1.2, experimental)

- **A lookup's icon:** a lookup can ask for an icon that Linen draws (for now, a lightbulb), and it gets its own button on the selection bar. Earlier versions of Linen refuse an extension that asks for one.

**Translations:** the new text in Simplified Chinese, Traditional Chinese, Japanese and Spanish is a draft, still to be reviewed by native speakers.

## v0.4.2 — 2026-10-12

- One click in the book text closes every panel, including Contents, Notes and Search docked beside the text on wide windows.

## v0.4.1 — 2026-10-12

- Explain answers again. Before, every lookup hung for 10 seconds and failed, whatever the provider.
- Scrolling over a panel (Aa, the selection bar, a note, a peek) scrolls the panel instead of turning the page and closing it.
- Scrolling in the Settings window no longer turns pages in the book.
- Panels close when you click outside them.
- Right-clicking a selection offers everything in the selection bar, including Look Up with Explain.
- A slow AI provider no longer counts toward an extension being suspended.

## v0.4.0 — 2026-10-11

Linen 0.4.0 looks words up in place, in your own dictionaries as well as your Mac's, and lets extensions answer there too.

**Look Up**

- **A new peek:** Look Up opens under the selection, never over the line you're reading. It closes when you turn the page, scroll, resize or press Esc, and you never lose your place.
- **Your own dictionaries:** add MDX dictionaries (with their MDD files) in Settings › Dictionaries, and put them in the order you want. Look Up opens on the first one with an entry; this Mac's dictionaries stay in the list. Entries are shown as their authors made them, on a light card in every theme. They can't run scripts or reach the internet.
- **Choose where to look:** the name above the answer is a menu: your dictionaries, this Mac's, and lookups from extensions you've installed, in each language you use (Linen's language, your Mac's preferred languages, and English).
- **Highlights too:** click a highlight, and its bar offers Look Up and your extensions' lookups.

**Extensions** (Host API 1.1, experimental)

- **Lookups:** an extension can answer in the peek with plain text that Linen lays out and labels. Nothing is sent until you choose it; the first time it sends text to a site, it says what and where, and waits for Continue. **What was sent** shows the exact request.
- **Sites on request:** an extension can ask for a site when it needs one, in a sheet from Linen. Allowed sites are listed in Settings › Extensions under “Sends to”, with Remove.
- **Keys stay with Linen:** an extension can ask you for an API key. You type it into a Mac dialog, it goes into your Mac's Keychain, and Linen adds it only to requests to that one site. The extension never sees it. Removing the extension removes its keys.
- **Options pages:** an extension can have its own Options…, in Settings › Extensions and in the peek's menu.

**Also**

- The selection bar's focus ring is easier to see in every theme.
- Linen is now MIT-licensed.

**Translations:** the new text in Simplified Chinese, Traditional Chinese, Japanese and Spanish is a draft, still to be reviewed by native speakers. Everything that was in 0.3.0 is unchanged.

## v0.3.0 — 2026-10-08

Linen 0.3.0 speaks Simplified Chinese, Traditional Chinese, Japanese and Spanish.

**Languages**

- **Four new languages:** menus, settings, messages and screen-reader text in Simplified Chinese, Traditional Chinese (Taiwan usage), Japanese and Spanish.
- **Follows your Mac:** Linen uses the first of your Mac's preferred languages that it has, or the language you choose for Linen in System Settings › General › Language & Region › Applications. If your Mac's language isn't one of these, Linen stays in English.
- **Or choose in Linen:** Settings › General › Language. The new language applies after Linen restarts.
- **Books keep their own language:** titles, contents, quotes and footnotes are marked with the book's language, so Chinese and Japanese characters take the book's forms and VoiceOver reads them in the right voice.

**Also**

- **Dates, times, percentages and sorting** follow the chosen language. English looks as before.
- **In Chinese and Japanese,** lines break between phrases rather than in the middle of a word, and headings use the system font.

## v0.2.1 — 2026-10-06

Linen 0.2.1 fixes navigation and scrolling problems found in everyday reading.

**Fixes**

- **Contents and search in Scroll mode:** choosing a chapter or a search result now goes straight there. Before, the page could stutter, show the chapter in the wrong place first, or not move at all.
- **Scrolling Contents:** the wheel and trackpad scroll only Contents, also at its top and bottom; the book stays where it is.
- **Toolbars in Scroll mode:** moving the pointer to the top or bottom edge brings up both bars, as it does when turning pages.

**Also**

- **A minimum window size of 760 × 480,** so the reader and its controls always fit.
- **Long titles** in the top bar end in “…” instead of running over the buttons.
- **Messages** such as “Back to page …” sit above the bottom bar while it shows, so they no longer cover the progress bar.

## v0.2.0 — 2026-10-02

Linen 0.2.0 brings the first set of comfort features after the first release.

**Reading**

- **Font and page width** in the Aa popover, for all books: the book's own fonts, Literata, the system sans, or OpenDyslexic; narrow, normal or wide pages.
- **Publisher styles** (Settings › Reading): Full, Balanced or Off. One badly styled book can be simplified on its own with _Simplify Styles for This Book_, with Undo.
- **Look Up**: select a word and choose Look Up (or press ⌃⌘D) to see its definition from the dictionaries on your Mac. Nothing leaves your Mac.
- **Search options**: whole words, and regular expressions.

**Library and shortcuts**

- **List view**: show All books as covers or as a list.
- **Your own shortcuts** (Settings › Shortcuts): change or remove any command's shortcut, including commands from extensions.

**Extensions**

- A new `reading.sessions` permission lets an extension hear when, and how long, you read, for example to keep reading statistics. Linen keeps none itself.

**Also**

- A new app icon.
- If this copy of Linen can't update itself (a standard account, or an app an administrator installed), it now says when a new version is available, with a link to download it.

## v0.1.2 — 2026-10-01

Linen 0.1.2 fixes the edges in immersive mode.

- Moving the pointer to an edge brings the progress bar back, and the bottom edge works again.

## v0.1.1 — 2026-09-30

Linen 0.1.1 makes immersive mode quieter and fixes what a code review found.

- **Immersive mode:** no location line, and the top edge brings back the top bar alone, with the title only.
- **Fixes:** Linen saves your place before it quits, importing a book again is safe, a book's XML is checked more strictly, and the updater is more robust.

## v0.1.0 — 2026-09-26

The first release of Linen, a quiet EPUB reader for macOS.

**Requirements:** macOS 13 or later with Safari 16.4 or later.

**Download:** `Linen_0.1.0_aarch64.dmg` for Apple silicon, `Linen_0.1.0_x64.dmg` for Intel Macs.

**First open:** this build is not yet signed with an Apple Developer ID, so macOS asks before opening it. After dragging Linen to Applications:

- Control-click Linen and choose **Open**, then **Open** again; or
- open it once, then go to System Settings › Privacy & Security and choose **Open Anyway**.

**What's in it:**

- Pages and Scroll reading, including right-to-left and vertical writing.
- Contents, Go to, search, and highlights and notes with Markdown and W3C export.
- Themes (Paper, Sepia, Night), a library, and extensions.
- VoiceOver support.

Linen sends nothing anywhere; your library stays in one folder on your Mac (see the README for backup and restore).

**Updates:** Linen checks once a day and installs signed updates in the background.
