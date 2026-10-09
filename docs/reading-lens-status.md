# Reading Lens status

The state of each stage of `docs/reading-lens-plan.md` (§5), on branch `reading-lens`. Its open questions, items 58–76 in `docs/pending-approvals.md`, were approved on 2026-10-09 (`docs/decisions.md`).

| Stage | State | Why |
|---|---|---|
| 1 Stuck-point diary | Not started (owner) | No code; needs the gate threshold values (item 59, approved in principle) |
| 2a Lookup peek and selection context | Built; see below | — |
| 2b Local dictionaries | Built; see below | O3 and O4 approved 2026-10-10 (items 78, 79) |
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
| Visual baselines for Canvas 2–9, Paper and Night | **Approved 2026-10-10 (item 77)** | 14 peek captures in `docs/visual/app/rl-*.png` (Paper, Night, Sepia once); reviewing them found and fixed a provider menu clipped by the scrolling answer, a source label reading “American English”, and a doubled Restart in “⋯” |
| Page-turn and memory budgets | **Turns pass; memory not measured** | `I6-turn-budget` p95 8 ms within and 3 ms across chapters (final run); `EB1` above. `scripts/perf-memory.sh` was not run on this branch |

### Runs

- **Unit tests:** `pnpm test` 391 passed (6 skipped drafts); `cargo test` 66 passed (1 ignored) plus 2 migration tests; `pnpm check`, `pnpm lint`, `pnpm format:check`, `cargo clippy -D warnings`, `cargo fmt --check` clean.
- **In-app, full suite:** first run 135/141 (five load flakes while other checks ran at the same time, and P10 against a stale sample package); each passed alone. Second run 146/147 (Spike H timing, fixed above). **Final run: 149/149**; after items 73, 75 and 76, 150/150, with Spike H's worst context at 2 ms and `I6-turn-budget` p95 8 ms within and 3 ms across chapters.
- **Found and fixed by the checks:** a removed extension kept its “not responding” state when installed again (`host.svelte.ts`); leaving a book cold did not cancel a pending lookup (`Reader.svelte`); whitespace between indented blocks counted as a sentence, so the sentence before a chunk was lost (`context.ts`, with a unit test).

## Stage 2b — local dictionaries

- **Last updated:** 2026-10-09
- **Before starting:** licence MIT (item 60); O8, O14, O16 approved (items 62, 69, 70); O3 and O4 parked as items 78 and 79 and built to their recommendations. Spike I below. MDX fixtures are generated by `scripts/corpus/mdx.py` (new code from the format notes; no licensed dictionary, DX12): the Rust set in `src-tauri/tests/fixtures/mdx`, the in-app set as `corpus/generated/dict-*`.

### What was built

| Rule | Where |
|---|---|
| DX1 Add dictionary… with its MDD files (`.mdd`, `.1.mdd` …), copied into `Dictionaries/` in the library | `dictionaries/generation.rs` (`sources`, `build`), `prefs/DictionariesPane.svelte`, `Library.dictionaries_dir` |
| DX2 immutable generations: staged, checked (a source changed during the copy fails), validated, indexed, renamed into place; old one kept while an open peek uses it; interrupted imports removed at launch | `dictionaries/generation.rs`, `dictionaries/state.rs` (pins), `dict_lookup`/`dict_release`, store migration 4 |
| DX3 MDX/MDD 1.2 and 2.0, `Encrypted` 0 and 2, zlib or stored, five encodings; refusals with a reason | `dictionaries/mdx.rs`; Settings words each refusal |
| DX4 exact, folded (NFKC, case, apostrophes, hyphens), then labelled suffix rules that never invent a headword | `dictionaries/fold.rs`, `dictionaries/lookup.rs` |
| DX5 `@@@LINK=` up to eight hops, cycles stop; `entry://` links open inside the peek, with Back | `dictionaries/lookup.rs`, `dictionaries/sanitise.rs`, `LookUpPeek.svelte` |
| DX6 sanitised entries (ammonia allowlist, CSS checks), `linen-dict://<generation>/` with `default-src 'none'`, traversal refused, only active or in-use generations served | `dictionaries/sanitise.rs`, `dictionaries/serve.rs`, `lib.rs` |
| DX7 the light card (O4, provisional) | `sanitise.rs` (`CARD_CSS`), the frame's background |
| DX8 sound links say “Audio isn’t available”; audio is never served | `sanitise.rs` (`mark_audio`), `serve.rs` |
| DX9 display only: entries reach the peek's frame and nothing else | no command or Host API returns entry text |
| DX10 entries read by offset; warm look-up p95 1 ms (budget 20 ms) | `mdx.rs` (`record`), `index.rs` |
| DX11 the reader's order; the peek opens on the first dictionary with a hit and lists the others in its menu | `dict_reorder`, `Reader.svelte` (`openLookup`), `providers.ts` |
| DX12 no dictionary data shipped; MIT-compatible crates only (`flate2`, `ripemd`, `encoding_rs`, `ammonia`, `unicode-normalization`) | `Cargo.toml` |
| DX13 caps after decompression; counts and offsets checked against the file | `mdx.rs` |
| DX14 “No entry for … in your dictionaries” when the reader's dictionaries and this Mac's have none; “Explain in context” when Explain applies | `LookUpPeek.svelte`, `Reader.svelte` |
| DX15 web links in entries are inert text | `sanitise.rs` |

