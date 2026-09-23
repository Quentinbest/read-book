# Linen (Quiet EPUB Reader) — implementation plan

Status: draft, revision B (revised after review on 2026-09-24; see §10) · Design source: `docs/design/Quiet EPUB Reader.html` (17 screens)

## 1. Scope and source of truth

### 1.1 What the repository contains

`docs/design/Quiet EPUB Reader.html` contains **only the 17 screen boards** (01 Library … 17 Footnote peek · Go to location). It was committed as “add design draft” (`3f5da5e`). No approval record exists in the repository.

Behaviour that isn’t visible in a mock comes from the design canvas’s written proposal and system diagrams: reveal rules, input thresholds, the S2 state model, shortcuts, the extension API, permissions and the failure table. **None of these boards are in the repository.** The MVP scope and exclusion lists below also come from that proposal.

### 1.2 Spec gate (blocks Phase 1)

Before Phase 1 starts:

1. The owner records whether the screens are approved, and whether the written proposal is approved with them (Q1). Record both in `docs/design/APPROVAL.md`: date, approver and revision.
2. The rules this plan depends on are committed as `docs/spec/behaviour.md`: every row of the rule register (§1.4) marked “canvas”, plus the S2 lane model, the extension manifest and API, the permission list and the failure table. A reviewer compares it against the canvas.
3. Every value in `docs/spec/behaviour.md` is labelled **approved** or **provisional**. Tests assert approved values. Provisional values live in one config module so tuning them doesn’t touch logic.

Phase 0 may use canvas values before the gate passes, because the spikes measure feasibility, not the exact numbers.

### 1.3 MVP scope

In scope, as defined by the canvas proposal:

- EPUB 2 and 3: open and import, reflowable, basic fixed layout.
- Library: Continue reading, cover grid, search, sort, remove.
- Reader: Pages (default) and Scroll modes, Paper / Sepia / Night / Auto themes.
- Navigation: Contents, chapter keys, scrubber, Back history, footnote peek, Go to location.
- Full-text search within a book.
- Selection, four-colour highlights, notes, and a Notes tab.
- Aa popover: size, theme, line spacing, layout.
- Progress persistence.
- Extension infrastructure, with Markdown Export as the first extension.
- Command palette.

Explicitly out of scope: sync, accounts, AI, TTS, statistics, store, DRM, collections, a marketplace, phone layouts, and **scripted EPUB content** (book JavaScript never runs; see §6.1). Tablet is out of scope unless Q2 says otherwise; desktop touch (Screen 13 zones on touch-screen laptops) is in scope.

### 1.4 Rule register

The source column shows where each value can be checked today.

