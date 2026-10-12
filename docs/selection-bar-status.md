# Compact selection bar status

The state of each step of `docs/selection-bar-plan.md` (§3), on branch `selection-bar-compact`. Its owner decisions, items 88–96 in `docs/pending-approvals.md`, were approved as recommended on 2026-10-13 (`docs/decisions.md`); the six captures are baselines.

| Step | State | Notes |
|---|---|---|
| 0 Approvals and plan | Done (2026-10-13) | Items 88–95 filed with the findings of `docs/selection-bar-plan.md` §1 |
| 1 SB1 Same-titled lookups | Done (2026-10-13) | `distinctLookupTitles` (6 unit tests); the host, ⌘K and so “⋯”, the right-click menu and the peek use it; fixture `lookup-twin`. In the app: `SB1-same-title`, `LK1-menu-items`, `LK14-highlight-lookup`, `A10-context-menu` and `D1` pass. See Runs |
| 2 SB2 Tooltips | Done (2026-10-13) | `barTooltip.ts` (7 unit tests), `ariaKeyShortcuts` (4 cases), the tooltip in `SelectionBar.svelte`, shortcuts from the registry. In the app: `SB2-bar-tooltips`, `A1-selection-bar`, `A3-keys`, `EA1-keyboard-path` and `D1` pass |
| 3 SB3 Icon-only core actions | Done (2026-10-13) | Note, Copy, Search the book or Delete highlight, Look Up: icons (18 px) on 32 px faces, named by `aria-label`; a divider before Look Up; attach mode unchanged. String “Delete highlight” with drafts. In the app: A1–A11, `LK14`, `LK1-menu-items`, `EA1-keyboard-path` (now reads the name), `P10`, `SB1`, `SB2`, `D1` pass. The captures wait for Step 4, which changes the same screens |
| 4 SB4, SB5 The lookup button | Done (2026-10-13) | Rust `Lookup.icon`, `LOOKUP_ICONS`, API 1.2.0 (test `lookup_icons_are_ones_linen_draws`); worker, `linen.d.ts`, README; `lightbulb` icon; the button, “⋯” only with something in it, stuck → “⋯”; SB5 in `Reader.svelte`; the test provider names the icon. In the app: `SB4-lookup-button` and every lookup check pass but `LK7-secrets` (canary port). Captures: item 96. Widths below |
| 5 SB6 Explain's manifest (private) | Done (2026-10-13), not committed or released | `linen-explain`, branch `selection-bar-icon` (uncommitted): manifest 0.2.0, `^1.2`, `icon: "lightbulb"`; `scripts/check.mjs` asks for 1.2 and allows only `lightbulb`; the test harness's `apiVersion` 1.2.0; README. `npm test` 117 passed (5 skipped), `npm run check` passes, `npm run pack` gives `dist/explain-0.2.0.linenext`, which installs through this branch's `registry::install` with its icon (a temporary test, removed). Renamed “Reading Lens” after item 95's approval (display name, the one options sentence that names it, README); the action “Explain”, the command and the approved notices stay. The renamed package installs as “Reading Lens” with its icon (temporary test, removed) |
| 6 Verification | Done (2026-10-13), but for the canary checks | `scripts/ci-local.sh --full` green (format, lint, types, 413 unit tests, design, rustfmt, clippy `-D warnings`, cargo test, Playwright); clippy with `spikes` clean. Full in-app suite: **172 of 175**, `D1-no-uncaught-errors` included; the three failures (`P2-hostile-extension`, `EP1-no-request-on-select`, `LK7-secrets`) need the canary on port 8765, which another program holds (Runs). Widths at least 34% shorter (target 30%). Captures approved as baselines (item 96) |

## Open checks

- **⌘C on a reader selection** (item 89): whether the Edit menu's Copy pastes plain text like the bar's Copy (B4). Needs a person with a real key press.
- **Native `title` tooltips in Linen's WebView**: how they look and how long they wait, so the bar's tooltips don't feel out of place beside the window's icon buttons.
- **The lightbulb at 16 px** next to Search and Look Up, in Paper, Sepia and the inverted Night bar (item 94): from the Step 4 captures.

