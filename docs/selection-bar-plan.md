# Compact selection bar — execution plan

The owner asked on 2026-10-12 for a shorter selection bar: icons in place of the action labels, short descriptions on hover, Explain as an icon-only button, the two “Explain” entries looked into, an icon for Explain, and a view on renaming the extension “Lens”. This plan turns that into steps. Its owner decisions are items 88–96 in `docs/pending-approvals.md`, all approved as recommended on 2026-10-13 (`docs/decisions.md`). Status: `docs/selection-bar-status.md`.

It amends two rules of `docs/implementation-plan.md` and `docs/reading-lens-plan.md` (both read-only; the amendment is recorded in `docs/decisions.md`): **A1**'s list of the bar's actions (now icons, with tooltips) and **LK1**'s “both appear in the selection ‘⋯’ menu” (one lookup may have a button on the bar).

## 1. What is there today (read 2026-10-13)

- **The bar is Linen's**, `src/reader/SelectionBar.svelte`: four colour dots | Note, Copy, Search (Delete for a clicked highlight), Look Up, each an icon and a word | “⋯” with the extension lookups and actions as text. 475 px wide in the approved `06-selection-bar.png` (measured inside the bar), before “⋯”.
- **Explain** (`linen-explain`, `io.github.quentinbest.explain` 0.1.0) contributes one lookup, “Explain”, and one ⌘K command; no selection action. Host API 1.1 gives lookups no icon, and every manifest struct has `deny_unknown_fields` (`src-tauri/src/extensions/manifest.rs`), so Linen 0.4.2 refuses a manifest with a field it doesn't know.
- **The second “Explain”.** The owner's library (`~/Library/Application Support/app.linen.reader`, read only) has three extensions, all on: Markdown Export, **Test Lookup** (`test.lookup`, the harness's provider, installed 2026-10-09, whose lookup is also titled “Explain”) and Explain (installed 2026-10-11). The host lists extensions in install order, so “⋯” shows the test provider's “Explain” first, and the peek's “Explain in context” (which opens the first extension lookup) opens the test provider. They are not the same action: the test provider follows a script in its storage and may reach only `127.0.0.1:8765`; Explain sends to the provider chosen in its options (DeepSeek is granted).
- **Tooltips** are the native `title` (`src/components/IconButton.svelte`, X4). The tooltip colours exist and are tested at 7:1 or more in every theme (`--tooltip`, `--tooltip-ink`; dark on Paper and Sepia, light at Night, like the bar).
- **Copy's ⌘C** is the Edit menu's standard Copy, not Linen's plain-text Copy (B4), so the Copy tooltip names no shortcut until that is checked.
- **Renaming** the extension's display name keeps its keys (Keychain account `<extension id>/<name>`, `secrets.rs`), grants and storage (keyed by ID in SQLite).

## 2. Rules

