# Reading Lens status

The state of each stage of `docs/reading-lens-plan.md` (§5), on branch `reading-lens`. Its open questions, items 58–76 in `docs/pending-approvals.md`, were approved on 2026-10-09 (`docs/decisions.md`).

| Stage | State | Why |
|---|---|---|
| 1 Stuck-point diary | Ready to start (owner) | No code; thresholds set 2026-10-10. Guide and coding script: `docs/reading-lens-stage1.md`, `scripts/reading-lens/g0.py` |
| 2a Lookup peek and selection context | Built; see below | — |
| 2b Local dictionaries | Built; see below | O3 and O4 approved 2026-10-10 (items 78, 79) |
| 2d Secrets, optional hosts, options pages | Built; see below | Gate 0 waived by the owner 2026-10-10 (not passed); Spike J passed. Canvas 12–13 baselines wait on item 82 |
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
- **“Explain in context” names the lookup it opens** (“‹title› in context”), found by the full suite: with the sample Dictionary installed first, the button opened its “Free Dictionary” lookup under Explain's name. For Explain it still reads “Explain in context”.
- **The selection bar's focus ring** is the bar's ink (item 73), 7:1 or more in every theme, where the accent was 2.24:1 on Paper and 1.90:1 at Night.

### Spike H (§6.5) — passed

`H-selection-context`: WebKit's `Intl.Segmenter` gives usable sentences over foliate ranges (inline markup, an abbreviation, a quote, CJK), and the LK5 unit set passes in Vitest (jsdom) and these cases in the app. Per selection: 1 ms or less once the chapter's text is read. The first lookup in a chapter took 24 ms when it also read the chapter, so the reader now reads it (`warmContext`) in idle time when the reader selects.

### Done-when (plan §5, Stage 2a)

Machine: Mac14,3 (M2, 8 GB), macOS 14.6.1, on the current desktop. The suite's last check, `D1-no-uncaught-errors`, passed in every run.

