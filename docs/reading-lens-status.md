# Reading Lens status

The state of each stage of `docs/reading-lens-plan.md` (§5), on branch `reading-lens`. Its open questions, items 58–76 in `docs/pending-approvals.md`, were approved on 2026-10-09 (`docs/decisions.md`).

| Stage | State | Why |
|---|---|---|
| 1 Stuck-point diary | Not started (owner) | No code; needs the gate threshold values (item 59, approved in principle) |
| 2a Lookup peek and selection context | Built; see below | — |
| 2b Local dictionaries | Not started | Licence chosen (MIT, item 60); O8, O14 and O16 approved; O3 and O4 (the Dictionaries pane, the light card) are still to be decided |
| 2d Secrets, optional hosts, options pages | Not started | Needs Gate 0 (Stage 1) and Spike J |
| 3–5 | Not started | After 2d, and private (Explain) |

## Stage 2a — lookup peek and selection context

- **Last updated:** 2026-10-09
- **Before starting:** Spike H, below. O9–O13 and O15 were built to the plan's recommendations and approved on 2026-10-09 (items 63–68). The English strings are approved; the four translations wait for the native reviewers (item 74).

### What was built

| Rule | Where |
|---|---|
| LK1 lookups contribution, “⋯” and ⌘K items, peek placement and pointer, one layer (S2), the looked-up range marked | `extensions/manifest.rs` (`lookups`, `onLookup:`), `host.svelte.ts` (`lookups`, `lookup`), `SelectionBar.svelte`, `App.svelte` + `lib/lookup/commands.ts`, `LookUpPeek.svelte`, `engine.ts` (`setLookupMark`) |
| LK2 fixed fields, validator, core-owned labels; EQ1, EQ2 | `lib/lookup/result.ts`, `lib/lookup/providers.ts` (`sourceLabel`) |
| LK3 pending state with the sentence underlined | `LookUpPeek.svelte` |
| LK4 invocation IDs, cancel (`cancel` to the Worker, `signal` aborts), stale answers dropped | `host.svelte.ts` (`lookup`, `cancelLookup`), `worker.js`, `Reader.svelte` (`ask`, `serial`) |
| LK5 selection context from the DOM range, `Intl.Segmenter`, 1,200-character cap, chunks and Scroll views, markers and soft hyphens out | `lib/lookup/context.ts`, `lib/search/extract.ts` (`skip`) |
| LK9 `book.lang`, `selection.sentences` | `lib/extensions/when.ts`, `extensions/when.rs` |
| LK10 failure states and actions, 10 s budget, suspended providers | `LookUpPeek.svelte`, `Reader.svelte` |
| LK11 provider and language menu; LK15 plain label for one provider | `lib/lookup/providers.ts`, `LookUpPeek.svelte` |
| LK12 close triggers | `Reader.svelte` (location, `relayout`, `active`, clicks), `lib/reader/state.ts` (Navigator) |
| LK13 no-room placement | `LookUpPeek.svelte` |
| LK14 lookups on a clicked highlight | `SelectionBar.svelte`, `Reader.svelte` (`currentTarget`) |
| EA1–EA3 dialog, live region, Tab and Esc, `lang`, reduced motion, forced colours | `LookUpPeek.svelte`, `Reader.svelte` |
| Host API 1.1 docs | `docs/extensions/README.md`, `docs/extensions/linen.d.ts` |
| Test lookup provider; sample Dictionary gains a lookup | `src-tauri/tests/fixtures/ext-packages/lookup`, `examples/extensions/dictionary` |

### Deviations from the plan

- **The core “Look up” is this Mac's dictionaries.** The plan predates 1.1's Look Up (item 61). Linen's MDX dictionaries join in Stage 2b.
- **The sample Dictionary keeps its Navigator tab** and adds a lookup (“Free Dictionary”) beside it, so the existing P10 checks still cover selection actions and tabs.
- **Fixed-layout books** keep the Mac's Look Up; only extension lookups are off there (item 67).
- **The first-request notice (Canvas 8, EX8)** is LK2's `notice` status (item 76): the provider gives the title, text and host; Linen draws Continue and Not now, and Continue asks again with `acknowledged: true`. Check `EX8-notice`.
- **Explain options… and key errors' “Change key”** wait for Stage 2d; a key error shows no action in 2a.
- **The selection bar's focus ring** is the bar's ink (item 73), 7:1 or more in every theme, where the accent was 2.24:1 on Paper and 1.90:1 at Night.