## Runs

- **2026-10-13, Step 1.** `pnpm check`, `pnpm lint`, `pnpm test` (405 passed, 6 skipped), `cargo test the_sample_and_test_packages_install` pass. In the app, the selection checks (`^A[0-9]+-`) pass but for `A5-click-highlight` (“current colour Yellow”) and `A8-notes-tab` (“blue filter shows 0”), which read highlights left by earlier checks and touch nothing Step 1 changed; to be confirmed by the full suite in Step 6.
- **The canary port is taken.** `EP1-no-request-on-select` fails only on its canary (“canary: ” empty): port 8765 is held by another program on this Mac (a local web server for another project), so the harness's canary server can't bind. Every canary check fails while it runs; it wasn't stopped, because it isn't this work's. The rest of EP1 passed (one request, only after Explain was chosen).
- **2026-10-13, Step 2.** `pnpm check`, `pnpm lint`, `pnpm test` (412 passed, 6 skipped). In the app: `SB2-bar-tooltips`, `A1-selection-bar`, `A3-keys`, `EA1-keyboard-path`, `D1-no-uncaught-errors` pass.
- **2026-10-13, Step 3.** `pnpm check`, `pnpm lint`, `pnpm test` (412 passed). In the app: the selection checks as in Step 1 (`A5`, `A8` the same two), `EA1-keyboard-path` after reading the button's name, and the extension checks `^P[0-9]+-` pass but for `P2-hostile-extension` (the canary port) and `P22-font-change-place` (no book open when run without the checks before it).
- **2026-10-13, Step 4.** `cargo test extensions::` (25 passed), `pnpm check`, `pnpm lint`, `pnpm test` (413 passed). In the app, `^(SB[0-9]|LK[0-9]+-|EA1|DX14-no-entry|EX8|D1)`: all pass but `LK7-secrets`, which reached your server on 127.0.0.1:8765 (404) instead of the canary. Visual run: `node tests/visual/compare.mjs` fails `06`, `12-extension-failure`, `14-night-selection` (this work) and `03`, `04`, `05`, `16`, `17-goto`, `rl-05-menu-*` (not this work; see item 96); captures not changed by this work were restored.
- **Widths** (inside the bar, from the captures): no extension 475 → 298 px; with Explain about 518 (its old “⋯” added to 475) → 332 px with its button; Screen 12 with “⋯” 524 → 347 px. At least 34% shorter in each case (target 30%).
- **2026-10-13, Step 6.** `scripts/ci-local.sh --full`: all green. `cargo clippy --features spikes --all-targets -D warnings`: clean. The full suite (`scripts/e2e.sh`, results in `docs/spikes/raw/e2e-reader.json`): 172 of 175 pass, `A5-click-highlight`, `A8-notes-tab`, `P10-selection-actions` and `P22-font-change-place` included (their subset failures were the order of checks). The three failures read the canary, which couldn't bind 127.0.0.1:8765 (“Address already in use”) while another program held the port; they need a run with that port free. After the run the tooltip was allowed to wrap a long name (CSS only); `A1-selection-bar`, `SB2-bar-tooltips`, `SB4-lookup-button` and `D1` pass again on that build.

## Not done

- **Nothing is committed or pushed**, in this repository (branch `selection-bar-compact`) or in `linen-explain` (branch `selection-bar-icon`).
- **The canary checks** (`P2-hostile-extension`, `EP1-no-request-on-select`, `LK7-secrets`) once port 8765 is free.
- **Your actions:** remove Test Lookup in Settings › Extensions (item 93); check ⌘C on a reader selection (item 89); send “Delete highlight” and “‹title› (‹name›)” to the native reviewers (items 88, 93).
- **Translations:** “Delete highlight” and “‹title› (‹name›)” are machine drafts in es, ja, zh-Hans and zh-Hant until the native review.
- **Release order** (item 92): a Linen release with Host API 1.2 first, then Explain 0.2.0 (“Reading Lens”).