| Rule | Value | Source | Used in |
|---|---|---|---|
| Notes open as a sheet | below 1240 px | Screen 15 | 2, 5 |
| Message queue | one at a time, newest first, 10 s timers pause on hover/focus, announced | Screen 12 | 1 |
| Touch zones | 30 / 40 / 30; swipe commits past 30% or on a flick; links, footnotes and highlights win; RTL mirrors | Screen 13 | 2 |
| Selection keys | F6 into bar, arrows between actions, Esc back to selection; H / ⇧⌘H, N / ⇧⌘N | Screen 06 | 5 |
| Footnote peek | below marker (above near page foot), never covers marker line, scrolls up to 50% of window height, Esc returns focus | Screen 17 | 3 |
| Highlights | tint plus 2 px underline in every theme | Screen 10 | 5 |
| Theme colours and contrast | Paper / Sepia / Night values | Screen 10 | 1 |
| Night selection bar | inverted surface 13.6:1, chrome hairline #3A3733 | Screen 14 | 5 |
| Shortcuts | ⌘[ Back, ⌘J Go to, ⌘T Go to chapter, ⌘+ ⌘− size, ⌘Z Undo, ⇧ at launch = safe mode | Screens 08, 09, 11, 12, 16, 17 | 2–7 |
| Aa scopes | size, theme, line spacing: all books; layout: this book | Screen 09 | 6 |
| Measure and margins | 66 ch clamped 56–74, whole-line page height, `max(48px, 7vh)` | canvas | 0, 2 |
| Chrome reveal | 150 ms edge dwell, centre tap, Tab | canvas | 2 |
| Wheel and trackpad | one page per notch, 250 ms cooldown; axis locked by first 12 px | canvas | 2 |
| Reflow debounce | 120 ms | canvas | 2 |
| Progress save | 1 s debounce plus blur and quit | canvas | 2 |
| Navigator floats; two-page spread | below 1100 px; from 1480 px | canvas | 2, 3 |
| Search debounce | 150 ms | canvas | 4 |
| Extension watchdog | 2 s / 10 s timeouts, suspended after 3 crashes in 10 min | canvas | 7 |
| S2 lanes | one floating layer, chrome and Navigator alternate, Esc pops one level | canvas | 1 |

## 2. Technical decisions

Each decision has a recommendation. §4 Phase 0 lists the spike that confirms it and the pass criteria.

| Decision | Recommendation | Alternatives | Confirmed by |
|---|---|---|---|
| App shell | **Tauri 2** (Rust core, system WebView). Small and fast, fits “lightweight”, and has an iOS/Android path for tablets later. | Electron: one Chromium engine everywhere, but around 10× larger. Switching after Phase 0 means rewriting the Rust core’s Tauri commands, the rusqlite store and the native gesture bridge as Node code, roughly an extra M. | Spikes A, E |
| EPUB engine | **foliate-js** (claimed MIT; covers pagination, CFI, search, highlight overlay, footnotes and fixed layout; recheck the licence when pinning). Its API is not declared stable, so pin a version and wrap it behind our own `ReaderEngine` interface. | Readium ts-toolkit navigator; epub.js (ageing) | Spikes A, D |
| UI framework | **TypeScript + Svelte 5** (small runtime, fine-grained updates for a UI that is mostly idle). | React or Solid | Q3. Svelte 5 is the default if Q3 is unanswered when the Phase 0 skeleton starts. |
| Persistence | **SQLite** in the Rust core (`rusqlite`), exposed through typed Tauri commands. Book files copied into the app data folder (Screen 12: “Books are copied into your library”). | IndexedDB in the WebView (simpler, but weaker durability and no native backup) | Spike F |
| Search | In-worker scan over each chapter’s normalised plain text, cached to disk per book. Current chapter first, results streamed. | SQLite FTS5. The trigram tokenizer (SQLite ≥ 3.34) does substring matching, but the index is roughly 3× the text, and CJK and diacritic folding still need our own normalisation. Revisit for library-wide search. | Spike F |
| Book content isolation | Book documents are served by a custom protocol from the Rust core with a strict CSP, and rendered in iframes that can’t run script and can’t reach IPC (§6.1). | Rendering from blob URLs in the app origin (rejected: shares the origin that holds IPC) | Spike E |
| Reader state | An explicit state machine for the three S2 lanes (chrome, docked panel, floating layer) plus a message queue | Ad-hoc component state | Not a spike, but build it first in Phase 1, because every screen depends on it. |

## 3. Architecture

```
┌──────────────────────────── WebView (TypeScript) ───────────────────────────┐
│ UI shell (Svelte): Library · Reader chrome · Navigator · Popovers ·         │
│                    Selection bar · Note card/sheet · ⌘K · Messages · Prefs   │
│ Reader state machine (S2 lanes)  ·  Input router (keys/wheel/pointer/touch) │
│ ReaderEngine adapter ──► foliate-js (paginator, CFI, overlayer, footnotes)  │
│   └─ book iframes: no script, no IPC, CSP from the book:// protocol         │
│ Search worker  ·  Annotation anchoring (CFI + TextQuote, fuzzy re-anchor)   │
│ Extension host: 1 Worker per extension (no direct network) + sandboxed      │
│   iframes for UI; all network and data access brokered by the host          │
└───────────────▲─────────────────────────── typed IPC (Tauri commands) ──────┘
                │   capability allow-list: main window only
┌───────────────┴──────────────── Rust core ──────────────────────────────────┐
│ Library service (import, hash, metadata, covers) · EPUB zip streaming        │
│   with entry/size/path limits · book:// protocol (CSP headers)               │
│ Store (SQLite): books, book_damage, positions, book_settings, annotations,   │
│   settings, extensions, extension_storage · Search-text cache on disk         │
│ File associations, OS open events, window/menu, native gesture bridge        │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Data model (first cut; provisional until B3 and B4 are decided, which must happen before Phase 1 exits).**

- `books`: id (uuid), content_hash, package_identifier (the OPF `unique-identifier` value), file_path, title, authors, language, page_direction, layout (reflowable or fixed), cover_path, added_at, opened_at, finished_at, replaced_at.
- `book_damage`: book_id, item_href, error_kind. Feeds the Contents markers on Screen 12.
- `positions`: book_id, cfi, fraction, updated_at.
- `book_settings`: book_id, layout_mode, navigator_docked.
- `annotations`: id, book_id, anchored_content_hash (the file version the anchor was made against), color, cfi_range, quote_exact, quote_prefix, quote_suffix, note, created_at, updated_at, deleted_at (soft delete, for Undo), anchor_status.
- `settings`: key, value.
- `extensions`: id, version, enabled, granted_permissions, installed_at.
- `extension_storage`: ext_id, key, value, with a quota.
- Reading-pace data for “min left in chapter”: shape decided with B2.

`package_identifier` plus `content_hash` let the importer recognise a changed file of the same book. `anchored_hash` lets anchoring detect “The book file changed after this was saved” (Screen 08). Use UUIDs and timestamps from the start so sync can be added later without migrating identity.

**Design tokens.** Lift colours, type, spacing, radii, elevation and motion from the screens into one `tokens.ts`, plus CSS custom properties per theme (Paper / Sepia / Night). The highlight tint and underline pairs and the forced-colours rules (present in 16 of the 17 screens) go in the same file. Every component reads tokens; no hex values in components.

## 4. Phases

Relative size: S ≈ days, M ≈ 1–2 weeks, L ≈ 3+ weeks, for one engineer. The order follows dependencies. Each phase ends with a **Done when** list; a phase isn’t complete until every item is met and the §5 gates pass. Where a phase needs a later phase’s work, it ships a named stub, removed in the later phase.

### Phase 0 — Spikes and skeleton (M–L)

**Before starting:**
- D7 decided: minimum OS versions. Recommendation, to confirm: macOS 13+, Windows 10 22H2+ with evergreen WebView2, Ubuntu 22.04+ (WebKitGTK 4.1, which Tauri 2 requires). Record the exact WebView version each spike ran on.
- Reference machines named in `docs/spikes/reference-machines.md`: one mid-range machine per OS, plus the oldest supported macOS.
- Q3 decided, or the Svelte 5 default taken.
- Test corpus assembled (§5.2).

**Work:**
- Tauri 2 app on all three OSes; CI builds; lint, format and type-check.
- Each spike writes `docs/spikes/<letter>-<name>.md` with its method, raw numbers and a pass/fail verdict per criterion.

| Spike | Pass criteria (on every reference machine unless stated) | If it fails |
|---|---|---|
| **A — rendering parity** | foliate-js pagination in WKWebView, WebView2 and WebKitGTK with the canvas measurements. On 20 fixed corpus chapters at 3 font sizes, the page count per chapter differs by ≤ 2% between engines. No clipped or split lines at page boundaries. Screenshots differ only in font rasterisation. | WebKitGTK only: ship Linux later, or evaluate Electron for Linux alone. Any engine: evaluate Readium, then Electron. |
| **B — input** | Can the WebView tell trackpad momentum from a deliberate swipe (Gap T1)? Pass: over 50 scripted and 50 manual gestures, one turn per gesture, with no extra turns from momentum. Otherwise prototype the native bridge (NSEvent phases on macOS, precision-touchpad data on Windows) and meet the same bar. | Keep wheel rules only (one page per notch, 250 ms cooldown) and record the degraded trackpad behaviour as a known limitation for owner sign-off. |
| **C — accessibility** | VoiceOver and NVDA read continuously inside the paginated iframe across at least 3 page boundaries. Record whether their reading position is observable to turn the visual page (Gap T2). | Fallback: continuous reading works in Scroll mode. Pages mode offers “Read from here”, which switches to Scroll for the session. Record this as a design change for owner approval. |
| **D — engine fidelity** | Moby-Dick opens to first page < 500 ms; a 100 MB corpus book < 1.5 s. 200 random CFIs round-trip to the same text position after changing font size 16 → 24 → 16 px and after resizing 1280 → 800 → 1280 px. 100 highlights stay on their exact text after the same reflows. Page turn < 16 ms at p95. | Evaluate Readium ts-toolkit against the same criteria before building anything in Phase 2. |
| **E — content isolation** | foliate-js renders, paginates and overlays highlights with book iframes that can’t run script and have the §6.1 CSP. A hostile test EPUB fails to: run script (inline, `onload`, `javascript:` links, SVG script); call any Tauri command; load any remote resource; read another book’s files; or escape the library folder via zip paths. Verified on all three WebViews. | Blocks Phase 2. If foliate-js needs script in the frame, isolate book frames in a separate origin that has no IPC capability, and re-run. |
| **F — persistence and search** | A progress write with WAL on is < 5 ms at p95. Killing the process during 1,000 writes loses at most the last uncommitted write, and the database opens cleanly. Searching all 135 chapters of Moby-Dick from the cache takes < 300 ms. Diacritic folding (é → e) and CJK matching work on corpus samples. | Persistence: revisit IndexedDB or batching. Search: move the scan to FTS5 trigram and re-run. |

**Done when:** all six spike reports exist with verdicts; `docs/spikes/decision.md` records go or no-go for Tauri + foliate-js and names the fallback taken for each failed criterion; the skeleton builds and passes lint and type-check in CI on all three OSes.

### Phase 1 — Foundation (M)

**Before starting:** the spec gate (§1.2) has passed. B7 (multiple windows) is decided, because it shapes the state machine.

- Tokens and themes (Paper / Sepia / Night / Auto following the system; forced-colours styles; reduced-motion handling).
- Base components: icon button, button, segmented control, switch, tabs, popover, sheet, toast/message queue, `kbd` hints. All keyboard-accessible, with the focus rules from the design.
- SQLite store, versioned migrations, typed IPC with the Tauri capability allow-list limited to the main window.
- Import pipeline: OS file open, drag-drop and ⌘O → validate with the zip limits (§6.1) → hash → duplicate and replaced-file check (per B3) → copy → extract metadata and cover → generated cover as fallback. Unopenable files map to Screen 12’s “damaged book” card; files with some unreadable items are imported and recorded in `book_damage`.
- Write-failure path: any failed store write raises the queued “Couldn’t save notes to disk · Retry” message (Screen 12). Retry re-sends the pending write; nothing is dropped silently.
- Reader state machine and message queue, with unit tests for the S2 rules.

**Done when:**
- Unit tests cover every S2 transition in `docs/spec/behaviour.md`.
- Every theme passes the contrast values on Screen 10.
- axe reports no serious or critical violations on the component gallery.
- Importing each corpus file gives the expected result: imported, imported with damage records, rejected with the damaged-book card, or rejected as hostile.
- Duplicate and replaced-file imports follow B3.
- A simulated disk-full write shows the Retry message, and Retry succeeds once space returns.
- The migration test (§5.3) passes.
- B1, B3, B4 and B7 are decided.

### Phase 2 — Reader core (L) · Screens 02, 03, 10, 13, 14, 15

**Before starting:** Spikes A, D and E passed, or their fallbacks were taken. B1 (page numbers) and B8 (Scroll mode) are decided. G8 designs exist for Scroll mode and the two-page spread.

- ReaderEngine adapter: open a book, render a chapter, paginate, CFI location, reflow around an anchor on resize or font change (120 ms debounce). Book frames follow §6.1.
- Immersive state and location line, including “N min left in chapter” per B2 (the reading-pace store lands here). Chrome reveal (150 ms edge dwell, centre tap, Tab) and auto-hide. OS edge exclusions.
- Input router: keys (with the Space-on-button rule), wheel (one page per notch, 250 ms cooldown), trackpad per the Spike B outcome (one turn per gesture, axis locked by the first 12 px), margin clicks (ignoring the click that activates the window), touch zones 30/40/30 (links and highlights win), finger-tracked swipes, RTL mirroring.
- Scroll mode; two-page spread from 1480 px.
- Progress persistence (1 s debounce, plus on blur and quit); resume chip; Back history and chip.
- Narrow layout (below 1100 px the Navigator floats; below 1240 px notes open as a sheet).
- Night chrome (Screen 14). The selection-bar inversion is stubbed until Phase 5.

**Done when:**
- Unit tests assert every input threshold in the rule register.
- Scripted end-to-end tests (§5.1) turn pages by key, wheel, click and touch zone in LTR and RTL books.
- Quitting mid-chapter and reopening restores the same CFI.
- Resize and font change keep the reading position (the Spike D check re-run in the app).
- Budgets from §5.4 that apply from Phase 2 hold on the reference machines.
- Hostile corpus EPUBs still fail as in Spike E.
- Visual baselines for Screens 02, 03, 10, 13, 14 and 15 are approved (§5.1).

### Phase 3 — Navigation (M) · Screens 04, 16, 17

- Navigator shell with Contents · Search · Notes tabs and a Library button; docked or floating by width.
- Contents from nav or NCX, falling back to a heading scan labelled “Generated from headings”. Chapters in `book_damage` are marked, so nothing looks silently missing (Screen 12).
- Footnote peek (noteref detection, placement, focus return); internal and external links. External links open in the system browser, never in the WebView.
- Go to location (percent, chapter, print page from the page-list), opened with ⌘J; Go to chapter with ⌘T.
- Command palette: command registry, fuzzy match, Recently closed (entries registered by later phases; empty until Phase 5), shortcut hints, `?` cheat sheet (G10 design needed).

**Done when:**
- Contents is correct for the corpus (nav, NCX-only, and no-TOC books), and damaged chapters show the marker.
- Footnote peek meets every Screen 17 rule in an end-to-end test, including focus return.
- Go to lands on the stated location for percent, chapter and page-list targets.
- Every command in the registry is reachable from ⌘K and shows its shortcut.
- Visual baselines for Screens 04, 16 and 17 are approved.

### Phase 4 — Search (M) · Screen 05

**Before starting:** B6 (result caps) is decided.

- Text extraction and normalisation in a worker (case, diacritics, CJK), with offset maps back to DOM ranges.
- Incremental search (150 ms debounce), current chapter first, results streamed and grouped by chapter, “Searching N of M” progress.
- Match marks in the page (a tint plus an outline for each match, a stronger one for the current match); ↵ / ⇧↵ and ⌘G; Esc returns to the original page if you only browsed.

**Done when:**
- A golden-result test gives the expected hit count and positions for each corpus query set, including diacritic and CJK queries.
- Every result lands on its match in the page.
- The Esc-return rule passes an end-to-end test.
- Search timing holds the Spike F budget in the app.
- The Screen 05 visual baseline is approved.

### Phase 5 — Selection and annotation (L) · Screens 06, 07, 08, 15

**Before starting:** B4 (highlight edge cases) and B5 (Undo scope) are decided.

- Selection bar: placement rules, F6 to focus it, H and N plus the ⇧⌘ variants, Night inversion (removes the Phase 2 stub), and an extension actions area that stays empty until Phase 7.
- Highlights: overlay rendering with tint plus underline, click versus drag, colour change, delete with Undo (10 s, pauses on hover or focus, listed in ⌘K Recently closed).
- Notes: margin card (1240 px and up) or bottom sheet, autosave, status announced, with the Phase 1 write-failure path.
- Anchoring: CFI plus a text quote, and `anchored_content_hash`. Fuzzy re-anchoring when a replaced file is imported (per B3). A “Couldn’t place” group with a re-attach flow.
- Notes tab: grouped by chapter, filter by colour and text, jump with a pulse and a Back chip. The “Export as Markdown” entry is hidden until Phase 7.

**Done when:**
- Anchoring tests pass: highlights survive reflow, and survive re-import of a corpus book edited in known ways (inserted paragraph, changed punctuation, deleted passage). Unplaceable ones land in “Couldn’t place”, never at a wrong position.
- The B4 edge cases have tests.
- Undo restores the exact annotation, including after the message has timed out, via ⌘K.
- A note typed and then followed by a quit is saved.
- Visual baselines for Screens 06, 07, 08 and 15 are approved.

### Phase 6 — Settings and library (M) · Screens 01, 09, 11-shell

**Before starting:** G2 and G4 designs exist; D2 (backup and restore) is decided.

- Aa popover (the four rows with scope labels; ⌘+ ⌘− ⌘0 anywhere).
- Library: Continue reading, grid, search, sort, item menu, remove (per G4), and the transition into a book.
- Preferences window shell (General, Reading, Library, Extensions, Shortcuts, About), including the single-key-shortcuts switch.
- **Spike G — extension network isolation** (S), so Phase 7 starts on a proven mechanism: see §6.2.

**Done when:**
- Aa scope rules hold (size, theme and line spacing change all books; layout changes this book only) and persist across restarts.
- Library search and sort give the expected order on a 500-book fixture.
- Removing a book follows G4, including what happens to its file and annotations.
- The cold-start budget holds with the 500-book fixture.
- Spike G report passes.
- Visual baselines for Screens 01, 09 and the Screen 11 shell are approved.

### Phase 7 — Extensions (L) · Screens 11, 12

**Before starting:** Spike G passed; D4 (package format) decided; G1 (consent dialog) and G7 (extension surfaces) designed.

- Manifest schema and validation; install from file; consent dialog (G1); enable and disable; uninstall with a prompt to delete its data.
- Extension host: one Worker per extension with the Spike G isolation, sandboxed iframes with a locked CSP for Navigator tabs, message-based API, permission checks on every brokered call, watchdog (2 s / 10 s timeouts, budgets, suspended after 3 crashes in 10 minutes), safe mode (⇧ at launch plus a menu item).
- Extension slots: commands, selection actions, Navigator tab, theme packs, exporters, annotation events.
- Markdown Export ported to the extension API (removes the Phase 5 hidden entry). A sample Dictionary extension to exercise network and selection permissions.
- Extension API reference in `docs/extensions/`.

**Done when:**
- A hostile test extension fails to reach any undeclared host, any Tauri command, book text without the permission, or another extension’s storage. Verified on all three WebViews.
- The watchdog restarts and suspends a hanging or crashing extension as specified, while reading continues (Screen 12).
- Safe mode starts with all extensions off.
- Markdown Export output matches a golden file.
- Visual baselines for Screens 11 and 12 are approved.

### Phase 8 — Hardening and release (M–L)

**Before starting:** D1, D3, D5 and D6 are decided.

- Accessibility pass against the D5 target (recommended: WCAG 2.2 AA): manual VoiceOver / NVDA / Orca runs, Windows high contrast, 200–400% zoom, user text spacing. Automated axe checks have run in every phase since Phase 1.
- EPUB edge cases: RTL, vertical writing, fixed layout, huge books, broken CSS and fonts.
- Performance budgets on all reference machines.
- Backup and restore per D2; uninstall behaviour.
- Packaging, code signing and notarisation; file associations; auto-update per D6.

**Done when:** the release checklist in §5.5 passes on all three OSes.

## 5. Verification

### 5.1 Test layers and what each one proves

| Layer | Runs | Proves | Doesn’t prove |
|---|---|---|---|
| Unit tests (TypeScript and Rust) | every commit | State machine, input thresholds, CFI and anchoring, permission checks, import validation and zip limits | Rendering or real input |
| Frontend integration tests: Playwright on Chromium and WebKit, with IPC mocked | every commit | UI flows and focus rules | Behaviour of WKWebView, WebView2 or WebKitGTK inside the app; real IPC |
| App end-to-end tests, Windows and Linux: WebDriver via `tauri-driver` | every merge to main | Real app, real IPC, real engine | macOS (`tauri-driver` has no macOS support at time of writing; recheck at Phase 0) |
| macOS app tests | every merge to main (smoke) and every release (full) | Real WKWebView app | Anything not in the script |
| Visual regression | every merge to main | No unreviewed visual change | Behaviour |
| Performance runs on reference machines | weekly, and before each phase closes | §5.4 budgets | — |

On macOS, Phase 0 decides between two options:
- An accessibility-driven UI driver (for example Appium’s Mac2 driver, unverified for this app) as the smoke suite.
- A written manual script if no driver works.

Either way, macOS is the main design platform, so every phase’s Done-when items are also checked by hand on macOS.

**Visual baselines.** The mocks can’t be pixel baselines. They contain spec overlays (Screens 06, 13 and 14), template placeholders (Screens 09 and 17) and fixed sample content. For each screen:
1. Build the same state from a fixture: Moby-Dick chapter 1 at 1280 × 800, the mock’s theme.
2. Compare it with the mock by eye and record sign-off.
3. Store that screenshot per engine as the baseline.

After that, diffs run against the approved screenshots.

### 5.2 Test corpus

- Standard Ebooks (public domain) and the IDPF epub3-samples.
- Deliberately broken files: truncated zip, bad OPF, missing items, bad CSS and fonts.
- A 100 MB book, and a 500-book library fixture.
- **Hostile EPUBs:**
  - inline script, event handlers and `javascript:` links
  - SVG with script
  - remote images, fonts and CSS
  - zip entries with `..`, absolute paths or symlinks
  - a zip bomb and 100,000-entry archives
  - oversized XML entities

### 5.3 Data safety tests

- **Migrations:** a fixture database from every schema version ever shipped, or from each phase before release, migrates to the current version with no data loss; checked row by row.
- **Crash consistency:** kill the process during progress, annotation and import writes; the database opens and the last committed state is intact.
- **Write failure:** disk full or a read-only folder raises the Retry message; nothing is dropped silently.

### 5.4 Budgets

Measured on the reference machines with release builds; the median and p95 of 20 runs are recorded.

| Measure | Budget | Starting point | Applies from |
|---|---|---|---|
| Cold start to library | < 1 s | process launch → library first paint, 500-book fixture | Phase 6 |
| Cold start to last book | < 1.5 s | process launch → first page of resumed book | Phase 2 |
| Open a book to first page | < 500 ms, or < 1.5 s for a 100 MB book | click → first page painted | Phase 2 |
| Page turn | < 16 ms p95 | input event → next frame painted | Phase 2 |
| Reflow after resize | < 150 ms | end of resize debounce → page painted | Phase 2 |
| Memory for a large book | < 400 MB | resident memory of all processes after reading 50 pages of the 100 MB book | Phase 2 |

### 5.5 Release checklist

- Every phase’s Done-when list still passes.
- Performance budgets hold.
- Accessibility target met, with manual runs recorded.
- Hostile corpus results unchanged.
- Backup and restore round-trip passes.
- Builds signed and notarised; file associations work on each OS.

### 5.6 Re-verification

- **foliate-js upgrade:** re-run Spike D’s CFI and highlight checks, the full corpus and the visual baselines before merging.
- **Tauri or WebView floor change:** re-run the Spike E hostile corpus and the visual baselines.
- **Change to `docs/spec/behaviour.md`:** update the affected tests in the same change; values move between provisional and approved only with owner sign-off.
- **Schema change:** add a migration fixture.
- CI blocks a merge when unit, integration, end-to-end or visual tests fail.

## 6. Security

### 6.1 Book content (untrusted input)

- **Scripts:** book JavaScript never runs. Book iframes are sandboxed without `allow-scripts`, and the `book://` protocol sends `script-src 'none'`.
- **Network:** `default-src` is limited to `book:` and `data:`; no remote resources.
- **IPC:** book frames can never call Tauri commands. The capability allow-list covers only the main window’s own origin; Spike E verifies this rather than assuming it.
- **Links:** external links open in the system browser after a check that they are `http(s)`.
- **Zip handling:**
  - Reject absolute paths, `..` segments and symlinks.
  - Proposed limits, tuned after the corpus run: ≤ 10,000 entries, ≤ 1 GB total uncompressed, per-entry compression ratio ≤ 100:1.
  - Stream entries; never extract to disk outside the book’s own cache folder.
- **XML:** the OPF and navigation parsers reject external entities and cap entity expansion.

### 6.2 Extensions

Extensions never reach Tauri commands directly; every call is brokered through the host and permission-checked.

For network access, extension Workers get **no direct network**. Candidate mechanism, proven by Spike G:
- Each Worker is served from its own origin with `connect-src 'none'`.
- Nested Workers are blocked, and `importScripts` is limited to the extension’s own package.
- The host performs `fetch` for declared hosts only.

Spike G passes when a test extension can’t reach an undeclared host through `fetch`, XHR, WebSocket, EventSource, `importScripts`, nested Workers or an iframe, on all three WebViews.

## 7. Screen → implementation map

| Screen | Phase | Main pieces |
|---|---|---|
| 01 Library | 6 | LibraryView, BookTile, ContinueReading, GeneratedCover |
| 02 / 03 Reader | 2 | ReaderView, Chrome (TopBar, Scrubber, MarginChevrons), LocationLine |
| 04 Contents | 3 | Navigator, ContentsTab, damage markers |
| 05 Search | 4 | SearchTab, search worker, match overlay |
| 06 / 07 / 15 Selection, notes | 5 | SelectionBar, NoteCard, NoteSheet, HighlightOverlay |
| 08 Notes | 5 (export entry in 7) | NotesTab, anchoring service |
| 09 Aa | 6 | ReadingSettingsPopover |
| 10 / 14 Themes, Night | 1–2 (Night selection bar in 5) | tokens, theme switching, Night chrome variants |
| 11 Extensions | 6–7 | Preferences › Extensions, extension host |
| 12 Empty / error | 1, 3, 5, 7 | EmptyLibrary, DamagedBook, write-failure Retry, ExtensionFailure, MessageQueue |
| 13 Tablet | 2 (desktop touch); tablet app per Q2 | TouchZones, SwipePager |
| 16 ⌘K | 3 | CommandPalette, command registry |
| 17 Footnote · Go to | 3 | FootnotePeek, GoToPopover |

## 8. Risks

1. **WebView differences** break pagination parity (worst on WebKitGTK). Mitigation: Spike A with numeric criteria, per-engine visual baselines, and the fallbacks in the Spike A row.
2. **The trackpad rules may need native input.** Browsers’ wheel events don’t say whether movement is a deliberate swipe or leftover momentum. Mitigation: Spike B, the native bridge, or a signed-off degraded rule.
3. **Screen reader and pagination sync** may not be observable from inside an iframe. Mitigation: Spike C and its Scroll-mode fallback.
4. **foliate-js has no stable API.** Mitigation: the adapter layer, a pinned version, and the upgrade re-verification rule (§5.6).
5. **Hostile book content** reaching IPC, the file system or the network. Mitigation: §6.1, Spike E, and the hostile corpus in CI.
6. **Extension sandbox escape** through IPC or direct network. Mitigation: §6.2 and Spike G.
7. **Spec drift**, from work built on rules not yet in the repo. Mitigation: the spec gate (§1.2) and provisional-value config.
8. **macOS is the main target but has the weakest automated coverage.** Mitigation: the macOS layer decided in Phase 0, plus manual Done-when checks.

## 9. Open decisions and gaps

Every item has a deadline, set by the phase that consumes it. If an item isn’t decided by its deadline, that phase doesn’t start (“Before starting” in §4), unless the item lists a default. Owner roles: **P** = product/design owner, **T** = tech lead.

### 9.1 Questions for you

| # | Question | Owner | Decide by | Default if undecided |
|---|---|---|---|---|
| Q1 | Is the canvas’s written proposal and system spec (behaviour, shortcuts, extension API) approved along with the 17 screens, or only the screens? Record in `docs/design/APPROVAL.md`. | P | Phase 1 start | none (spec gate) |
| Q2 | Is tablet (iPad) in the first release, or desktop only with tablet next? Decides whether Tauri mobile work, G9 and T4 belong in the MVP. | P | Phase 2 start | desktop only |
| Q3 | Svelte, React or Solid? Tauri or Electron (beyond the Phase 0 result)? | T | Phase 0 skeleton | Svelte 5, Tauri 2 |
| Q4 | For G3: can users edit metadata or not? | P | Phase 6 start | no editing |

### 9.2 Behaviour that is unspecified or ambiguous

| # | Item | Decide by |
|---|---|---|
| B1 | What a “page” number means. “Back to page 43” and “Page 5 of 12” use dynamic page numbers that change with font size. Per chapter or per book, and how they relate to the print page-list. | Phase 1 exit |
| B2 | The “time left in chapter” algorithm: reading-pace model and what data it stores. | Phase 2 start |
| B3 | Import semantics: the copy-or-link setting, library folder location, duplicate detection (content hash vs `package_identifier`), book updates and the re-anchoring flow. | Phase 1 start |
| B4 | Highlight edge cases: overlapping or nested highlights, highlights spanning chapters, maximum length, and what Copy includes. | Phase 1 exit (schema) |
| B5 | Undo scope: only deletions, or also colour changes and note edits? | Phase 5 start |
| B6 | Search limits: result caps for very common words; how deferred regex and whole-word options would appear. | Phase 4 start |
| B7 | Multiple windows or books open at once. | Phase 1 start |
| B8 | Scroll mode: continuous across chapters or per chapter; what the location line and scrubber show. | Phase 2 start |
| B9 | DRM-protected files: detection and messaging. | Phase 1 exit |
| B10 | UI localisation, including right-to-left UI mirroring. | Phase 1 exit (whether strings are externalised from the start) |

### 9.3 Missing screens or states (design needed by the named phase)

- **G1** Extension install and permission consent dialog, and the re-prompt when an update asks for more (Phase 7).
- **G2** Preferences sections other than Extensions: General, Reading, Library, Shortcuts, About (Phase 6).
- **G3** Book info sheet. The failure table says title and author can be edited there, which contradicts “metadata editing is out of scope” (Q4; Phase 6).
- **G4** Library states: sort menu open, item menu, remove confirmation or Undo (and whether Remove deletes the file), search with no results, importing several or large files, duplicate import, re-importing an updated file (Phase 6; the import-related states by Phase 1 exit).
- **G5** Panels in Sepia and Night: Navigator, popovers, ⌘K, sheets, library and Preferences (Phase 2 start for reader panels, Phase 6 for the rest).
- **G6** Windows and Linux window chrome; every mock uses macOS (Phase 1 exit).
- **G7** Extension surfaces: an extension’s Navigator tab, the Dictionary result inside a peek, the top-bar ⋯ menu (Phase 7).
- **G8** Reading variants: Scroll mode, two-page spread, fixed-layout zoom and pan, RTL and vertical writing, image lightbox, resume chip in context, “≈” approximate locations (Phase 2 start).
- **G9** Tablet beyond the tap-zone overlay (only if Q2 puts tablet in the MVP).
- **G10** The `?` cheat sheet (Phase 3), and loading or progress states for large books (Phase 2).

### 9.4 Technical assumptions a WebView may not satisfy

| # | Item | Resolved by |
|---|---|---|
| T1 | Trackpad momentum detection | Spike B |
| T2 | “The visible page follows the screen reader” | Spike C |
| T3 | Caret browsing with F7: Chromium has it; WKWebView may need our own implementation | Phase 2 start |
| T4 | iPad edit-menu integration and Apple Pencil need native code on Tauri mobile | only if Q2 = tablet |
| T5 | Platform UI font vs Instrument Sans (used in all 17 screens); license-check bundled fonts (Literata is OFL) | Phase 1 start |

### 9.5 Product and operations decisions

| # | Item | Decide by |
|---|---|---|
| D1 | Privacy: telemetry and crash reporting (opt-in or none) | Phase 8 start |
| D2 | Backup, export and restore of all user data; uninstall behaviour | Phase 6 start |
| D3 | Distribution channels: Mac App Store sandbox limits file access and extensions; Microsoft Store, Flatpak | Phase 1 start (affects the file-access model), with a final decision at Phase 8 |
| D4 | Extension package format, signing, API reference, extension localisation | Phase 7 start |
| D5 | Accessibility conformance target (recommend WCAG 2.2 AA) | Phase 1 start |
| D6 | Auto-update mechanism and release cadence | Phase 8 start |
| D7 | Minimum OS versions (these decide WebView features) | Phase 0 start |

## 10. Revision history

- **Revision B (2026-09-24):** addressed the plan review.
  - Spec gate and rule register (§1.2, §1.4).
  - Phase 0 spikes D–F added, with numeric pass criteria and fallbacks for every spike.
  - Decision register with deadlines (§9).
  - Done-when criteria per phase.
  - Book-content and extension-network security (§6, Spikes E and G).
  - Revised verification layers, visual baselines, data-safety tests and budget method (§5).
  - Stubs for cross-phase dependencies.
  - Corrected the FTS5 rationale; costed the Electron fallback; re-verification rules (§5.6).
- **Revision A:** initial draft.
