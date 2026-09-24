# Linen (Quiet EPUB Reader) — implementation plan

Status: draft, revision D (rewritten on 2026-09-24 against the approved design files; C1–C6 decided the same day; see §11) · Design sources: `docs/design/` (proposal, system, screens)

## 1. Sources and scope

### 1.1 Design sources

All three files were approved on 2026-09-24 (`docs/design/APPROVAL.md`) and are committed in `e0c38fe`.

| File | Contents | Cited in this plan as |
|---|---|---|
| `Quiet EPUB Reader (proposal).html` | Written proposal in 5 boards, §1–25: vision, users, IA, canvas, modes, navigation, search, annotation, typography, library, interaction, keyboard, visual system, motion, accessibility, cross-platform, EPUB handling, plugins, plugin security, MVP, state model, failure table, references, screen set, recommendation | **P§n** |
| `Quiet EPUB Reader (system).html` | S1 information architecture, S2 product state model, S3 reading-canvas anatomy, S4 visual system (all theme tokens), S5 extension architecture (Host API v1) | **S1–S5** |
| `Quiet EPUB Reader (screens).html` | The 17 screen boards, 01 Library … 17 Footnote peek · Go to location (renamed from `Quiet EPUB Reader.html`, unchanged since `3f5da5e`) | **Screen nn** |

Each file is a self-extracting bundle: boards are nested, gzip-compressed pages that render only with JavaScript. They can't be grepped or diffed as committed, so Phase 0 adds a text export (§5, Phase 0).

**When sources disagree.** Nothing is resolved silently. Every conflict found is listed in §10.2 with a recommended default. The owner either accepts the default or decides otherwise before the phase that needs it.

### 1.2 Spec gate (blocks Phase 1)

1. ~~Owner approval of the screens and the written proposal (Q1).~~ **Done 2026-09-24.**
2. ~~The proposal and system boards are in the repository.~~ **Done in `e0c38fe`.** This replaces revision C’s requirement to transcribe the canvas into `docs/spec/behaviour.md`. The rule register (§2) cites the committed files directly, so there is no second copy to drift.
3. The text export of the design files is committed (Phase 0), and a reviewer has checked every rule-register row against it.
4. ~~Design conflicts C1–C6 (§10.2) are decided, or their defaults accepted.~~ **Done 2026-09-24:** the owner accepted all six defaults.

**Approved and provisional values.** A value is **approved** if the design states it or the owner approved it in this plan. Otherwise it is **provisional**. This plan marks provisional values with “(prov.)”. Tests assert approved values. Provisional values live in the same config module as approved ones, but are labelled there too. A value moves from provisional to approved only with owner sign-off.

### 1.3 MVP scope (P§20)

In scope:

- **Open and import:** OS open, drag and drop, ⌘O; books copied into the library; EPUB 2 and 3, reflowable; basic fixed layout.
- **Library:** Continue reading, cover grid, search, sort, remove.
- **Reading:** Pages and Scroll; Paper, Sepia, Night, Auto.
- **Navigation:** Contents tab, chapter keys, scrubber, Back history, footnote peeks, Go to location.
- **Search:** incremental full-text search in the open book, grouped results, marks in the text, next and previous.
- **Selection, highlights and notes:** four colours, margin notes, edit, delete with Undo, Notes tab.
- **Comfort:** Aa popover and size shortcuts.
- **Progress persistence:** CFI saved on page turn (1 s debounce), on blur and on quit.
- **Extensions:** manifest, sandbox, permissions, commands, selection actions, Navigator tab, themes, annotation export and events, Extensions page. Markdown Export is the first extension.
- **Command palette.**

Out of scope for the first release:

- **Not planned:** sync, accounts, AI, TTS, statistics dashboards, social features, a store, DRM, collections/tags/series, an extension marketplace, phone layouts.
- **Book scripts:** scripted EPUB content never runs (approved 2026-09-24; §7.1).
- **Tablet:** iPad is out of the first release (Q2) and planned for the next one. Desktop touch (the Screen 13 zones on touch-screen laptops) is in scope.

The design schedules these for later releases. They are not built now, but the architecture must leave room for them:

- **1.1:** font family choice, page width, the Publisher styles setting and per-book “Simplify styles” (C5), library list view, dictionary and translation peek providers, metadata providers, reading-statistics session events.
- **1.2:** TTS, sync providers.
- **Later:** importing other formats, book-processing transforms.

## 2. Rule register

Each rule gets an ID. Tests name the ID they assert, and the config module uses the same ID as its key. Values are approved unless marked (prov.).

### 2.1 Canvas and typography — L

| ID | Rule | Source | Phase |
|---|---|---|---|
| L1 | Measure is 66 ch of the current font (≈ 640 px at 19 px), clamped to 56–74 ch; CJK 34–40 | P§4, S3 | 0, 2 |
| L2 | Column = min(measure, W − 2 × min margin). Side margin = (W − measure) / 2, at least 24 px on desktop. Extra width becomes margin, never longer lines. At large sizes, margins shrink before characters per line fall | P§4, S3 | 2 |
| L3 | Page height = viewport − max(48 px, 7vh), snapped to whole lines; never a half line at the foot | P§4, S3 | 0, 2 |
| L4 | Font size: 19 px desktop default, scaled by the OS text size; 12 steps from 14 to 32 px, up to 48 px with ⌘+ | P§4, P§9 | 2, 6 |
| L5 | Line height 1.55 (Compact 1.4, Loose 1.75); +0.05 above 70 characters per line | P§4, S3 | 2 |
| L6 | Paragraphs keep the book’s convention; if it has neither indent nor spacing, add 0.8 em | P§4 | 2 |
| L7 | Justified text only when hyphenation is on; otherwise follow the book’s alignment without justifying | P§4 | 2 |
| L8 | Width breakpoints: below 760 px, one column, minimum margins, panels float · 760–1100 px, Navigator floats · 1100–1480 px, Navigator docks and the column recentres · 1480 px and up, two-page spread in Pages mode | S3, P§4 | 2, 3 |
| L9 | Below 480 px of height the location line is hidden | P§4 | 2 |
| L10 | Reflow re-paginates around the CFI of the first visible character; resize is debounced by 120 ms; only the current chapter reflows live, the rest when idle | P§4 | 2 |
| L11 | Images fit the column and page height and are never split across pages; at Night they are dimmed about 10%, never inverted | P§4, P§17 | 2 |
| L12 | Tables and code wider than the column scroll horizontally inside a container with an edge fade. Tables break between rows; code keeps its whitespace and is never hyphenated or justified | P§17 | 2 |
| L13 | The publisher decides structure (headings, emphasis, small caps, drop caps, poetry indents, special blocks, images, tables). The reader decides comfort (size, body line height, text and background colours, margins, measure). Absolute font sizes are converted to relative units | P§17 | 2 |
| L14 | Book CSS is sanitised: no scripts, no network requests, no fixed positioning. Computed colours are contrast-checked | P§17, P§22 | 2 |
| L15 | Embedded fonts are used by default and obfuscated fonts are decoded. After a 1.5 s load timeout or failed glyph coverage, fall back to Literata without a prompt | P§17, P§22 | 2 |
| L16 | Chapters over ~1 MB are split into virtual sections at element boundaries. Pagination starts near the viewport; locations show “≈” until done, then settle silently | P§17, P§22 | 2 |
| L17 | Large books are read from the zip on demand. Only the current spine item and its neighbours are loaded | P§17 | 2 |
| L18 | When over 30% of a book is code or tables, the Aa popover shows a one-line hint to try Scroll | P§5 | 6 |

### 2.2 Input and page turning — I

| ID | Rule | Source | Phase |
|---|---|---|---|
| I1 | Wheel (notched), Pages mode: one notch = one page, 250 ms cooldown | P§5 | 2 |
| I2 | Trackpad, vertical, Pages mode: distance accumulates; one turn at 80 px, then locked until the gesture and its momentum end | P§5 | 2 |
| I3 | Trackpad, horizontal, Pages mode: one turn per swipe; content follows the fingers and respects natural scrolling | P§5 | 2 |
| I4 | A gesture’s axis is locked by its first 12 px of travel. Detection uses accumulated distance, not event counts | P§5 | 2 |
| I5 | Momentum after a turn is ignored: gesture phases on macOS, steadily decaying deltas on Windows precision touchpads | P§5, P§16 | 0 (Spike B), 2 |
| I6 | Rapid input queues at most one pending turn; neighbouring pages are pre-laid out so a turn renders within a frame | P§5, P§22 | 2 |
| I7 | More than 3 pages in under 1 s shows a “Back to page N” chip | P§5 | 2 |
| I8 | Keys in Pages mode: → ← · Space ⇧Space · PgDn PgUp · ↓ ↑ = next / previous page | P§5, P§12 | 2 |
| I9 | Keys in Scroll mode: Space and PgDn move a screen with two lines of overlap; ↓ ↑ scroll by line | P§5 | 2 |
| I10 | Space turns a page only when the text has focus; on a focused button it presses the button | P§6 | 2 |
| I11 | Desktop margins are click targets for previous and next, with a faint chevron on hover. Clicks inside the column never turn pages. The click that activates an inactive window never turns one. Nothing turns during a selection drag | P§5, P§6, S3 | 2 |
| I12 | Touch zones 30 / 40 / 30 = back · controls · forward. A tap on a link, note marker or highlight wins over the zones; taps near selection handles never turn pages | P§5, Screen 13 | 2 |
| I13 | Touch swipe tracks the finger; commits past 30% of the width or on a flick; settles in 180 ms with no bounce | P§5, P§14, Screen 13 | 2 |
| I14 | Touch laptops choose behaviour per event from the pointer type (a touch tap in text turns, a mouse click selects); layout density never switches mid-session | P§11, P§16 | 2 |
| I15 | RTL books follow the spine’s page-progression-direction: ← is next, the left zone moves forward, swipes mirror, progress fills right to left. Arrow keys stay spatial | P§5, Screen 13 | 2 |
| I16 | Vertical writing (vertical-rl) paginates right to left with ruby preserved; Scroll becomes horizontal for these books | P§5, P§17 | 8 |
| I17 | Pinch has no effect on reflowable books; it zooms fixed-layout pages | Screen 13, P§17 | 2 |