| Done-when | State | Evidence |
|---|---|---|
| AC1–AC8, AC10–AC13 with the test provider, 20 of 20 where stated | **Pass** | AC1 `EP1-no-request-on-select` (net.fetch spy and canary); AC2 `LK1-menu-items` (when-clause, 41 words, safe mode, ⌘K); AC3 `LK1-peek-placement` (within a frame, below with pointer, above at the page foot; Pages and Scroll); AC4 `LK5-repeated-string`; AC5 `LK5-context-bounds`; AC6 `LK4-cancel-stale` (20 of 20, and the provider heard 20 cancellations); AC7 `LK4-position-integrity` (20 runs); AC8 `EQ1-labels`; AC10 `LK10-failure-states`; AC11 `EA1-focus-and-announce` (role and label, polite live region, Tab and Esc, axe, 7:1 in Paper, Sepia and Night); AC12 `EB1-budgets-pending` (turn p95 3.0 ms with a request pending); AC13 `D1-no-uncaught-errors` |
| The §6.6 Stage 2a checks, alone and in the full suite | **Partly** | Built and passing: `LK14-highlight-lookup`, `LK13-no-room`, `LK12-close-triggers` (resize, ⌘+, Navigator, page turn, a jump, ⌘L; theme keeps it), `LK1-fixed-layout`, `LK15-single-provider`, `LK5-chunk-boundary`, `LK5-context-cleanup`, `LK4-rapid`, `S14-warm-cancel`, `N5-quit-pending`, `EP3-context-excludes`, `EP5-crashlog-clean`. Added 2026-10-10: `EA1-keyboard-path` (F7, ⇧→, F6, the arrows to Look Up, Tab into the peek, Esc back; activating the focused button stands in for Enter, which a synthetic key event cannot do) and `LK1-layouts` (a two-page spread, a right-to-left book, vertical writing). **Not built:** `LK9-language-fallback` (the corpus has no book without a language or with `und`; covered by unit tests), `LK1-touch` (the harness cannot post touches), `EA3-reduced-motion` and `X7-peek-forced-colours` (the CSS is in place; the harness cannot switch these media queries) |
| Visual baselines for Canvas 2–9, Paper and Night | **Approved 2026-10-10 (item 77)** | 14 peek captures in `docs/visual/app/rl-*.png` (Paper, Night, Sepia once); reviewing them found and fixed a provider menu clipped by the scrolling answer, a source label reading “American English”, and a doubled Restart in “⋯” |
| Page-turn and memory budgets | **Pass** (memory: see Stage 2b's budget below) | `I6-turn-budget` p95 8 ms within and 3 ms across chapters (final run); `EB1` above. `scripts/perf-memory.sh` was not run on this branch |

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
| §6.6 Stage 2b checks and §6.7 data safety | **Partly** | Passing: `DX3-encodings`, `DX13-caps`, `DX2-swap-while-open`, `DX1-duplicate`, `DX7-night-card`, `DX4-headword-variants`. By unit test only: `DX8-audio`, `DX15-entry-links`, kill during an import (`recovery_removes_half_built_and_unused_generations`), the migration (`tests/migrations.rs`, v4 fixture). `DX14-no-entry` (added 2026-10-10: a word neither the reader's dictionaries nor this Mac's know, with and without Explain). **Not built:** `DX1-restore-elsewhere` (generations are folder names inside the library, so the D2 rule holds by construction), disk full |
| Hostile and broken fixtures make no request, run no script, never leave the old generation unusable | **Pass** | `DX6-hostile-entry` (canary), `generation.rs` tests (failures leave only the old generation) |
| Parser fuzzing, 30 minutes | **Pass** (cargo-fuzz, and the stable mutation fuzzer) | `fuzz_mutations` (`mdx.rs`), see Runs. The `cargo fuzz` target is in `src-tauri/fuzz/` and needs nightly (item 80) |
| Canvas 11 and 14 baselines | **Approved 2026-10-10 (item 81)** | `docs/visual/app/rl-11-dictionary-entry-*.png`, `rl-14-settings-dictionaries.png` |
| Memory budget with three dictionaries; warm p95 within DX10 | **Pass** | `scripts/perf-memory.sh 20 dictionaries` (2026-10-10, M2 8 GB, macOS 14.6.1): a 100 MB book after 50 pages, with three dictionaries (one of 60,000 entries) and an entry open: RSS median 293 MB, p95 345 MB; footprint median 362 MB, p95 368 MB (budget 400). Without dictionaries, the same day: RSS p95 336 MB, footprint p95 352 MB, so dictionaries add about 9 MB RSS and 16 MB footprint at p95. Warm look-up p95 1 ms (`DX10-budgets`) |

### Runs (Stage 2b)

- **Rust:** 93 tests (2 ignored opt-in: external samples, the fuzzer). The reader also reads every valid file TranslateDict's own generator wrote (`LINEN_MDX_SAMPLES=… cargo test reads_external_samples -- --ignored`), its `Encrypted=2` file included, and refuses the damaged ones.
- **Fuzzing (cargo-fuzz, nightly, address sanitizer; item 80):** `cargo +nightly fuzz run mdx -- -max_total_time=1800` in `src-tauri/fuzz`, seeded with the 17 test dictionaries: 4,948,921 runs in 30 minutes, no crash, leak, timeout or cap breach; coverage 1,648 edges, 581 corpus inputs, 327 MB peak. The target compiles `src/dictionaries/mdx.rs` itself, because the Tauri crate does not link under the sanitizer.
- **Fuzzing (stable):** `LINEN_FUZZ_SECS=1800 cargo test --release fuzz_mutations -- --ignored`: 12,863,236 mutated files in 30 minutes, no panic, no record past its cap, slowest 53 ms.
- **In-app:** the 13 Stage 2b checks pass alone and in the full suite: **165/165** on 2026-10-10 (with `EA1-keyboard-path`, `LK1-layouts` and `DX14-no-entry`) (Stage 2a and 2b checks with everything else; the last, `D1-no-uncaught-errors`, included). Two checks first failed in the full suite because of earlier checks (a book left open, links recorded on purpose); they now open their own book and count only their own links.

## Stage 2d — secrets, optional hosts and options pages

- **Last updated:** 2026-10-10
- **Before starting:** Gate 0 was waived by the owner on 2026-10-10, not passed (`docs/decisions.md`). Spike J ran first: `docs/spikes/j-keychain.md`. Ad-hoc-signed builds read each other's Keychain items silently, so updates and reinstalls keep keys, and the Keychain protects them at the level of the account only. LK7 keeps the Keychain and claims no more than that.

### What was built

| Rule | Where |
|---|---|
| LK6 `optionalPermissions` (exact network hosts, no wildcards, not also required); `linen.permissions.request/has`; Linen's host sheet (Canvas 13); grants kept across updates while still listed; Remove in Settings › Extensions cancels what the extension was doing | `extensions/manifest.rs`, `extensions/registry.rs` (`grant_optional`, `revoke_optional`, `install`), `ext_commands.rs` (`extension_grant`, `extension_revoke`), `worker.js`, `lib/extensions/access.ts`, `components/HostSheet.svelte`, `host.svelte.ts` (`load`, `unload`) |
| LK7 keys: typed into a native macOS dialog (an `NSAlert` with a secure field), so the value never enters the WebView; kept in the Keychain (`security-framework`), service `app.linen.extension-key`; SQLite keeps only names, hosts and labels (migration 5, `extension_secrets`); `linen.secrets.request/has`, nothing returns a value | `extensions/secrets.rs`, `native.rs` (`ask_secret`), `ext_commands.rs` (`extension_secret_request`, `Vaults`), `store.rs` |
| LK7 `net.fetch` `headers` and `auth`: the key only when host and port equal the key's host, over https (http only to localhost and 127.0.0.1); redirects never followed; Authorization, Cookie, Host, the key headers, Content-Length, Connection, Transfer-Encoding, TE, Upgrade, Proxy-* and Sec-* refused | `extensions/secrets.rs` (`may_send`, `checked_headers`), `extensions/registry.rs` (`net_fetch`) |
| LK7-keychain-denied: a key that can't be read is one refusal the host recognises; the peek says so, with Open options, and nothing is sent | `ext_commands.rs` (`KEY_UNAVAILABLE`), `host.svelte.ts` (`KeyUnavailable`), `LookUpPeek.svelte`, `Reader.svelte` |
| LK8 `contributes.options`: Options… in the extension's row, the page in a Linen dialog on the extension's own origin with the style kit; `linenUi.call` reaches storage, permissions and secrets through the frame only; Open options in the peek for a refused key | `prefs/OptionsDialog.svelte`, `prefs/ExtensionsPane.svelte`, `prefs/Preferences.svelte`, `app/settingsWindow.ts`, `ui.js` |
| EP2 requests only to granted hosts, enforced by the core; EP4 “Sends to ‹host›” with Remove | `registry.rs` (`net_fetch`), `ExtensionsPane.svelte` |
| Removing an extension deletes its keys (Keychain and rows) and cancels its pending request; its host sheet answers itself | `ext_commands.rs` (`extension_remove`), `host.svelte.ts`, `App.svelte`, `Reader.svelte` |
| Host API 1.1 docs | `docs/extensions/README.md` (“Optional hosts and keys”), `docs/extensions/linen.d.ts` |
| Test provider | `src-tauri/tests/fixtures/ext-packages/keyed` (an optional host, a key, an options page, a slow request) |

Test builds with a throwaway `LINEN_DATA_DIR` keep keys in memory, so the in-app checks never touch the person's Keychain. The harness can't type into a modal `NSAlert`, so it hands over the key the dialog would return (`hooks.secretFor`, `spike_secret_save`), and `spike_vault_deny` makes the memory vault refuse as a locked Keychain would.

### Deviations from the plan

- **The key is typed into a native dialog, not an HTML sheet.** Canvas 13 draws the key sheet in the page. A field in the WebView would put the value in the WebView, which LK7 rules out, so Linen asks with an `NSAlert` and a secure field, with the canvas's title and wording. It has no capture (the harness can't capture a modal alert).
- **The host sheet quotes the extension's purpose** (“‹name› says: …”) under Linen's own wording, because Linen can't know what an arbitrary extension sends. For the same reason, “Sends to” reads “what you choose to send”, where Canvas 12 says “the text you explain” for Explain.
- **The options dialog is a frame and Done.** Canvas 12's provider, model, language, key and study-log rows, and Test connection, are Explain's own page (Stage 3); Linen draws only the dialog around it.
- **Revoking a host** cancels the extension's running work (its pending lookup ends and the peek closes) instead of stopping it as a failure.