| ID | Rule |
|---|---|
| SB1 | **Same-titled lookups are told apart.** When two or more active extension lookups have the same title (ignoring case and surrounding space), each shows as “‹title› (‹extension name›)”. One whose extension is named like the title keeps the plain title, if it is the only one; when names collide too, the extension's ID replaces the name. The labels are worked out over every active lookup, whatever the selection, so a label never changes between selections. They apply wherever a lookup's title appears: “⋯”, the right-click menu, ⌘K, the peek's provider menu, its “in context” button and SB4's tooltip |
| SB2 | **The bar's tooltips.** Every button on the bar that shows an icon has one, showing its name and, when the command has one, its shortcut, from the registry (“Look Up (⌃⌘D)”). It appears after the pointer rests 500 ms on a button; once one has shown, the next appears at once while the pointer stays in the bar (and for 300 ms after it leaves). Keyboard focus (F6, the arrows, Home and End) shows it at once; a click's focus doesn't. It hides when the pointer leaves, on a click, on Esc, when the bar closes or moves. It sits centred on its button on the side of the bar away from the selection, or the other side when that side has no room in the window, and stays 8 px from the window's edges. One at a time. Colours `--tooltip` and `--tooltip-ink`, 12 px UI text, 6 px radius; fades in with the bar's timing (an opacity change only, which reduced motion keeps); a CanvasText border in forced colours. It takes no focus and no pointer events, never changes the selection, and never delays a click. It is `aria-hidden`: the button's `aria-label` is the same text without the shortcut, and `aria-keyshortcuts` gives the shortcut. Buttons on the bar have no `title`, so two tooltips never show |
| SB3 | **Icon-only core actions.** Note, Copy, Search (Delete for a clicked highlight) and Look Up show their icon only, on a 32 px face. Names: Note, Copy, Search the book, Delete highlight, Look Up. Groups, with dividers: the colours | Note, Copy, Search or Delete | Look Up and SB4's button | “⋯”. Attaching a note (Attach here, Cancel) and the items inside “⋯” keep their words. The right-click menu, the Edit menu, ⌘K and the cheat sheet keep naming every action |
| SB4 | **A lookup's button.** Host API 1.2: a lookup may name an `icon` from a set the core draws (`lightbulb` for now; others are refused at install). The first lookup, in install order, that names an icon, whose `when` holds and that isn't stuck, gets an icon button after Look Up: name and tooltip “‹title› in context” (SB1's title), `data-lookup-item` as in “⋯”. The others stay in “⋯”. A stuck one goes back to “⋯” with Not responding and Restart (Screen 12). Hidden, never greyed, when it doesn't apply (A1). “⋯” shows only when it has a lookup or an action to offer; Manage extensions… alone doesn't show it |
| SB5 | **One Explain.** The peek's “‹title› in context” opens the lookup that has SB4's button when there is one, otherwise the first extension lookup, so the bar and the peek open the same thing |
| SB6 | **Explain's manifest** (private repository): its lookup names `icon: "lightbulb"` and its engines become `^1.2`. Released only after a Linen with Host API 1.2 |

## 3. Steps

Each step ends with `scripts/ci-local.sh` green and its Done-when met before the next begins.

### Step 0 — Approvals and this plan (no code)

- Items 88–95 in `docs/pending-approvals.md`; this plan; `docs/selection-bar-status.md`; a row in `docs/PROGRESS.md`.
- **Done when:** the items are filed with recommendations and the findings above.

### Step 1 — SB1, same-titled lookups

- `src/lib/lookup/providers.ts`: `distinctLookupTitles(lookups)` → a map from `extId/lookupId` to the title to show. Pure.
- `src/lib/lookup/providers.test.ts`: no clash; two extensions with one title; a name equal to the title; names that clash too (the ID); case and spaces.
- `src/extensions/host.svelte.ts`: `lookupTitles` over every slotted lookup (before `when`); `lookups(ctx)` returns the shown title.
- `src/App.svelte`: ⌘K's “Look Up with …” uses the same titles.
- `src/lib/strings/en.ts`: `lens.titleFrom(title, name)` → “‹title› (‹name›)”; machine drafts in `es`, `ja`, `zh-Hans`, `zh-Hant`.
- In-app check `SB1-same-title`: a second package whose lookup is titled “Explain” (`src-tauri/tests/fixtures/ext-packages/lookup-twin`); “⋯”, the right-click menu, ⌘K and the peek's menu tell them apart; the twin is removed at the end.
- **Done when:** the unit tests and `SB1-same-title` pass, and the Stage 2a lookup checks still pass.

### Step 2 — SB2, tooltips (labels still shown)

- `src/reader/barTooltip.ts`: the timing (rest, warm, cool-down) as a small state machine with injected timers; `barTooltip.test.ts` with fake timers.
- `src/reader/SelectionBar.svelte`: a tooltip element; `pointerover` and `pointerout` on the buttons, F6 (`focusFirst`) and the arrow keys, hide on a press, Esc, focus leaving the bar, prop changes (the bar moves) and close; placement after measuring; `aria-keyshortcuts` from the registry's chord (`Reader.svelte` passes `barShortcut`). Each button carries its tooltip text in `data-tip`.
- `src/lib/commands/keys.ts`: `ariaKeyShortcuts(chord)` (“Control+Meta+D”), with a unit test.
- In-app check `SB2-bar-tooltips`: nothing at 300 ms, the tooltip by 700 ms with the right text; the next button's at once; gone on leave; F6 shows the first one at once; Esc hides it and returns focus to the text; the selection is unchanged throughout; the tooltip is inside the window and clear of the selected line when the far side has room.
- **Done when:** the unit tests, `SB2-bar-tooltips`, `A2`'s keyboard checks and `EA1-keyboard-path` pass.

### Step 3 — SB3, icon-only core actions

- `src/reader/SelectionBar.svelte`: icon-only faces (32 px, no padding), `aria-label`, the new divider before Look Up; attach mode unchanged.
- `src/lib/strings/en.ts`: `annotations.deleteHighlight` (“Delete highlight”); Search uses `lookUp.searchBook`. Drafts in the four languages.
- `src/spikes/e2e.ts`: the checks that find Search and Delete by name (`/^search$/i`, `/^delete$/i`) find “Search the book” and “Delete highlight”; LK14's label list.
- Captures for approval: `06-selection-bar`, `14-night-selection`, `12-extension-failure`.
- **Done when:** the selection and annotation checks (A1–A5, A10, K6, K7) pass; the captures are taken.

### Step 4 — SB4 and SB5, the lookup button

- Rust, `src-tauri/src/extensions/manifest.rs`: `Lookup.icon: Option<String>`, checked against `LOOKUP_ICONS = ["lightbulb"]`; `API_VERSION` 1.2.0; tests (accepted, refused, a 1.1 manifest still installs).
- `src-tauri/src/extensions/worker.js`, `docs/extensions/linen.d.ts`: `apiVersion` 1.2.0 and the `icon` field; `docs/extensions/README.md`: a 1.2 section.
- `src/lib/extensions/types.ts`, `host.svelte.ts` (`lookups` returns `icon`), `src/lib/lookup/providers.ts` (`icon` on a provider).
- `src/components/icons.ts`: `lightbulb`, an outline bulb and one base line on the 24 px grid.
- `src/reader/Reader.svelte`: `barLookups` carries `icon`; SB5's key for the peek's “in context”.
- `src/reader/SelectionBar.svelte`: the button after Look Up; the rest in “⋯”; stuck → “⋯”; “⋯” only with something to offer.
- The test provider (`ext-packages/lookup`) names `lightbulb` and requires `^1.2`, so the lookup checks go through the button; the sample Dictionary's “Free Dictionary” stays in “⋯”. `chooseLookup` and the checks that opened “⋯” for the test provider press the button instead.
- In-app check `SB4-lookup-button`: one button, named “Explain in context”, after Look Up; Free Dictionary in “⋯”; none for 41 words, none in safe mode, none in a fixed-layout book (no greyed button); a stuck provider moves to “⋯” with Not responding and Restart; with no other lookup or action, no “⋯”; the peek's “in context” opens the button's lookup (SB5). `EP1-no-request-on-select` still passes: selecting asks nothing, the button asks once.
- Captures for approval: the bar with the button (Paper, Night), a tooltip (Paper), `12-extension-failure`.
- **Done when:** the Rust and unit tests, `SB4-lookup-button` and every lookup check pass.

### Step 5 — SB6, Explain (private repository, a branch only)

- In `linen-explain`, on a branch: the lookup's `icon`, engines `^1.2`, version 0.2.0; its contract tests pass. Not released: it needs a Linen with Host API 1.2 first. The rename waits for item 95.
- **Done when:** its tests pass and the package installs in this branch's Linen (`registry::install`, a temporary test, not committed).

### Step 6 — Verification

- `scripts/ci-local.sh --full`; the spike build; the full in-app suite, with `D1-no-uncaught-errors`; `scripts/e2e.sh v` and `node tests/visual/compare.mjs`; captures listed for approval in a new item.
- Width: the bar with Explain's button against today's bar with Explain in “⋯”, same window and theme: at least 30% narrower.
- **Done when:** all of the above pass, the status document has the evidence, and the captures wait for approval.

## 4. Not in this plan

- The rename (item 95, approved: Reading Lens) is Explain 0.2.0's display name only, never its ID or its lookup's title; it lives in `linen-explain`.
- Removing the test provider from the owner's library (item 93, approved): the owner's action, in Settings › Extensions.
- A first-run hint for the icons: Linen stays quiet; the tooltips and the labelled menus carry the words.