### 2.3 Chrome, layers and state — S

| ID | Rule | Source | Phase |
|---|---|---|---|
| S1 | Three independent reader lanes, each with at most one state. Chrome: Immersive ⇄ Controls visible. Docked panel: None or Navigator (Contents, Search, Notes, extension tab). Floating layer: None or one of Aa, Go to, Selection bar → Note card, Footnote peek, Image view, ⌘K | S2, P§21 | 1 |
| S2 | Opening a floating layer closes the current one. Below 1100 px the Navigator floats and joins the floating lane | S2 | 1 |
| S3 | Chrome and Navigator alternate: opening the Navigator hides the bars, and its header carries Library, title and progress | S2 rule 1 | 1, 3 |
| S4 | Esc pops exactly one level (floating layer → Navigator → chrome → immersive page) and never leaves the book | S2, P§12 | 1 |
| S5 | Closing any layer returns focus to the text at the same reading position | S2 rule 2 | 1 |
| S6 | A page turn closes the floating layer (a note card saves first) and hides the chrome; the Navigator stays | P§21, S2 | 1, 2 |
| S7 | Starting a selection dismisses popovers and peeks | P§21, S2 | 1, 5 |
| S8 | Only modals (⌘K, dialogs) suspend reader input; focus is trapped only in modals | P§21, P§15 | 1 |
| S9 | Chrome reveal: pointer at the top or bottom edge with a 150 ms dwell reveals that bar only; the top zone is 64 px (bottom: 64 px, prov.). Centre tap toggles both bars. Tab reveals the chrome and focuses its first control | P§6, S2, S3 | 2 |
| S10 | Chrome hides on page turn, Esc, or 3 s after the pointer leaves an edge — never while a control has focus or a popover is open | P§6, S2 | 2 |
| S11 | OS edges: in macOS full screen the top zone starts below the system menu bar; the bottom zone is off when the Dock or taskbar auto-hides at that edge | P§6, S3 | 2 |
| S12 | The pointer hides after 2 s idle | P§1 | 2 |
| S13 | Per book, remember the location, layout mode and whether the Navigator was docked | S2 rule 6 | 2 |
| S14 | The reader is never unloaded while layers are open; after going to the library the book stays warm in memory | P§3, P§2 | 2, 6 |
| S15 | Top bar: Library · Contents · Search · Notes · Aa · ⋯. Bottom bar: progress scrubber · Go to. Eight controls in total | S1, P§6 | 2 |
| S16 | Extensions can occupy a Navigator tab or a menu item, never a new lane | S2 rule 7 | 7 |

### 2.4 Navigation — N

