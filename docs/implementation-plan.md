# Linen (Quiet EPUB Reader) — implementation plan

Status: draft for review · Source of truth: `docs/design/Quiet EPUB Reader.html` (17 approved screens, revision 2)

## 1. Scope and source of truth

The approved file contains **only the 17 screens** (01 Library … 17 Footnote peek · Go to). Behaviour that isn’t visible in a mock comes from the design canvas’s written proposal and system diagrams: reveal rules, input thresholds, the state model, shortcuts, the extension API and permissions. Those boards are **not** in the approved export. This plan treats them as the working spec. Confirm that (see §8, Q1).

MVP scope, as defined by the design:

- EPUB 2 and 3: open and import, reflowable, basic fixed layout.
- Library: Continue reading, cover grid, search, sort, remove.
- Reader: Pages (default) and Scroll modes, Paper / Sepia / Night / Auto themes.
- Navigation: Contents, chapter keys, scrubber, Back history, footnote peek, Go to location.
- Full-text search.
- Selection, four-colour highlights, notes, and a Notes tab.
- Aa popover: size, theme, line spacing, layout.
- Progress persistence.
- Extension infrastructure, with Markdown Export as the first extension.
- Command palette.

Explicitly out of scope: sync, accounts, AI, TTS, statistics, store, DRM, collections, a marketplace, and phone layouts.

## 2. Technical decisions to make first

Each decision below has a recommendation. Phase 0 spikes must confirm it before the rest of the plan depends on it.

| Decision | Recommendation | Alternatives | What the spike must prove |
|---|---|---|---|
| App shell | **Tauri 2** (Rust core, system WebView). Small and fast, fits “lightweight”, and has an iOS/Android path for tablets later. | Electron (one Chromium engine everywhere, but around 10× larger) | CSS multi-column pagination looks the same in WKWebView (macOS), WebView2 (Windows) and WebKitGTK (Linux). If Linux diverges badly, Electron is the fallback. |
| EPUB engine | **foliate-js** (MIT; covers pagination, CFI, search, highlight overlay, footnotes and fixed layout) | Readium ts-toolkit navigator; epub.js (ageing) | Moby-Dick and a 100 MB book open quickly; CFI round-trips survive font-size reflow; highlights stay anchored; page turns render within a frame. Its API is not declared stable, so pin a version and wrap it behind our own `ReaderEngine` interface. |
| UI framework | **TypeScript + Svelte 5** (small runtime, fine-grained updates for a UI that is mostly idle) | React or Solid — pick whatever the team knows best | None; this is a team-preference call. |
| Persistence | **SQLite** in the Rust core (`rusqlite`), exposed through typed Tauri commands. Book files copied into the app data folder. | IndexedDB in the WebView (simpler, but weaker durability and no native backup) | Atomic writes with WAL; progress saves under 5 ms. |
| Search | In-worker scan over each chapter’s normalised plain text, cached to disk per book. Current chapter first, results streamed. | SQLite FTS5 (only prefix matching; needed later for library-wide search) | 135 chapters searched in under 300 ms once cached; diacritic and CJK normalisation works. |
| Reader state | An explicit state machine for the three lanes in S2 (chrome, docked panel, floating layer) plus a message queue | Ad-hoc component state | Not a spike, but build it first, because every screen depends on it. |

## 3. Architecture