### Done-when (plan §5, Stage 2d)

| Done-when | State | Evidence |
|---|---|---|
| AC9, with a redirect and a different-host attempt | **Pass** | `LK7-secrets`: the key reaches its own host (canary saw `[key:authorization]`), a 302 is returned and not followed, a request with the key to another allowed host is refused before it is sent |
| A test extension can't read a secret, send one to another host, or set Authorization, Cookie or Host | **Pass** | `LK7-secrets` (`linen.secrets` has only `has` and `request`; the key's value is in no storage, request record, page, row or crash log); `secrets.rs` and `registry.rs` unit tests |
| The Stage 2d checks in §6.6 | **Pass** | `LK6-deny-revoke` (deny, allow, remove while a request is pending, Remove in Settings); `LK6-remove-extension`; `LK7-keychain-denied`; `LK8-options-page`. “An update asking for a new host asks again”: unit test `optional_hosts_are_granted_revoked_and_kept_across_updates` |
| Canvas 12 and 13 baselines approved | **Waiting** (item 82) | `docs/visual/app/rl-13-host-sheet.png`, `rl-12-settings-sends-to.png`, `rl-12-options-dialog.png`, `rl-09-error-keychain-paper.png` |

### Runs (Stage 2d)

Mac14,3 (M2, 8 GB), macOS 14.6.1, on the current desktop, 2026-10-10.

- **Unit tests:** `cargo test` 97 passed (3 ignored opt-in) plus the 2 migration tests (schema v5 fixture); new tests in `secrets.rs`, `manifest.rs` (`optional_hosts_and_options_pages_are_checked`) and `registry.rs` (`optional_hosts_are_granted_revoked_and_kept_across_updates`, `the_sample_and_test_packages_install`). `pnpm test` 395 passed (6 skipped drafts). `pnpm check`, `pnpm lint`, `pnpm format:check`, `cargo clippy -D warnings` (with and without `spikes`) and `cargo fmt --check` clean.
- **In-app, full suite:** **170/170**, the last, `D1-no-uncaught-errors`, included. `LK6-deny-revoke` then gained the pending-request case; with it, the Stage 2d checks, `EP1-no-request-on-select` and both D1 checks passed again (8/8).
- **Visual:** `scripts/e2e.sh v` captured the four Stage 2d screens. Against `docs/visual/baselines/`, ten older screens differ by 0.1–1.2% (the selection colour of an active window against the inactive Desktop 2 baselines, and glyph edges; item 77 describes the same). Stage 2d does not touch them, and their captures were not committed.
- **Found and fixed while writing the checks:** removing an extension showed “stopped responding” in the peek instead of cancelling, and its host sheet stayed open.