### Spike H (§6.5) — passed

`H-selection-context`: WebKit's `Intl.Segmenter` gives usable sentences over foliate ranges (inline markup, an abbreviation, a quote, CJK), and the LK5 unit set passes in Vitest (jsdom) and these cases in the app. Per selection: 1 ms or less once the chapter's text is read. The first lookup in a chapter took 24 ms when it also read the chapter, so the reader now reads it (`warmContext`) in idle time when the reader selects.

### Done-when (plan §5, Stage 2a)

Machine: Mac14,3 (M2, 8 GB), macOS 14.6.1, on the current desktop. The suite's last check, `D1-no-uncaught-errors`, passed in every run.

| Done-when | State | Evidence |
|---|---|---|
| AC1–AC8, AC10–AC13 with the test provider, 20 of 20 where stated | **Pass** | AC1 `EP1-no-request-on-select` (net.fetch spy and canary); AC2 `LK1-menu-items` (when-clause, 41 words, safe mode, ⌘K); AC3 `LK1-peek-placement` (within a frame, below with pointer, above at the page foot; Pages and Scroll); AC4 `LK5-repeated-string`; AC5 `LK5-context-bounds`; AC6 `LK4-cancel-stale` (20 of 20, and the provider heard 20 cancellations); AC7 `LK4-position-integrity` (20 runs); AC8 `EQ1-labels`; AC10 `LK10-failure-states`; AC11 `EA1-focus-and-announce` (role and label, polite live region, Tab and Esc, axe, 7:1 in Paper, Sepia and Night); AC12 `EB1-budgets-pending` (turn p95 3.0 ms with a request pending); AC13 `D1-no-uncaught-errors` |
| The §6.6 Stage 2a checks, alone and in the full suite | **Partly** | Built and passing: `LK14-highlight-lookup`, `LK13-no-room`, `LK12-close-triggers` (resize, ⌘+, Navigator, page turn, a jump, ⌘L; theme keeps it), `LK1-fixed-layout`, `LK15-single-provider`, `LK5-chunk-boundary`, `LK5-context-cleanup`, `LK4-rapid`, `S14-warm-cancel`, `N5-quit-pending`, `EP3-context-excludes`, `EP5-crashlog-clean`. **Not built:** `LK9-language-fallback` (the corpus has no book without a language or with `und`; covered by unit tests), `LK1-layouts` (spread, vertical, right-to-left), `LK1-touch`, `EA1-keyboard-path` (F7 caret path), `EA3-reduced-motion` and `X7-peek-forced-colours` (the CSS is in place; the harness cannot switch these media queries) |
| Visual baselines for Canvas 2–9, Paper and Night | **Captured; waiting for approval (item 77)** | 14 peek captures in `docs/visual/app/rl-*.png` (Paper, Night, Sepia once); reviewing them found and fixed a provider menu clipped by the scrolling answer, a source label reading “American English”, and a doubled Restart in “⋯” |
| Page-turn and memory budgets | **Turns pass; memory not measured** | `I6-turn-budget` p95 8 ms within and 3 ms across chapters (final run); `EB1` above. `scripts/perf-memory.sh` was not run on this branch |

### Runs

- **Unit tests:** `pnpm test` 391 passed (6 skipped drafts); `cargo test` 66 passed (1 ignored) plus 2 migration tests; `pnpm check`, `pnpm lint`, `pnpm format:check`, `cargo clippy -D warnings`, `cargo fmt --check` clean.
- **In-app, full suite:** first run 135/141 (five load flakes while other checks ran at the same time, and P10 against a stale sample package); each passed alone. Second run 146/147 (Spike H timing, fixed above). **Final run: 149/149**; after items 73, 75 and 76, 150/150, with Spike H's worst context at 2 ms and `I6-turn-budget` p95 8 ms within and 3 ms across chapters.
- **Found and fixed by the checks:** a removed extension kept its “not responding” state when installed again (`host.svelte.ts`); leaving a book cold did not cancel a pending lookup (`Reader.svelte`); whitespace between indented blocks counted as a sentence, so the sentence before a chunk was lost (`context.ts`, with a unit test).