### Spike I (§6.5) — passed, with two fallbacks

| Question | Finding |
|---|---|
| Renders a sanitised entry with its CSS, fonts and images | Yes: `DX1-import` finds the MDD image and the dictionary's CSS colour in a capture of the frame. Fonts load by the same rule as images (`font-src linen-dict:`); no font fixture yet |
| Text selectable and copyable | Selectable (no script, no user-select rule); copying uses the system's ⌘C inside the frame. Not checked by the harness, which cannot post keys into a cross-origin frame |
| Follows `entry://` links, with Back | Yes (`I-dictionary-frame`, a real click). **Fallback:** the parent cannot read a cross-origin frame's history, so Back returns to the entry the peek opened on, not one step |
| Sizes itself inside the peek | **Fallback:** with no script in the frame its height can't be measured; the frame has a fixed height (240 px, at most 30% of the window) and scrolls |
| Read by VoiceOver | Not checked (needs a person) |

### Done-when (plan §5, Stage 2b)

| Done-when | State | Evidence |
|---|---|---|
| AC14–AC19 | **Pass** | AC14 `DX1-import`; AC15 `DX6-hostile-entry` (canary empty); AC16 traversal: `serve.rs` and `sanitise.rs` unit tests (a cross-origin frame can't be read in the app); AC17 `DX5-redirects`; AC18 `DX9-never-sent`; AC19 `DX10-budgets` (p95 1 ms, 60,000 entries, three dictionaries) |
| §6.6 Stage 2b checks and §6.7 data safety | **Partly** | Passing: `DX3-encodings`, `DX13-caps`, `DX2-swap-while-open`, `DX1-duplicate`, `DX7-night-card`, `DX4-headword-variants`. By unit test only: `DX8-audio`, `DX15-entry-links`, kill during an import (`recovery_removes_half_built_and_unused_generations`), the migration (`tests/migrations.rs`, v4 fixture). **Not built:** `DX14-no-entry` in the app (this Mac's dictionaries know most words a test can select), `DX1-restore-elsewhere` (generations are folder names inside the library, so the D2 rule holds by construction), disk full |
| Hostile and broken fixtures make no request, run no script, never leave the old generation unusable | **Pass** | `DX6-hostile-entry` (canary), `generation.rs` tests (failures leave only the old generation) |
| Parser fuzzing, 30 minutes | **Pass** (cargo-fuzz, and the stable mutation fuzzer) | `fuzz_mutations` (`mdx.rs`), see Runs. The `cargo fuzz` target is in `src-tauri/fuzz/` and needs nightly (item 80) |
| Canvas 11 and 14 baselines | **Approved 2026-10-10 (item 81)** | `docs/visual/app/rl-11-dictionary-entry-*.png`, `rl-14-settings-dictionaries.png` |
| Memory budget with three dictionaries; warm p95 within DX10 | **p95 passes; memory not measured** | `DX10-budgets`; `scripts/perf-memory.sh` not run on this branch |

### Runs (Stage 2b)

- **Rust:** 93 tests (2 ignored opt-in: external samples, the fuzzer). The reader also reads every valid file TranslateDict's own generator wrote (`LINEN_MDX_SAMPLES=… cargo test reads_external_samples -- --ignored`), its `Encrypted=2` file included, and refuses the damaged ones.
- **Fuzzing (cargo-fuzz, nightly, address sanitizer; item 80):** `cargo +nightly fuzz run mdx -- -max_total_time=1800` in `src-tauri/fuzz`, seeded with the 17 test dictionaries: 4,948,921 runs in 30 minutes, no crash, leak, timeout or cap breach; coverage 1,648 edges, 581 corpus inputs, 327 MB peak. The target compiles `src/dictionaries/mdx.rs` itself, because the Tauri crate does not link under the sanitizer.
- **Fuzzing (stable):** `LINEN_FUZZ_SECS=1800 cargo test --release fuzz_mutations -- --ignored`: 12,863,236 mutated files in 30 minutes, no panic, no record past its cap, slowest 53 ms.
- **In-app:** the 13 Stage 2b checks pass alone and in the full suite: **162/162** on 2026-10-10 (Stage 2a and 2b checks with everything else; the last, `D1-no-uncaught-errors`, included). Two checks first failed in the full suite because of earlier checks (a book left open, links recorded on purpose); they now open their own book and count only their own links.