| ID | Rule | Source | Phase |
|---|---|---|---|
| N1 | Every jump (contents, search result, link, annotation, percentage, Go to) pushes the previous location; Back (⌘[ · Alt+← · mouse back button) pops it | P§3, P§6, S2 rule 4 | 2 |
| N2 | A “Back to page N ⌘[” chip appears after every jump | P§6, Screens 08, 12 | 2 |
| N3 | New books open at the bodymatter landmark, not the cover | P§2 | 2 |
| N4 | Resuming deep in a book shows “Resumed in Chapter N · Go to beginning” as a queued message with the standard 10 s timer (M2; C1) | P§2, Screen 12, C1 | 2 |
| N5 | Leaving for the library writes the position immediately | P§3 | 2 |
| N6 | Contents comes from nav → NCX → a heading scan (labelled “Generated from headings”) → the spine list. The current entry is marked “You are here”; damaged chapters are marked | P§17, P§22, Screens 04, 12 | 3 |
| N7 | The scrubber tooltip previews the chapter and % while dragging | P§6, Screen 03 | 3 |
| N8 | Go to (⌘J, or click the location label): Percent, Chapter or Print page. A preview line shows the target (“31% is in Chapter 42 · …”); the current place stays in Back history. Print page appears only when the book has a page-list | P§6, Screen 17 | 3 |
| N9 | Footnote peek: opens below the marker (above it near the page foot) and never covers the marker’s line; scrolls inside up to 50% of window height; never navigates. The marker keeps focus styling; Tab moves into the peek; Esc closes and returns focus to the marker. Actions: Open note in place, Copy. Referenced `epub:type="footnote"` asides are removed from the flow | P§6, P§17, Screen 17 | 3 |
| N10 | Internal links jump, push history and show the Back chip. External links open in the system browser, with the URL shown on hover or long-press; there is no in-app browser | P§6, P§17 | 3 |
| N11 | Images open a zoomable image view on click or double-tap | P§17 | 3 |

### 2.5 Search — F

| ID | Rule | Source | Phase |
|---|---|---|---|
| F1 | Entry: ⌘F, the search button, or “Search in book” in ⌘K; a current selection pre-fills the field | P§7 | 4 |
| F2 | Runs 150 ms after typing stops, from 2 characters (1 for CJK); ignores case and diacritics; quotes match exact phrases | P§7 | 4 |
| F3 | Current chapter first, then onward, wrapping; results stream in. While indexing, the header reads “Searching N of M chapters”; typing is never blocked | P§7, Screen 05 | 4 |
| F4 | Live count (“14 results in 5 chapters”, with “so far” while running). Snippets about 90 characters, cut at word boundaries, match emphasised. Results grouped by chapter with counts; current chapter expanded and marked | P§7, Screen 05 | 4 |
| F5 | Marks in the text: every visible match has a soft tint plus a 1 px outline (3.8:1); the active one a stronger tint plus a 2 px accent outline. Marks exist only while Search is open | P§7, S2 rule 3 | 4 |
| F6 | ↵ / ⇧↵ in the field; ⌘G / ⇧⌘G (F3 / ⇧F3) anywhere; ↑ ↓ move through results and the page follows | P§7, P§12 | 4 |
| F7 | Esc returns to the original page if you only browsed; after choosing a result you stay and Back returns | P§7, Screen 05 | 4 |
| F8 | The index is built in a background worker on first search and persisted | P§7, P§17, S5 | 4 |

### 2.6 Selection, highlights and notes — A

| ID | Rule | Source | Phase |
|---|---|---|---|
| A1 | Selection bar: four colour dots, Note, Copy, Search, “⋯” (extension actions). It sits 12 px above the first selected line and flips below when there is less than 56 px above; it never covers the selection. It is ink-coloured, fades in over 100 ms after mouse-up, and hidden actions stay hidden rather than greyed | P§8, P§13, Screen 06 | 5 |
| A2 | The bar is announced as “Selection actions, F6”. F6 moves focus in, arrows move between actions, Esc returns to the selection | P§8, Screen 06 | 5 |
| A3 | H / ⇧⌘H applies the last-used colour; N / ⇧⌘N adds a note | P§8, P§12 | 5 |
| A4 | Clicking a colour saves the highlight with no confirmation. Highlights are a tint plus a 2 px darker underline (3.5–4.9:1 on Paper and Sepia) in every theme; the Night tint is fainter. Lists always name the colour (Yellow, Green, Blue, Rose) | P§8, S4, Screen 10 | 5 |
| A5 | A plain click on a highlight reopens the bar with colour, Note, Copy and Delete; a drag that starts on one begins a new selection | P§8 | 5 |
| A6 | Notes belong to a highlight. From 1240 px the card opens in the margin beside the passage; below that it is a bottom sheet with Delete and Done. It saves as you type and shows its status; Esc or a click elsewhere closes it; an empty note is discarded and the highlight kept. A margin dot marks passages with notes | P§8, Screens 07, 15 | 5 |
| A7 | Delete is immediate, with an Undo message and ⌘Z; never a confirmation dialog | P§8 | 5 |
| A8 | Notes tab: highlights by chapter in reading order, filter by colour and text, “N highlights · M notes”. Choosing one jumps, pulses the passage for 1.2 s and offers Back. Unanchorable ones are listed under “Couldn’t place” with their quote, a reason and Re-attach; they are never dropped | P§8, Screen 08 | 5 |
| A9 | Two anchors per highlight: an EPUB CFI and a text quote with prefix and suffix. Stored as W3C Web Annotation–compatible JSON | P§8, S5 | 5 |
| A10 | Touch: long-press with system handles; the bar sits below the selection. Right-click gives the native context menu with the same actions; force-click keeps macOS Look Up | P§8 | 5 |
| A11 | Keyboard selection uses caret browsing (F7) with ⇧ + arrows | P§8 | 5 (see T3) |

### 2.7 Messages and timing — M

| ID | Rule | Source | Phase |
|---|---|---|---|
| M1 | Messages queue bottom-centre, one at a time, newest first; non-modal; announced to screen readers | P§21, S2 rule 5, Screen 12 | 1 |
| M2 | Timers are 10 s and pause while pointed at or focused | P§8, P§15, Screen 12 | 1 |
| M3 | Dismissed messages stay reachable under ⌘K › Recently closed; Undo also works with ⌘Z | P§8, Screens 12, 16 | 1, 3, 5 |
| M4 | Nothing that offers an action disappears on a timer the reader can’t control | P§15 | 1 |

### 2.8 Shortcuts — K

| ID | Action | macOS | Windows / Linux | Source |
|---|---|---|---|---|
| K1 | Next / previous chapter | ⌥↓ ⌥↑ (also ] [) | Alt+↓ Alt+↑ (also ] [) | P§12 |
| K2 | Search in book · next / previous result | ⌘F · ⌘G ⇧⌘G | Ctrl+F · F3 ⇧F3 | P§12 |
| K3 | Contents: one command, listed as “Go to chapter…” in ⌘K, that opens the Contents tab focused on the current chapter (C2) | ⌘T | Ctrl+T | P§12, S2, Screen 16, C2 |
| K4 | Go to location | ⌘J | Ctrl+J | P§12 |
| K5 | Highlights and notes panel | ⇧⌘A | Ctrl+Shift+A | P§12, S2 |
| K6 | Highlight / note on selection | ⇧⌘H / ⇧⌘N (also H / N) | Ctrl+Shift+H / N (also H / N) | P§12 |
| K7 | Into the selection bar · caret browsing | F6 · F7 | F6 · F7 | P§8, P§12 |
| K8 | Text size larger / smaller / reset | ⌘+ ⌘− ⌘0 | Ctrl + − 0 | P§12 |
| K9 | Command palette · shortcut cheat sheet | ⌘K · ? | Ctrl+K · ? | P§12 |
| K10 | Back after a jump | ⌘[ | Alt+← | P§12 |
| K11 | Library · Open | ⌘L · ⌘O | Ctrl+L · Ctrl+O | P§12, P§2 |
| K12 | Full screen | ⌃⌘F | F11 | P§12 |
| K13 | Undo | ⌘Z | Ctrl+Z | P§8 |
| K14 | Close layer / hide chrome | Esc | Esc | P§12 |
| K15 | Safe mode | hold ⇧ at launch | hold Shift at launch | P§19 |

Shortcut rules (P§12):

- Single-key shortcuts (H, N, [, ]) act only when the page has focus and no text field is active.
- One setting turns them all off. They also switch off automatically while a screen reader runs (see T6).
- Shortcuts bind to physical key positions (see T7).
- Every shortcut is shown next to its command in ⌘K and on the `?` cheat sheet.
- Remapping is deferred, and extensions can’t claim core shortcuts. Extension commands have no shortcuts in the MVP; they get them when remapping arrives (C4).

### 2.9 Visual system and motion — V

| ID | Rule | Source | Phase |
|---|---|---|---|
| V1 | Theme tokens: **Paper** ground #F7F3EC, ink #22201C (14.7:1), secondary #6B655C (5.2:1), accent #8C4A2F (6.0:1), hairline #E4DDD1 · **Sepia** #F1E6D2, #3B2F22 (10.5:1), #6E5E4A (5.1:1), #8C4A2F (5.4:1), #E2D4BC · **Night** #1B1A18, #D9D3C7 (11.7:1), #9A9387 (5.7:1), #D39A73 (7.1:1), #2E2C29. Auto follows the system between Paper and Night | S4, Screen 10 | 1 |
| V2 | Night chrome keeps the page colour with a #3A3733 hairline; the selection bar inverts to a light surface (13.6:1 against the page) | Screen 14 | 2, 5 |
| V3 | UI type: platform face (SF Pro, Segoe UI Variable, desktop sans on Linux); sizes 12 / 14 / 17 px only; weights 400 / 500 / 600. Reading: book font or Literata. See T5 | P§13, S4 | 1 |
| V4 | Spacing 4, 8, 12, 16, 24, 32, 48, 64. Radius 6 controls, 10 popovers and cards, 14 sheets, round for dots and chips | P§13, S4 | 1 |
| V5 | 1 px hairlines at ~10% ink only where a surface meets the page; buttons have no borders. Shadows on popovers and floating panels only. Popovers and docked panels have no scrim; modals use a 20% scrim of the page colour | P§13 | 1 |
| V6 | Icons: 1.6 px outline strokes on a 24 px grid, filled only when active. Hover: 6% ink wash. Disabled: 40% in menus only | P§13, S4 | 1 |
| V7 | Focus: 2 px accent ring with 2 px offset on :focus-visible, never removed. Selected tabs, segments, swatches and switches also carry a ≥ 3:1 border or outline | P§13, P§15 | 1 |
| V8 | Motion: chrome 160 ms in / 220 ms out (fade + 4 px slide) · Navigator 220 ms slide, column recentres by transform and reflows afterwards · popover 140 ms from 98% · selection bar 100 ms fade · page turn by key, wheel or trackpad 0 ms (optional 120 ms crossfade in Preferences) · swipe tracks the finger, 180 ms settle · jump: instant + 1.2 s pulse · theme 200 ms crossfade · library → book 240 ms cover grow | P§14, S4 | 1–6 |
| V9 | Nothing in the reader exceeds 250 ms. Under reduced motion everything becomes a fade of ≤ 100 ms. No motion during key-repeat turns or for page turns while a screen reader runs | P§14 | 1, 2 |

### 2.10 Accessibility — X

| ID | Rule | Source | Phase |
|---|---|---|---|
| X1 | Contrast: body text ≥ 7:1 in every theme; secondary UI text ≥ 4.5:1; highlighted text ≥ 9:1; meaningful non-text marks (highlight underlines, search outlines, switch borders, focus rings) ≥ 3:1 | P§15 | 1, 5 |
| X2 | Focus order: top bar → Navigator → page → bottom bar. Nothing is ever focused while invisible | P§15 | 1, 2 |
| X3 | Pagination is visual only: assistive tech reads the chapter continuously, and the visible page follows the reading position. Page-turn announcements are brief and can be turned off | P§15 | 0 (Spike C), 2 |
| X4 | Icon buttons have labels and tooltips; hit areas are 44 px, extending past the visible icon | P§15 | 1 |
| X5 | User text spacing (WCAG 1.4.12) triggers re-pagination, not clipping | P§15 | 2 |
| X6 | At 200–400% zoom the layout uses its narrow form: one column, floating panels, no horizontal UI scrolling | P§15 | 2, 8 |
| X7 | Windows high-contrast mode is honoured; highlights fall back to system highlight colours | P§15, P§16 | 1, 8 |
| X8 | UI text a reader must read or act on is ≥ 12 px; decorative cover lettering is hidden from assistive tech | P§13 | 1, 6 |

### 2.11 EPUB handling, library and failures — E

| ID | Rule | Source | Phase |
|---|---|---|---|
| E1 | EPUB 2 and 3: navigation document first, NCX as fallback; EPUB 3 landmarks, page-list and noteref semantics used when present; media overlays ignored | P§17 | 2, 3 |
| E2 | Basic fixed layout: pages scaled to fit, spreads per `rendition:spread`, zoom by pinch and ⌘+; typography controls disabled with one line of explanation in Aa | P§17 | 2, 6 |
| E3 | Corrupted book: validate on import, tolerate damaged entries. Error card: “Part of this book couldn’t be opened · N of M chapters are damaged” with Read anyway / Show file / Remove | P§22, Screen 12 | 1, 3 |
| E4 | Missing metadata: title from the first heading or the file name; generated typographic cover with a tint derived from the title. Metadata is read-only (Q4) | P§10, P§22 | 1 |
| E5 | Save failure: write-ahead log, atomic writes; “Couldn’t save notes to disk · Retry”; nothing discarded | P§22, Screen 12 | 1 |
| E6 | Library: Continue reading (the current book large, the next two small, last-opened time); All books cover grid with count. Tile: cover, title (2 lines), author, progress bar with %, or “New” / “Finished”. Header: search (title and author), sort (Recent, Title, Author), Open… | P§10, Screen 01 | 6 |
| E7 | Item menu: Book info, Show in Finder / Show in Explorer / Show in Files (the platform’s label), Remove. The damaged-book card keeps “Show file” (C6) | P§10, S1, C6 | 6 |
| E8 | One click opens a book at its saved position; “Resume reading” is the default button, so Return opens the current book from launch. Opening an .epub from the OS skips the library | P§10 | 6 |
| E9 | Empty library: “Your library is empty”, drop target, Open a book… ⌘O, “Books are copied into your library and stay on this device.” | Screen 12 | 1, 6 |
| E10 | Book info (sheet, modal) shows metadata read-only and EPUB accessibility metadata | S1, P§15, Q4 | 6 |

### 2.12 Extensions — P

| ID | Rule | Source | Phase |
|---|---|---|---|
| P1 | Manifest: `id`, `version`, `engines.linen` (semver range), `activation` events, `contributes` (`commands`, `selectionActions` with a `when` clause, `navigatorTabs`, themes, exporters), `permissions` | P§18 | 7 |
| P2 | Host API v1 is async, uses structured messages, follows semver and is checked against the manifest on every call. It includes `commands.register`, `selection.addAction`, `navigator.addTab` and `themes.contribute` (no permission needed); `book.selection()` needs `book.selection`; `book.text(range)` needs `book.text`; `annotations.list` / `on` need `annotations.read`; `net.fetch(url)` needs `network:<host>`; `files.save(dialog)` needs `files.export` | S5 | 7 |
| P3 | Permissions and consent. **None needed:** commands, themes, UI slots and 10 MB private storage. **At install:** `book.metadata`, `book.selection` (only when invoked), `library.read`, `network:<host>` (wildcards strongly warned). **At install, highlighted:** `book.text`, `annotations.write`. **Each use, through an OS dialog:** `files.import` / `.export`. **At install, strongly warned:** `background` (1.1, with session events; C3) | P§19, C3 | 7 |
| P4 | Each extension runs in its own sandboxed Worker with no DOM, and is activated lazily and unloaded when idle. Its UI renders in a sandboxed frame with no same-origin access and a locked CSP, styled by Linen’s style kit | P§18, P§19, S5 | 7 |
| P5 | Lifecycle: install from file → review permissions → enabled → activated on demand → unloaded when idle → update (new permissions ask again) → disable or remove (asks before deleting the extension’s data). The core supports the current and previous major API version; incompatible extensions are disabled at load with a reason | P§19 | 7 |
| P6 | Watchdog: 2 s timeout for UI contributions, 10 s for work; CPU and memory budgets; 3 crashes in 10 min → suspended. The slot shows one quiet line (“Dictionary stopped responding · Restart”); stuck selection actions are marked “Not responding” in place; reading carries on | P§19, S5, Screens 11, 12 | 7 |
| P7 | Safe mode: hold ⇧ at launch, or “Restart without extensions” in Preferences | P§19, Screen 11 | 7 |
| P8 | Extensions can’t inject DOM or CSS into the page or chrome, draw over text, add toolbar buttons, bind core shortcuts, notify while you read, or modify other extensions. The top bar has no extension slot; an extension can be pinned in the ⋯ menu | P§18, S5 | 7 |
| P9 | Themes are declarative token packs (JSON, no code, no permissions), listed with the built-in themes | P§18, S5, Screen 11 | 7 |
| P10 | Core selection actions render immediately; extension actions are appended when ready and never block the bar | P§8, Screen 12 | 5, 7 |

## 3. Technical decisions

Each decision has a recommendation. §5 Phase 0 lists the spike that confirms it and the pass criteria.

| Decision | Recommendation | Alternatives | Confirmed by |
|---|---|---|---|
| App shell | **Tauri 2** (Rust core, system WebView). Small and fast, which fits “lightweight”. It has an iOS/Android path for the tablet release, and it matches the design’s own implementation note (P§16). | Electron: one Chromium engine everywhere, but about 10× larger. Switching after Phase 0 means rewriting the Rust core’s Tauri commands, the rusqlite store and the native bridges as Node code: roughly an extra M. | Spikes A, E; Q3 |
| EPUB engine | **foliate-js** (claimed MIT, recheck when pinning). It covers pagination, CFI, search, highlight overlay, footnotes and fixed layout. Its API isn’t declared stable, so pin a version and wrap it behind our own `ReaderEngine` interface. | Readium ts-toolkit navigator; epub.js (ageing) | Spikes A, D |
| UI framework | **TypeScript + Svelte 5** | React, Solid | Q3, decided |
| Persistence | **SQLite** in the Rust core (`rusqlite`, WAL, atomic transactions; E5), behind typed Tauri commands. Book files are copied into the app data folder (Screen 12). | IndexedDB in the WebView | Spike F |
| Search | In-worker scan over each chapter’s normalised plain text, cached to disk per book (F8). Current chapter first, results streamed. | SQLite FTS5 trigram: substring matching, but the index is about 3× the text and CJK/diacritic folding still needs our own normalisation. Revisit for library-wide search. | Spike F |
| Book content isolation | A custom `book://` protocol from the Rust core with a strict CSP, rendered in iframes that can’t run script or reach IPC (§7.1). The CSS sanitiser (L14) runs in the core before content is served. | Blob URLs in the app origin (rejected: that origin holds IPC) | Spike E |
| Reader state | An explicit state machine for the S2 lanes (S1–S8), plus the message queue (M1–M4) and location history (N1) as separate services | Ad-hoc component state | Built first in Phase 1 |
| Commands | One **command registry** is the source for ⌘K, the macOS menu bar, the Windows/Linux ⋯ app menu (P§16), the `?` cheat sheet, shortcut hints and extension commands | Hand-maintained menus per platform | Built in Phase 1 |
| Annotation format | Internal rows (§4) serialise losslessly to W3C Web Annotation JSON with a CFI selector and a TextQuoteSelector (A9). Exporters and the extension API use that JSON. | Custom JSON | Phase 5 round-trip test |

## 4. Architecture

```
┌──────────────────────────── WebView (TypeScript) ───────────────────────────┐
│ UI shell (Svelte): Library · Reader chrome · Navigator · Popovers ·         │
│   Selection bar · Note card/sheet · Peek · Image view · ⌘K · Messages ·     │
│   Preferences window · Book info                                            │
│ Reader state machine (S2 lanes) · Message queue · Location history          │
│ Command registry → ⌘K, menus, cheat sheet · Input router (keys/wheel/       │
│   pointer/touch; single-key gating)                                         │
│ ReaderEngine adapter ──► foliate-js (paginator, CFI, overlayer, footnotes)  │
│   └─ book iframes: no script, no IPC, CSP from the book:// protocol         │
│ Search worker · Annotation anchoring (CFI + TextQuote, fuzzy re-anchor)     │
│ Extension host: 1 Worker per extension (no direct network) + sandboxed      │
│   iframes for UI; all network and data access brokered by the host          │
└───────────────▲─────────────────────────── typed IPC (Tauri commands) ──────┘
                │   capability allow-list: main and Preferences windows only
┌───────────────┴──────────────── Rust core ──────────────────────────────────┐
│ Library service (import, hash, metadata, covers) · EPUB zip streaming       │
│   with entry/size/path limits · CSS sanitiser · font deobfuscation ·        │
│   book:// protocol (CSP headers)                                            │
│ Store (SQLite, WAL): books, book_damage, positions, book_settings,          │
│   annotations, settings, extensions, extension_storage · search-text cache  │
│ Native bridges: file associations and OS open events, menu bar, gesture     │
│   phases (T1), screen-reader state (T6), Dock/taskbar auto-hide (T8)        │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Data model.** First cut, provisional until B3 and B4 are decided; both must be decided before Phase 1 exits.

- `books`: id (uuid), content_hash, package_identifier (OPF `unique-identifier`), file_path, title, authors, language, page_direction, writing_mode, layout (reflowable or fixed), has_page_list, a11y_metadata (JSON), cover_path, generated_cover_tint, added_at, opened_at, finished_at, replaced_at.
- `book_damage`: book_id, item_href, error_kind. Feeds E3 and the Contents markers.
- `positions`: book_id, cfi, fraction, updated_at.
- `book_settings`: book_id, layout_mode, navigator_docked (S13).
- `annotations`: id, book_id, anchored_content_hash, color, cfi_range, quote_exact, quote_prefix, quote_suffix, note, created_at, updated_at, deleted_at (soft delete, for Undo), anchor_status.
- `settings`: key, value. Covers global Aa values, single-key shortcuts, page-turn crossfade and page-turn announcements.
- `extensions`: id, version, enabled, granted_permissions, installed_at, crash_log (for P6).
- `extension_storage`: ext_id, key, value, with a 10 MB quota (P3).
- Reading-pace data for “N min left in chapter”: shape decided with B2.

`package_identifier` plus `content_hash` let the importer recognise a changed file of the same book. `anchored_content_hash` lets anchoring say “The book file changed after this was saved” (Screen 08). UUIDs and timestamps are used from the start so sync can come later without migrating identity.

**Design tokens.** S4 now defines every theme token (V1), plus type, spacing, radii, elevation and motion (V3–V8). They go into one `tokens.ts` with CSS custom properties per theme, along with the highlight tint and underline pairs and the forced-colours rules. Components read tokens and contain no hex values. Extension theme packs (P9) supply the same token set and are validated against X1 before they appear in the theme list (prov.).

## 5. Phases

Relative size: S ≈ days, M ≈ 1–2 weeks, L ≈ 3+ weeks, for one engineer. The order follows dependencies.

- Each phase ends with a **Done when** list. A phase isn’t complete until every item is met and the §6 gates pass.
- Where a phase needs a later phase’s work, it ships a named stub, which the later phase removes.
- Every Done-when item that asserts a rule names its rule ID.

### Phase 0 — Spikes and skeleton (M–L)

**Before starting:**
- Minimum OS versions (D7, decided): macOS 13+, Windows 10 22H2+ with evergreen WebView2, Ubuntu 22.04+ (WebKitGTK 4.1). Record the exact WebView version each spike ran on.
- Reference machines are named in `docs/spikes/reference-machines.md`: one mid-range machine per OS, plus the oldest supported macOS.
- Test corpus assembled (§6.2).

**Work:**
- **Design text export.** Add a script that unpacks the three design bundles into `docs/design/text/` (one Markdown file per board, same names as the boards), and commit the output. Reviewers check rule citations against it, and future design revisions show up as text diffs. CI fails if the export is stale.
- Tauri 2 app on all three OSes; CI builds; lint, format and type-check.
- Each spike writes `docs/spikes/<letter>-<name>.md` with its method, raw numbers and a pass/fail verdict per criterion.

Pass criteria and fallbacks were approved on 2026-09-24. Changing a threshold needs owner sign-off, recorded in the spike report.

| Spike | Pass criteria (on every reference machine unless stated) | If it fails |
|---|---|---|
| **A — rendering parity** | foliate-js pagination in WKWebView, WebView2 and WebKitGTK with L1–L3. On 20 fixed corpus chapters at 3 font sizes, the page count per chapter differs by ≤ 2% between engines. No clipped or split lines at page boundaries. Screenshots differ only in font rasterisation. | WebKitGTK only: ship Linux later, or evaluate Electron for Linux alone. Any engine: evaluate Readium, then Electron. |
| **B — input** | Can the WebView tell trackpad momentum from a deliberate swipe (T1)? Pass: over 50 scripted and 50 manual gestures, one turn per gesture for both axes (I2, I3), with no extra turns from momentum (I5). Also record whether the activating click is distinguishable (I11). Otherwise prototype the native bridge (NSEvent phases on macOS, precision-touchpad data on Windows) and meet the same bar. | Keep wheel rules only (I1) and record the degraded trackpad behaviour as a known limitation, for owner sign-off. |
| **C — accessibility** | VoiceOver and NVDA read continuously inside the paginated iframe across at least 3 page boundaries. Record whether their reading position is observable, so the visual page can follow it (X3, T2). | Continuous reading works in Scroll mode; Pages mode offers “Read from here”, which switches to Scroll for the session. Recorded as a design change for owner approval. |
| **D — engine fidelity** | Moby-Dick opens to the first page in < 500 ms; a 100 MB corpus book in < 1.5 s. 200 random CFIs round-trip to the same text after font size 16 → 24 → 16 px and resize 1280 → 800 → 1280 px. 100 highlights stay on their exact text after the same reflows. Page turn < 16 ms at p95. | Evaluate Readium ts-toolkit against the same criteria before building anything in Phase 2. |
| **E — content isolation** | foliate-js renders, paginates and overlays highlights with book iframes that can’t run script and have the §7.1 CSP. A hostile test EPUB fails to: run script (inline, `onload`, `javascript:` links, SVG script); call any Tauri command; load any remote resource; read another book’s files; escape the library folder via zip paths; or use `position: fixed` over the chrome (L14). Verified on all three WebViews. | Blocks Phase 2. If foliate-js needs script in the frame, isolate book frames in a separate origin that has no IPC capability, and re-run. |
| **F — persistence and search** | A progress write with WAL on takes < 5 ms at p95. Killing the process during 1,000 writes loses at most the last uncommitted write, and the database opens cleanly. Searching all 135 chapters of Moby-Dick from the cache takes < 300 ms. Diacritic folding (é → e), CJK matching and quoted phrases (F2) work on corpus samples. | Persistence: revisit IndexedDB or batching. Search: move the scan to FTS5 trigram and re-run. |

**Done when:**
- All six spike reports exist with verdicts.
- `docs/spikes/decision.md` records go or no-go for Tauri + foliate-js and names the fallback taken for each failed criterion.
- The design text export is committed and checked in CI.
- The skeleton builds and passes lint and type-check in CI on all three OSes.

### Phase 1 — Foundation (M–L)

**Before starting:** the spec gate (§1.2) has passed. B3, B7, D3, D5 and T5 are decided.

- **Tokens and themes (V1, V3–V7):** Paper, Sepia, Night, and Auto following the system; forced-colours styles (X7); reduced-motion handling (V9).
- **Base components:** icon button, button, segmented control, switch, tabs, popover, sheet, modal with page-colour scrim, message, `kbd` hint. All keyboard-accessible, with 44 px hit areas (X4) and the focus ring (V7).
- **Store:** SQLite with WAL, versioned migrations, typed IPC. The Tauri capability allow-list is limited to the app’s own windows.
- **Import pipeline:** OS file open, drag-drop and ⌘O → validate with the zip limits (§7.1) → hash → duplicate and replaced-file check (per B3) → copy → extract metadata and cover. Missing metadata and covers follow E4. Unopenable or partly damaged files follow E3: damaged items are recorded in `book_damage`.
- **Write-failure path (E5):** any failed store write raises “Couldn’t save notes to disk · Retry”. Retry re-sends the pending write; nothing is dropped silently.
- **Reader state machine (S1–S8)**, message queue (M1–M4) and location-history service (N1), each unit-tested without UI.
- **Command registry and shortcut layer (K1–K15):**
  - Physical key positions (T7).
  - Single-key gating: page focus, no active text field, the user setting, and auto-off while a screen reader runs (T6).
  - macOS menu bar and the Windows/Linux ⋯ app menu generated from the registry. Items that later phases haven’t built yet are hidden.
- **Empty library state (E9).** The full library UI comes in Phase 6. Until then the stub shows this state or a plain list.

**Done when:**
- Unit tests cover every transition and rule in S1–S8 and M1–M4, and the history push and pop in N1.
- Every theme meets V1 and X1 in automated contrast checks.
- axe reports no serious or critical violations on the component gallery.
- Importing each corpus file gives the expected result: imported; imported with damage records; rejected with the E3 card; or rejected as hostile.
- Duplicate and replaced-file imports follow B3.
- A simulated disk-full write shows the Retry message, and Retry succeeds once space returns.
- The migration test (§6.3) passes.
- Single-key shortcuts are inert in a text field, when switched off, and with a screen reader running (on each OS where T6 found a signal).
- B1, B4, B9 and B10 are decided.

### Phase 2 — Reader core (L+) · Screens 02, 03, 10, 14

**Before starting:** Spikes A, D and E passed, or their fallbacks were taken. B2, B8, B11, B12 and T3 are decided. The G8 designs for Scroll mode, two-page spread, fixed layout and the loading state exist.

- **ReaderEngine adapter:** open a book (N3 for new books), render, paginate, CFI location, and reflow around an anchor (L10). Book frames follow §7.1.
- **Canvas (L1–L17):**
  - Measure, margins, page height and line height.
  - Publisher/comfort split and the CSS sanitiser.
  - Fonts, with deobfuscation and fallback.
  - Images, tables and code.
  - Virtual sections and “≈” locations.
  - Load-on-demand for large books.
  - User text spacing (X5).
- **Fixed layout (E2):** scale to fit, spreads, ⌘+ and pinch zoom (I17).
- **Immersive state and location line:** chapter, plus “N min left in chapter” per B2 (the reading-pace store lands here). The location line hides below 480 px of height (L9). The pointer hides after 2 s idle (S12).
- **Chrome (S9–S11, S15):** edge reveal with OS edge exclusions (T8), centre tap, Tab; auto-hide; top and bottom bars with their eight controls. Buttons whose features arrive later are hidden until then.
- **Input router (I1–I15):**
  - Keys, including the Space-on-button rule.
  - Wheel, and trackpad per the Spike B outcome.
  - Margin clicks, including the activating-click rule.
  - Touch zones with tap priority; finger-tracked swipes.
  - Per-event pointer type; RTL mirroring.
  - At most one queued turn; the multi-page Back chip.
- **Scroll mode (I9, per B8); two-page spread and breakpoints (L8, per B12).**
- **Progress persistence:** 1 s debounce, plus on blur, quit and leaving for the library (N5). Resume message (N4). Back history and the Back chip (N1, N2).
- **Screen-reader sync (X3)** per the Spike C outcome; page-turn announcements with an off switch.
- **Night chrome (V2).** The selection-bar inversion stays stubbed until Phase 5.
- **Motion (V8, V9)** for chrome and page turns.

**Done when:**
- Unit tests assert every I-rule threshold and L1–L10.
- Scripted end-to-end tests (§6.1) turn pages by key, wheel, click and touch zone in LTR and RTL books, and confirm that clicks inside the column and the activating click never turn a page.
- Quitting mid-chapter and reopening restores the same CFI; a new book opens at bodymatter.
- Resize, font change and text-spacing overrides keep the reading position (the Spike D check re-run in the app).
- A fixed-layout corpus book scales, spreads and zooms.
- A 1 MB+ chapter opens within the open-book budget and shows “≈” until pagination completes.
- The §6.4 budgets that apply from Phase 2 hold on the reference machines.
- Hostile corpus EPUBs still fail as in Spike E.
- Visual baselines for Screens 02, 03, 10 and 14 (chrome) are approved (§6.1).

### Phase 3 — Navigation (M–L) · Screens 04, 16, 17

**Before starting:** the G10 designs (cheat sheet) and the image view (G8) exist. B13 is decided.

- **Navigator shell:** Contents · Search · Notes tabs. The header holds Library, the title and progress (S3). It docks from 1100 px and floats below, joining the floating lane (S2, L8). The column recentres by transform, then reflows (V8). ⌘T, ⌘F and ⇧⌘A switch tabs when it is open. The docked state is remembered per book (S13).
- **Contents (N6):** the fallback chain, “You are here”, and damage markers.
- **Chapter keys (K1), and the scrubber with its preview tooltip (N7).**
- **Go to popover (N8):** Percent, Chapter and Print page, with the preview line.
- **Footnote peek (N9); internal and external links (N10); image view (N11).**
- **Command palette UI:**
  - Sections: Recently closed (M3), Reading, Extensions.
  - Fuzzy match, shortcut hints, ↑ ↓ ↵ navigation, `?` for the cheat sheet.
  - The palette suspends reader input (S8).

**Done when:**
- Contents is correct for the corpus (nav, NCX-only and no-TOC books), and damaged chapters show the marker.
- The footnote peek meets every N9 rule in an end-to-end test, including focus return and placement near the page foot.
- Go to lands on the stated location for percent, chapter and page-list targets, and Back returns.
- Every registry command is reachable from ⌘K, the macOS menu bar and the ⋯ menu, and shows its shortcut; the cheat sheet lists every shortcut.
- The S2 Esc order holds with the Navigator docked plus a floating layer open.
- Visual baselines for Screens 04, 16 and 17 are approved.

### Phase 4 — Search (M) · Screen 05

**Before starting:** B6 is decided.

- **Text extraction and normalisation in a worker:** case, diacritics, CJK, quoted phrases. Offset maps lead back to DOM ranges, and the index is persisted (F8).
- **Incremental search (F1–F4):** entry points, debounce and minimum length, current chapter first, streamed and grouped results, the “Searching N of M” and “so far” states, snippets.
- **Marks in the page (F5), navigation keys (F6), and the Esc-return rule (F7).**

**Done when:**
- A golden-result test gives the expected hit count and positions for each corpus query set, including diacritic, CJK and quoted-phrase queries.
- Every result lands on its match in the page, and marks meet the F5 contrast values.
- The Esc-return rule (F7) passes an end-to-end test.
- Search timing holds the Spike F budget in the app.
- The Screen 05 visual baseline is approved.

### Phase 5 — Selection and annotation (L) · Screens 06, 07, 08, 14, 15

**Before starting:** B4 and B5 are decided. The G11 design (Re-attach flow) exists.

- **Selection bar (A1–A3, A10, A11):**
  - Placement and flip rules; F6 focus; H and N plus the ⇧⌘ variants.
  - The native context menu with the same actions.
  - Night inversion (V2, which removes the Phase 2 stub).
  - An extension-actions area that stays empty until Phase 7 (P10).
- **Highlights (A4, A5, A7):** overlay rendering with tint plus underline; click versus drag; colour change; delete with Undo (M2, M3, K13).
- **Notes (A6):** the margin card from 1240 px, the bottom sheet below; autosave with announced status; empty-note discard; margin dot; the Phase 1 write-failure path.
- **Anchoring (A9):** CFI plus a text quote, and `anchored_content_hash`. Fuzzy re-anchoring when a replaced file is imported (per B3). The “Couldn’t place” group with Re-attach (A8).
- **Notes tab (A8):** grouped by chapter, filter by colour and text, jump with a 1.2 s pulse and the Back chip. The “Export as Markdown” entry stays hidden until Phase 7.
- **W3C Web Annotation serialisation**, used by export and the extension API.

**Done when:**
- Anchoring tests pass. Highlights survive reflow, and survive re-import of a corpus book edited in known ways (inserted paragraph, changed punctuation, deleted passage). Anything unplaceable lands in “Couldn’t place”, never at a wrong position.
- The B4 edge cases have tests.
- Annotations round-trip through W3C JSON without loss.
- Undo restores the exact annotation, including after the message has timed out, via ⌘K › Recently closed.
- A note typed and then followed by a quit is saved; an empty note is discarded and its highlight kept.
- Highlighted text meets X1 (≥ 9:1) and the underlines meet A4, in every theme.
- Visual baselines for Screens 06, 07, 08, 14 (selection bar) and 15 are approved.

### Phase 6 — Settings and library (M) · Screens 01, 09, 11-shell, 12

**Before starting:** the G2, G3 and G4 designs exist. D2 is decided.

- **Aa popover:**
  - Rows: size (L4 steps), theme, line spacing and layout, each with its scope label (“All books” / “This book”).
  - ⌘+ ⌘− ⌘0 anywhere.
  - The code-and-tables hint (L18) and the fixed-layout explanation (E2).
  - A “More in Settings…” link.
- **Library (E6–E9):**
  - Continue reading, the grid with its count, the tile states, and generated covers.
  - Search and sort; the item menu; Remove (per G4).
  - Return opens the current book; the 240 ms cover-grow transition; the book stays warm (S14).
- **Book info sheet (E10, G3):** read-only.
- **Preferences window shell:** General, Reading, Library, Extensions, Shortcuts, About. Contents per G2, including the single-key shortcut switch, the page-turn crossfade and the page-turn announcements.
- **Spike G — extension network isolation (S)**, so Phase 7 starts on a proven mechanism (§7.2).

**Done when:**
- Aa scope rules hold (size, theme and line spacing change all books; layout changes this book only) and persist across restarts.
- Library search and sort give the expected order on a 500-book fixture.
- Removing a book follows G4, including what happens to its file and annotations.
- The cold-start budget holds with the 500-book fixture.
- The Spike G report passes.
- Visual baselines for Screens 01, 09, the Screen 11 shell and the Screen 12 empty and damaged states are approved.

### Phase 7 — Extensions (L) · Screens 11, 12

**Before starting:** Spike G passed. D4 is decided. The G1 (consent dialog) and G7 (extension surfaces) designs exist.

- **Manifest schema and validation (P1)**; install from file; the consent dialog (G1) with the P3 consent tiers; enable and disable; update re-prompts; uninstall with the prompt to delete data (P5).
- **Extension host (P2, P4):**
  - One Worker per extension with the Spike G isolation.
  - Sandboxed iframes with a locked CSP for Navigator tabs, styled by the style kit.
  - Lazy activation and idle unload.
  - A permission check on every brokered call.
  - Semver compatibility (current and previous major).
- **Watchdog (P6); safe mode and “Restart without extensions” (P7).**
- **Slots:**
  - ⌘K commands.
  - Selection “⋯” actions with `when` clauses (P10).
  - A Navigator tab.
  - Theme packs (P9).
  - The Notes export menu.
  - Read-only annotation events. Session events and the `background` permission wait for 1.1 (C3).
  - Pinning in the top-bar ⋯ menu (P8).
- **Markdown Export** ported to the extension API and listed as “Built-in” (removes the Phase 5 hidden entry).
- **Samples:** a Dictionary extension (a selection action plus a Navigator tab, with network permission) and a Night Owl-style theme pack, to exercise the slots and permissions.
- **Extension API reference** in `docs/extensions/`.

**Done when:**
- A hostile test extension fails to reach any undeclared host, any Tauri command, book text without the permission, another extension’s storage, or more than 10 MB of storage. Verified on all three WebViews.
- The watchdog restarts and suspends a hanging or crashing extension as P6 specifies; the selection bar marks it “Not responding”; reading continues (Screen 12).
- Safe mode starts with every extension off.
- An extension built for an unsupported major version is disabled at load with a reason.
- Markdown Export output matches a golden file.
- Visual baselines for Screen 11 and the Screen 12 extension-failure state are approved.

### Phase 8 — Hardening and release (M–L)

**Before starting:** D1, D3, D5 and D6 are decided.

- **Accessibility pass against D5** (recommended: WCAG 2.2 AA; the design’s own AAA body-contrast target, X1, still applies):
  - Manual VoiceOver, NVDA and Orca runs.
  - Windows high contrast (X7).
  - 200–400% zoom (X6) and user text spacing.
  - Automated axe checks have run in every phase since Phase 1.
- **EPUB edge cases:** vertical writing (I16), RTL, huge books, broken CSS and fonts, and the whole P§22 failure table.
- **Performance budgets** on all reference machines.
- **Backup and restore per D2; uninstall behaviour.**
- **Packaging and distribution:**
  - Code signing and notarisation.
  - File associations: macOS Open With and Dock drop; a Windows Jump List of recent books; Linux MIME registration in the `.desktop` file (P§16).
  - Auto-update per D6.

**Done when:** the release checklist in §6.5 passes on all three OSes.

## 6. Verification

### 6.1 Test layers and what each one proves

| Layer | Runs | Proves | Doesn’t prove |
|---|---|---|---|
| Unit tests (TypeScript and Rust) | every commit | State machine, rule-register thresholds, CFI and anchoring, permission checks, import validation and zip limits, CSS sanitiser | Rendering or real input |
| Frontend integration tests: Playwright on Chromium and WebKit, IPC mocked | every commit | UI flows, focus rules, Esc order | Behaviour of WKWebView, WebView2 or WebKitGTK inside the app; real IPC |
| App end-to-end tests, Windows and Linux: WebDriver via `tauri-driver` | every merge to main | Real app, real IPC, real engine | macOS (`tauri-driver` has no macOS support at time of writing; recheck at Phase 0) |
| macOS app tests | every merge to main (smoke) and every release (full) | Real WKWebView app | Anything not in the script |
| Visual regression | every merge to main | No unreviewed visual change | Behaviour |
| Performance runs on reference machines | weekly, and before each phase closes | §6.4 budgets | — |

**macOS layer.** Phase 0 decides between an accessibility-driven UI driver (for example Appium’s Mac2 driver, unverified for this app) as the smoke suite, and a written manual script if no driver works. Either way, macOS is the main design platform, so every phase’s Done-when items are also checked by hand on macOS.

**Visual baselines.** The mocks can’t be pixel baselines. They contain spec overlays (Screens 06, 13, 14 and 15), template placeholders (Screen 09 `{{fs}}`, Screen 17 bracketed text) and fixed sample content. For each screen:
1. Build the same state from a fixture: Moby-Dick chapter 1 at 1280 × 800 (760 px wide for Screen 15), in the mock’s theme.
2. Compare it with the mock by eye and record the sign-off.
3. Store that screenshot per engine as the baseline.

After that, diffs run against the approved screenshots. Screen 13 is a spec overlay for the tablet release, so it has no baseline; its rules are covered by I12–I15 tests.

### 6.2 Test corpus

- Standard Ebooks (public domain) and the IDPF epub3-samples, including fixed-layout, RTL, vertical-writing, page-list and noteref samples.
- Books with more than 30% code or tables (L18), a chapter over 1 MB (L16), and obfuscated fonts (L15).
- Deliberately broken files: truncated zip, bad OPF, missing items, bad CSS and fonts, missing metadata and cover, no TOC.
- A 100 MB book and a 500-book library fixture.
- **Hostile EPUBs:**
  - inline script, event handlers and `javascript:` links
  - SVG with script
  - remote images, fonts and CSS
  - `position: fixed` overlays
  - zip entries with `..`, absolute paths or symlinks
  - a zip bomb and 100,000-entry archives
  - oversized XML entities

### 6.3 Data-safety tests

- **Migrations:** a fixture database from every schema version ever shipped (before release: from each phase) migrates to the current version with no data loss, checked row by row.
- **Crash consistency:** kill the process during progress, annotation and import writes; the database opens and the last committed state is intact.
- **Write failure:** disk full or a read-only folder raises the Retry message; nothing is dropped silently.

### 6.4 Budgets

Measured on the reference machines with release builds; the median and p95 of 20 runs are recorded.

| Measure | Budget | Starting point | Applies from |
|---|---|---|---|
| Cold start to library | < 1 s | process launch → library first paint, 500-book fixture | Phase 6 |
| Cold start to last book | < 1.5 s | process launch → first page of resumed book | Phase 2 |
| Open a book to first page | < 500 ms, or < 1.5 s for a 100 MB book | click → first page painted | Phase 2 |
| Page turn | < 16 ms p95 (one frame; I6) | input event → next frame painted | Phase 2 |
| Reflow after resize | < 150 ms | end of the 120 ms debounce → page painted | Phase 2 |
| Memory for a large book | < 400 MB | resident memory of all processes after reading 50 pages of the 100 MB book | Phase 2 |
| Reader transitions | ≤ 250 ms (V9) | transition start → end | Phase 1 |

### 6.5 Release checklist

- Every phase’s Done-when list still passes.
- Performance budgets hold.
- The accessibility target is met, with manual runs recorded.
- Hostile corpus results are unchanged.
- The backup and restore round-trip passes.
- Builds are signed and notarised; file associations work on each OS.

### 6.6 Re-verification

- **foliate-js upgrade:** re-run Spike D’s CFI and highlight checks, the full corpus and the visual baselines before merging.
- **Tauri or WebView floor change:** re-run the Spike E hostile corpus and the visual baselines.
- **Design change:** regenerate the text export. Every changed rule-register row is updated in the same change as its tests, and new or changed values need owner sign-off.
- **Schema change:** add a migration fixture.
- CI blocks a merge when unit, integration, end-to-end or visual tests fail.

## 7. Security

### 7.1 Book content (untrusted input)

- **Scripts:** book JavaScript never runs. Book iframes are sandboxed without `allow-scripts`, and the `book://` protocol sends `script-src 'none'`.
- **Network:** `default-src` is limited to `book:` and `data:`; no remote resources.
- **CSS:** the sanitiser strips network references and fixed positioning (L14), so a book can’t overlay the chrome or fake app UI.
- **IPC:** book frames can never call Tauri commands. The capability allow-list covers only the app’s own windows; Spike E verifies this rather than assuming it.
- **Links:** external links open in the system browser after a check that they are `http(s)`.
- **Zip handling:**
  - Reject absolute paths, `..` segments and symlinks.
  - Limits (approved 2026-09-24): ≤ 10,000 entries, ≤ 1 GB total uncompressed, per-entry compression ratio ≤ 100:1. If the corpus run shows a legitimate book exceeding them, changing them needs owner sign-off.
  - Stream entries; never extract to disk outside the book’s own cache folder.
- **XML:** the OPF and navigation parsers reject external entities and cap entity expansion.
- **Fonts:** deobfuscation runs in the core on bounded input; fonts that fail to load or parse fall back silently (L15).

### 7.2 Extensions

Extensions never reach Tauri commands directly; every call is brokered through the host and permission-checked (P2). They have no raw file system, shell or native modules (P§19). Storage is private per extension and capped at 10 MB. Theme packs are data only and are validated before use.

For network access, extension Workers get **no direct network**. The candidate mechanism, which Spike G must prove:
- Each Worker is served from its own origin with `connect-src 'none'`.
- Nested Workers are blocked, and `importScripts` is limited to the extension’s own package.
- The host performs `fetch` for declared hosts only (`net.fetch`).

Spike G passes when a test extension can’t reach an undeclared host through `fetch`, XHR, WebSocket, EventSource, `importScripts`, nested Workers or an iframe, on all three WebViews.

## 8. Design → implementation map

| Board | Phase | Main pieces |
|---|---|---|
| S1 Information architecture | 1–7 | App routes (Library, Reader), Preferences window, layer inventory |
| S2 Product state model | 1 | Reader state machine, message queue, history |
| S3 Canvas anatomy | 0, 2 | Layout engine settings, reveal zones, margin click targets, breakpoints |
| S4 Visual system | 1 | `tokens.ts`, theme CSS, motion constants |
| S5 Extension architecture | 7 | Host API v1, extension host, watchdog |
| 01 Library | 6 | LibraryView, BookTile, ContinueReading, GeneratedCover |
| 02 / 03 Reader | 2 | ReaderView, Chrome (TopBar, BottomBar, Scrubber, MarginChevrons), LocationLine |
| 04 Contents | 3 | Navigator, ContentsTab, damage markers |
| 05 Search | 4 | SearchTab, search worker, match overlay |
| 06 / 07 / 15 Selection, notes | 5 | SelectionBar, NoteCard, NoteSheet, HighlightOverlay |
| 08 Notes | 5 (export entry in 7) | NotesTab, anchoring service, CouldntPlace, Re-attach |
| 09 Aa | 6 | ReadingSettingsPopover |
| 10 / 14 Themes, Night | 1–2 (Night selection bar in 5) | tokens, theme switching, Night chrome variants |
| 11 Extensions | 6–7 | Preferences › Extensions, extension host, safe-mode restart |
| 12 Empty and error states | 1, 5, 6, 7 | EmptyLibrary, DamagedBook, write-failure Retry, ExtensionFailure, MessageQueue |
| 13 Tablet | 2 (desktop touch); tablet app after the first release (Q2) | TouchZones, SwipePager |
| 16 ⌘K | 1 (registry), 3 (UI) | CommandRegistry, CommandPalette, CheatSheet, app menus |
| 17 Footnote · Go to | 3 | FootnotePeek, GoToPopover |

## 9. Risks

1. **WebView differences** break pagination parity (worst on WebKitGTK). Mitigation: Spike A with numeric criteria, per-engine visual baselines, and the fallbacks in the Spike A row.
2. **Several design rules need native signals that a WebView doesn’t expose:** trackpad momentum (T1), screen-reader state (T6), physical key labels (T7) and Dock/taskbar auto-hide (T8). Mitigation: spikes and native bridges in the Rust core. Where a signal can’t be obtained, the degraded behaviour is recorded for owner sign-off rather than shipped silently.
3. **Screen reader and pagination sync** may not be observable from inside an iframe. Mitigation: Spike C and its Scroll-mode fallback.
4. **foliate-js has no stable API.** Mitigation: the adapter layer, a pinned version, and the upgrade re-verification rule (§6.6).
5. **Hostile book content** reaching IPC, the file system, the network or the chrome. Mitigation: §7.1, Spike E, and the hostile corpus in CI.
6. **Extension sandbox escape** through IPC or direct network. Mitigation: §7.2 and Spike G.
7. **Spec drift and conflicts** between the proposal, the system boards and the screens. Mitigation: rule IDs with citations, the committed text export, the C-list (§10.2), and tests that name rule IDs.
8. **macOS is the main target but has the weakest automated coverage.** Mitigation: the macOS layer decided in Phase 0, plus manual Done-when checks.
9. **Phase 2 is the largest phase** (the canvas, input, fixed layout and screen-reader sync all land there). Mitigation: its Done-when list is split by rule group, so progress is measurable. If it slips, fixed layout (E2) can move to Phase 8 with owner sign-off; it is the least-used MVP path.

## 10. Open decisions and gaps

Every item has a deadline, set by the phase that consumes it. If an item isn’t decided by its deadline, that phase doesn’t start (“Before starting” in §5), unless the item lists a default. Owner roles: **P** = product/design owner, **T** = tech lead. IDs from revision C are kept so earlier references stay valid.

### 10.1 Questions for you (decided)

| # | Question | Decision |
|---|---|---|
| Q1 | Is the written proposal and system spec approved along with the 17 screens? | Yes, 2026-09-24 |
| Q2 | Is tablet in the first release? | No: desktop only, tablet next (2026-09-24) |
| Q3 | UI framework and shell? | Svelte 5 and Tauri 2, subject to the Phase 0 go/no-go (2026-09-24) |
| Q4 | Can users edit metadata? | No; metadata is read-only (2026-09-24). This supersedes the P§22 failure-table row “Edit title and author in Book info”. |

### 10.2 Conflicts between design sources

Found while writing revision D. **All six were decided on 2026-09-24: the owner accepted the recommended defaults.** The rule register and phases already reflect them.

| # | Conflict | Decision (accepted default) | Owner | Decided |
|---|---|---|---|---|
| C1 | The resume chip is a “four-second chip” (P§2), but it is drawn as a queued message (Screen 12), and P§15 says nothing that offers an action may vanish on a timer the reader can’t control; messages last 10 s. | 10 s, pausing on hover or focus, like every other message (M2, M4) | P | 2026-09-24 |
| C2 | ⌘T opens the Contents tab (P§6, P§12, S2), but Screen 16 labels ⌘T “Go to chapter…”. | One command: “Go to chapter…” in ⌘K opens the Contents tab with focus on the current chapter; ⌘T runs it | P | 2026-09-24 |
| C3 | S5 and Screen 11 (Reading Time, with the `background` permission) show session events in the MVP, but P§18 schedules reading-statistics session events for 1.1. | MVP ships annotation events only. Session events and the `background` permission come in 1.1; Reading Time on Screen 11 is illustrative. | P | 2026-09-24 |
| C4 | P§18 gives extension commands “user-assigned shortcuts” in the MVP, but P§12 defers remapping and Screen 16 shows extension commands with “no shortcut”. | No shortcuts for extension commands in the MVP; they arrive with remapping | P | 2026-09-24 |
| C5 | The P§22 failure table offers a per-book “Simplify styles” recovery, but P§9 and P§17 put the Publisher styles setting in 1.1, and no screen shows it. | Both are 1.1; the MVP relies on the sanitiser and comfort overrides (L13, L14) | P | 2026-09-24 |
| C6 | P§10 says the item menu offers “Show in Finder / Explorer”; S1 says “Show file”; Screen 12’s damaged-book card says “Show file”. | The platform label in menus (“Show in Finder” / “Show in Explorer” / “Show in Files”), and “Show file” on the damaged-book card | P | 2026-09-24 |

### 10.3 Behaviour that is unspecified or ambiguous

| # | Item | Decide by |
|---|---|---|
| B1 | What a “page” number means. “Back to page 43” and “Back to page 7” (Screens 08, 12) use dynamic page numbers that change with font size. Per chapter or per book, and how they relate to the print page-list (N8). | Phase 1 exit |
| B2 | The “N min left in chapter” algorithm (“from the reader’s own pace”, P§6): the reading-pace model and what data it stores. | Phase 2 start |
| B3 | Import semantics. The design fixes “copied into your library” (Screen 12). Still open: library folder location, duplicate detection (content hash vs `package_identifier`), book updates and the re-anchoring flow. | Phase 1 start |
| B4 | Highlight edge cases: overlapping or nested highlights, highlights spanning chapters, maximum length, what Copy includes. | Phase 1 exit (schema) |
| B5 | Undo scope: only deletions (as drawn), or also colour changes and note edits? | Phase 5 start |
| B6 | Search limits: result caps for very common words. Regex and whole-word matching are deferred (P§7); decide only whether the UI reserves space for them. | Phase 4 start |
| B7 | Multiple windows or books open at once. The design shows one reader; recommend one book per window, with a single window in the MVP. | Phase 1 start |
| B8 | Scroll mode: continuous across chapters or per chapter; what the location line and scrubber show. | Phase 2 start |
| B9 | DRM-protected files: detection and messaging (DRM is out of scope, but such files will be opened). | Phase 1 exit |
| B10 | UI localisation. P§17 says the UI stays in the app’s language; decide whether strings are externalised from the start and whether RTL UI mirroring is planned. | Phase 1 exit |
| B11 | What Esc from the Navigator returns to. S2 draws Navigator → Chrome, but chrome and Navigator alternate (S3). Recommend: restore the chrome state from before the Navigator opened. | Phase 2 start |
| B12 | Whether the two-page spread breakpoint (1480 px) uses the window width or the width left after a docked Navigator (320 px). Recommend the remaining width, applied after the Navigator slide. | Phase 2 start |
| B13 | The lifetime of Back history and ⌘K › Recently closed: per session or persisted, and maximum depth. Recommend per book, in memory, with the last 50 jumps (prov.); Recently closed for the session, last 20 (prov.). | Phase 3 start |

### 10.4 Missing screens or states (design needed by the named phase)

- **G1** Extension install and permission consent dialog, including the re-prompt when an update asks for more (Phase 7). Screen 11 supplies the plain-language permission strings.
- **G2** Preferences sections other than Extensions: General, Reading, Library, Shortcuts, About (Phase 6).
- **G3** Book info sheet: read-only (Q4), with EPUB accessibility metadata (E10) (Phase 6).
- **G4** Library states: sort menu open, item menu, remove confirmation or Undo (and whether Remove deletes the copied file and annotations), search with no results, importing several or large files, duplicate import, re-importing an updated file. Due Phase 6; the import-related states by Phase 1 exit.
- **G5** Navigator, popovers, ⌘K, sheets, library and Preferences in Sepia and Night. S4 now supplies every token, so this is a review of derived designs rather than new design work (Phase 2 start for reader panels, Phase 6 for the rest).
- **G6** Windows and Linux window chrome and the ⋯ app menu. P§16 describes them, but every mock uses macOS (Phase 1 exit).
- **G7** Extension surfaces: an extension’s Navigator tab, pinned items in the top-bar ⋯ menu, and the “Built-in” and suspended states in context. The dictionary peek is 1.1 and not needed (Phase 7).
- **G8** Reading variants: Scroll mode, two-page spread, fixed-layout zoom and pan, RTL and vertical writing, image view, the loading state for large books, and “≈” locations (Phase 2 start; image view by Phase 3 start).
- **G9** Tablet beyond the tap-zone overlay: not needed for the first release (Q2).
- **G10** The `?` cheat sheet (Phase 3 start).
- **G11** The “Couldn’t place” Re-attach flow: how the reader picks the new passage (Phase 5 start).
- **G12** The Aa popover’s code-and-tables hint (L18) and fixed-layout explanation (E2) (Phase 6 start).

### 10.5 Technical assumptions a WebView may not satisfy

| # | Item | Resolved by |
|---|---|---|
| T1 | Trackpad momentum and gesture-phase detection (I2, I3, I5) | Spike B |
| T2 | “The visible page follows the screen reader” (X3) | Spike C |
| T3 | Caret browsing with F7 (A11): Chromium has it; WKWebView and WebKitGTK may need our own implementation | Phase 2 start |
| T4 | iPad edit-menu integration and Apple Pencil need native code on Tauri mobile | Not needed for the first release (Q2) |
| T5 | Platform UI font vs Instrument Sans (the stand-in used in all screens); licence check for bundled fonts (Literata is OFL) | Phase 1 start |
| T6 | Detecting a running screen reader, so single-key shortcuts and motion switch off (K rules, V9). There is no web API for this; it needs native queries (for example the VoiceOver state on macOS, `SPI_GETSCREENREADER` on Windows, the AT-SPI bus on Linux). Fallback: the Preferences switch only, recorded for sign-off. | Phase 1 start |
| T7 | Physical key binding with correct labels. `KeyboardEvent.code` gives position in every engine, but `navigator.keyboard.getLayoutMap()` for labels is Chromium-only, so WebKit needs a native layout query. | Phase 1 start |
| T8 | Dock and taskbar auto-hide state, and the macOS full-screen menu-bar zone (S11), need native queries | Phase 2 start |

### 10.6 Product and operations decisions

| # | Item | Decide by |
|---|---|---|
| D1 | Privacy: telemetry and crash reporting (opt-in or none). The design promises books “stay on this device” (Screen 12). | Phase 8 start |
| D2 | Backup, export and restore of all user data; uninstall behaviour | Phase 6 start |
| D3 | Distribution channels. The Mac App Store sandbox limits file access and extensions; also Microsoft Store and Flatpak. | Phase 1 start (affects the file-access model); final decision at Phase 8 |
| D4 | Extension package format, signing, API reference, extension localisation. A signed registry comes after the MVP (P§19). | Phase 7 start |
| D5 | Accessibility conformance target (recommend WCAG 2.2 AA, plus the design’s AAA body contrast) | Phase 1 start |
| D6 | Auto-update mechanism and release cadence | Phase 8 start |
| D7 | Minimum OS versions | **Decided 2026-09-24:** macOS 13+, Windows 10 22H2+ (evergreen WebView2), Ubuntu 22.04+ |

## 11. Revision history

- **Revision D, amended (2026-09-24):** the owner accepted the defaults for C1–C6. The rule register (N4, K3, E7, P3, shortcut rules), the Phase 7 slots and the phase entry conditions were updated to match.
- **Revision D (2026-09-24):** rewritten against the approved design files committed in `e0c38fe` (proposal §1–25, system S1–S5, screens 01–17).
  - The spec gate no longer requires transcribing the canvas. Rules cite the committed files, and Phase 0 adds a text export.
  - The rule register was expanded from 18 rows to 137 rules with IDs and exact citations. It adds rules previously missing from the plan:
    - Input: the 80 px trackpad threshold, the multi-page Back chip.
    - Breakpoints: 760 px, and the 64 px reveal zone.
    - Search and selection: the search minimums, the selection-bar placement.
    - Motion and tokens: the full motion table, every S4 token.
    - EPUB handling: font, CSS, long-chapter and large-book handling.
    - Extensions: the manifest and Host API v1, the permission tiers.
  - Fixed an error: ⌘T opens Contents, not a separate “Go to chapter” (see C2).
  - Added design conflicts C1–C6, behaviour gaps B11–B13, missing designs G11–G12, and native-signal assumptions T6–T8.
  - Moved the command registry to Phase 1 (it drives ⌘K, menus and the cheat sheet) and basic fixed layout to Phase 2 (it is MVP scope). Screen 13 no longer gets a visual baseline.
- **Revision C (2026-09-24):** owner approved the spike pass criteria and fallbacks, the zip limits, the minimum OS versions (D7) and the rule that book scripts never run. Owner decided Q1–Q4.
- **Revision B (2026-09-24):** addressed the plan review:
  - Added the spec gate and rule register.
  - Added Spikes D–F, the decision register, Done-when criteria, and the book-content and extension-network security sections.
  - Revised the verification layers.
  - Added stubs for cross-phase dependencies and the re-verification rules.
- **Revision A:** initial draft.