```
┌──────────────────────────── WebView (TypeScript) ───────────────────────────┐
│ UI shell (Svelte): Library · Reader chrome · Navigator · Popovers ·         │
│                    Selection bar · Note card/sheet · ⌘K · Messages · Prefs   │
│ Reader state machine (S2 lanes)  ·  Input router (keys/wheel/pointer/touch) │
│ ReaderEngine adapter ──► foliate-js (paginator, CFI, overlayer, footnotes)  │
│ Search worker  ·  Annotation anchoring (CFI + TextQuote, fuzzy re-anchor)   │
│ Extension host: 1 Web Worker per extension + sandboxed iframes for UI       │
└───────────────▲─────────────────────────── typed IPC (Tauri commands) ──────┘
                │
┌───────────────┴──────────────── Rust core ──────────────────────────────────┐
│ Library service (import, hash, metadata, covers) · EPUB zip streaming        │
│ Store (SQLite): books, positions, book_settings, annotations, settings,      │
│   extensions, extension_storage · Search-text cache on disk                   │
│ File associations, OS open events, window/menu, native gesture bridge        │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Data model (first cut).**

- `books`: id (uuid), content_hash, file_path, title, authors, language, page_direction, layout (reflowable or fixed), cover_path, added_at, opened_at, finished_at.
- `positions`: book_id, cfi, fraction, updated_at.
- `book_settings`: book_id, layout_mode, navigator_docked.
- `annotations`: id, book_id, color, cfi_range, quote_exact, quote_prefix, quote_suffix, note, created_at, updated_at, deleted_at (soft delete, for Undo), anchor_status.
- `settings`: key, value.
- `extensions`: id, version, enabled, granted_permissions, installed_at.
- `extension_storage`: ext_id, key, value, with a quota.

Use UUIDs and timestamps from the start so sync can be added later without migrating identity.

**Design tokens.** Lift colours, type, spacing, radii, elevation and motion from the approved screens into one `tokens.ts`, plus CSS custom properties per theme (Paper / Sepia / Night). The highlight tint and underline pairs and the forced-colours rules go in the same file. Every component reads tokens; no hex values in components.

## 4. Phases

Relative size: S ≈ days, M ≈ 1–2 weeks, L ≈ 3+ weeks, for one engineer. The order follows dependencies.

### Phase 0 — Spikes and skeleton (M)
- Tauri 2 app on all three OSes; CI builds; lint, format and type-check.
- **Spike A — rendering:** foliate-js pagination in the three WebViews, using the canvas measurements (66 ch measure clamped to 56–74, whole-line page height, `max(48px, 7vh)` margins).
- **Spike B — input:** can a WebView tell trackpad momentum from a deliberate swipe? If not, build a small native bridge that forwards gesture phases (NSEvent on macOS, precision-touchpad data on Windows) (see Gap T1).
- **Spike C — accessibility:** VoiceOver and NVDA reading continuously inside the paginated iframe, and whether we can see their reading position to turn the visual page (Gap T2).
- Exit: a written go/no-go on Tauri + foliate-js.

### Phase 1 — Foundation (M)
- Tokens and themes (Paper / Sepia / Night / Auto following the system; forced-colours styles; reduced-motion handling).
- Base components: icon button, button, segmented control, switch, tabs, popover, sheet, toast/message queue, `kbd` hints — all keyboard-accessible, with the focus rules from the design.
- SQLite store, migrations, typed IPC.
- Import pipeline: OS file open, drag-drop and ⌘O → validate → hash → copy → extract metadata and cover → generated cover as fallback. Errors map to the Screen 12 “damaged book” card.
- Reader state machine and message queue, with unit tests (S2 rules: one floating layer, chrome and Navigator alternate, Esc pops one level and returns focus to the text).

### Phase 2 — Reader core (L) · Screens 02, 03, 10, 13, 14, 15
- ReaderEngine adapter: open a book, render a chapter, paginate, CFI location, reflow around an anchor on resize or font change (120 ms debounce).
- Immersive state and location line; chrome reveal (150 ms edge dwell, centre tap, Tab) and auto-hide; the OS edge exclusions.
- Input router: keys (with the Space-on-button rule), wheel (one page per notch, 250 ms cooldown), trackpad (one turn per gesture, axis locked by the first 12 px), margin clicks (ignoring the click that activates the window), touch zones 30/40/30 (links and highlights win), finger-tracked swipes, RTL mirroring.
- Scroll mode; two-page spread from 1480 px.
- Progress persistence (1 s debounce, plus on blur and quit); resume chip; Back history and chip.
- Narrow layout (below 1100 px the Navigator floats; below 1240 px notes open as a sheet).
- Night chrome (Screen 14).

### Phase 3 — Navigation (M) · Screens 04, 16, 17
- Navigator shell with Contents · Search · Notes tabs and a Library button; docked or floating by width.
- Contents from nav or NCX, falling back to a heading scan and labelled “Generated from headings”.
- Footnote peek (noteref detection, placement, focus return); internal and external links.
- Go to location (percent, chapter, print page from the page-list), opened with ⌘J.
- Command palette: command registry, fuzzy match, Recently closed, shortcut hints, `?` cheat sheet.

### Phase 4 — Search (M) · Screen 05
- Text extraction and normalisation in a worker (case, diacritics, CJK), with offset maps back to DOM ranges.
- Incremental search (150 ms debounce), current chapter first, results streamed and grouped by chapter, “Searching N of M” progress.
- Match marks in the page (a tint plus an outline for each match, a stronger one for the current match); ↵ / ⇧↵ and ⌘G; Esc returns to the original page if you only browsed.

### Phase 5 — Selection and annotation (L) · Screens 06, 07, 08, 15
- Selection bar: placement rules, F6 to focus it, H and N plus the ⇧⌘ variants, Night inversion, extension actions area.
- Highlights: overlay rendering with tint plus underline, click versus drag, colour change, delete with Undo (10 s, pauses on hover or focus, and listed in ⌘K Recently closed).
- Notes: margin card (1240 px and up) or bottom sheet, autosave, status announced.
- Anchoring: CFI plus a text quote; fuzzy re-anchoring on book updates; a “Couldn’t place” group with a re-attach flow.
- Notes tab: grouped by chapter, filter by colour and text, jump with a pulse and a Back chip.

### Phase 6 — Settings and library (M) · Screens 01, 09, 11-shell
- Aa popover (the four rows with scope labels; ⌘+ ⌘− ⌘0 anywhere).
- Library: Continue reading, grid, search, sort, item menu, and the transition into a book.
- Preferences window shell (General, Reading, Library, Extensions, Shortcuts, About), including the single-key-shortcuts switch.

### Phase 7 — Extensions (L) · Screens 11, 12
- Manifest schema and validation; install from file; enable and disable; uninstall with a prompt to delete its data.
- Extension host: one Worker per extension, sandboxed iframes with a locked content security policy for Navigator tabs, message-based API, permission checks on every call, watchdog (2 s / 10 s timeouts, budgets, suspended after 3 crashes in 10 minutes), safe mode (⇧ at launch plus a menu item).
- Extension slots: commands, selection actions, Navigator tab, theme packs, exporters, annotation events.
- Markdown Export ported to the extension API; a sample Dictionary extension to exercise network and selection permissions.

### Phase 8 — Hardening and release (M–L)
- Accessibility pass against WCAG 2.2 AA: axe on all UI, manual VoiceOver / NVDA / Orca runs, Windows high contrast, 200–400% zoom, user text spacing.
- EPUB edge cases: RTL, vertical writing, fixed layout, huge books, broken CSS and fonts.
- Performance budgets (below).
- Packaging, code signing and notarisation; file associations; an auto-update decision (Gap D6).

**Budgets to hold from Phase 2 onward:**

| Measure | Budget |
|---|---|
| Cold start to library | < 1 s |
| Open a book to first page | < 500 ms, or < 1.5 s for a 100 MB book |
| Page turn | < 16 ms |
| Reflow after resize | < 150 ms |
| Memory for a large book | < 400 MB |

**Test strategy.**

- **Unit tests:** state machine, CFI and anchoring, input thresholds, permission checks.
- **EPUB corpus:** Standard Ebooks (public domain), the IDPF epub3-samples, and deliberately broken files.
- **End-to-end:** WebDriver via `tauri-driver`. It has no macOS support, so macOS needs a Playwright harness against the WebView build, plus manual passes.
- **Visual regression:** against the 17 approved screens.

## 5. Screen → implementation map

| Screen | Phase | Main pieces |
|---|---|---|
| 01 Library | 6 | LibraryView, BookTile, ContinueReading, GeneratedCover |
| 02 / 03 Reader | 2 | ReaderView, Chrome (TopBar, Scrubber, MarginChevrons), LocationLine |
| 04 Contents | 3 | Navigator, ContentsTab |
| 05 Search | 4 | SearchTab, search worker, match overlay |
| 06 / 07 / 15 Selection, notes | 5 | SelectionBar, NoteCard, NoteSheet, HighlightOverlay |
| 08 Notes | 5 | NotesTab, anchoring service |
| 09 Aa | 6 | ReadingSettingsPopover |
| 10 / 14 Themes, Night | 1–2 | tokens, theme switching, Night chrome variants |
| 11 Extensions | 6–7 | Preferences › Extensions, extension host |
| 12 Empty / error | 1, 5, 7 | EmptyLibrary, DamagedBook, ExtensionFailure, MessageQueue |
| 13 Tablet | 2 (desktop touch); tablet app later | TouchZones, SwipePager |
| 16 ⌘K | 3 | CommandPalette, command registry |
| 17 Footnote · Go to | 3 | FootnotePeek, GoToPopover |

## 6. Risks

1. **WebView differences** break pagination parity (worst on WebKitGTK). Mitigation: Spike A, the Electron fallback, and per-engine visual tests.
2. **The trackpad rules may need native input.** Browsers’ wheel events don’t say whether movement is a deliberate swipe or leftover momentum, which the one-turn-per-gesture rule depends on.
3. **Screen reader and pagination sync** may not be observable from inside an iframe.
4. **foliate-js has no stable API.** Mitigation: the adapter layer and a pinned version.
5. **Extension sandbox escape through IPC.** Mitigation: Tauri capability allow-lists, so extensions never reach Tauri commands directly, and every call brokered through the host.

## 7. Gaps the design doesn’t cover

### Missing screens or states (need design before or during the named phase)
- **G1 Extension install and permission consent dialog**, and the re-prompt when an update asks for more (Phase 7). The model is specified, but the moment of consent was never drawn.
- **G2 Preferences sections other than Extensions:** General, Reading (defaults, single-key shortcut switch, publisher styles), Library (copy or link, storage location), Shortcuts, About (Phase 6).
- **G3 Book info sheet.** The failure table says title and author can be edited there, which contradicts “metadata editing is out of scope”. Decide which.
- **G4 Library states:** sort menu open, item menu, remove confirmation or Undo (and whether Remove deletes the file), search with no results, importing several or large files (progress), duplicate import, re-importing an updated file.
- **G5 Panels in Sepia and Night:** only the reader chrome and selection bar are drawn at Night. The Navigator, popovers, ⌘K, sheets, library and Preferences have no Night or Sepia versions.
- **G6 Windows and Linux window chrome.** Every mock uses macOS traffic lights.
- **G7 Extension surfaces:** what an extension’s Navigator tab looks like, the Dictionary result inside a peek, and the contents of the top-bar ⋯ menu.
- **G8 Reading variants:** Scroll mode, the two-page spread, fixed-layout books (zoom and pan), RTL and vertical-writing books, image lightbox, the resume chip in context, and the “≈” approximate locations while pagination finishes.
- **G9 Tablet beyond the tap-zone overlay:** controls visible on tablet, Aa and Navigator as sheets, and iPad edit-menu integration.
- **G10 The `?` shortcut cheat sheet**, and loading or progress states when opening large books.

### Behaviour that is unspecified or ambiguous
- **B1 What a “page” number means.** “Back to page 43” and “Page 5 of 12” use dynamic page numbers that change with font size. Define them (per chapter? per book?) and how they relate to the print page-list.
- **B2 The “time left in chapter” algorithm** (reading-pace model, what data it stores).
- **B3 Import semantics:** the copy-or-link setting, library folder location, duplicate detection (content hash vs identifier), book updates and the re-anchoring flow.
- **B4 Highlight edge cases:** overlapping or nested highlights, highlights spanning chapters, maximum length, and what Copy includes (plain text or with a citation).
- **B5 Undo scope:** only deletions, or also colour changes and note edits?
- **B6 Search limits:** result caps for very common words, and how regex and whole-word options (deferred) would appear.
- **B7 Multiple windows or books open at once:** not addressed.
- **B8 Scroll mode details:** continuous across chapters or per chapter, and what the location line and scrubber show.
- **B9 DRM-protected files:** detection and messaging.
- **B10 UI localisation, including right-to-left UI mirroring** (only right-to-left book content is covered).

### Technical assumptions the design makes that a WebView may not satisfy
- **T1 Trackpad momentum detection.** The design assumes the swipe-versus-momentum signal from the OS; browsers don’t expose it (Spike B).
- **T2 “The visible page follows the screen reader.”** This needs the screen reader’s reading position, which may not be observable (Spike C).
- **T3 Caret browsing with F7.** Chromium has it; WKWebView doesn’t, so it may need our own implementation.
- **T4 iPad edit-menu integration and the Apple Pencil** need native code on Tauri mobile.
- **T5 Platform UI font vs Instrument Sans in the comps.** Decide, and license-check any bundled fonts. Literata is OFL.

### Product and operations decisions not covered
- **D1** Privacy: telemetry and crash reporting (opt-in or none).
- **D2** Backup, export and restore of all user data; uninstall behaviour.
- **D3** Distribution channels: the Mac App Store sandbox limits file access and extensions; also Microsoft Store and Flatpak.
- **D4** Extension package format, signing, API reference docs, and how extensions translate their text.
- **D5** Accessibility conformance target: state WCAG 2.2 AA formally.
- **D6** Auto-update mechanism and release cadence.
- **D7** Minimum OS versions (these decide WebView features).

## 8. Questions for you
1. Is the canvas’s written proposal and system spec (behaviour, shortcuts, extension API) approved along with the 17 screens, or only the screens?
2. Is tablet (iPad) in the first release, or desktop only with tablet next? This decides whether Tauri mobile work and G9 / T4 belong in the MVP.
3. Do you have a preference between Svelte, React and Solid, or between Tauri and Electron?
4. For G3: can users edit metadata or not?
