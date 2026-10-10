# Reading Lens — implementation plan

Status: draft for owner review, 2026-10-09; revision 2 the same day adds the edge cases and test gaps found in review (O8–O17, LK12–LK15, DX13–DX15, §6.6–6.10). Nothing here is built. Values marked (prov.) are provisional until the owner signs them off, as in `docs/implementation-plan.md` §1.2.

Reading Lens adds a **lookup peek** to Linen: the reader selects text and asks for it to be looked up in a local dictionary or explained by Explain, an installable extension, in a panel under the selection. This plan turns the three design documents into stages, work, tests and gates. It doesn’t change `docs/implementation-plan.md`, which stays read-only; this file is the rule register that code and tests for this work cite (`// LK4: …`).

## 1. Sources and scope

### 1.1 Design sources

| Source | What it gives this plan | Cited as |
|---|---|---|
| Spec page, “Reading Lens for Linen” (private): https://claude.ai/artifact/6w5wLAxnDijmEnQfC8iUfL | The assessment, the stages and gates, every rule (LK, EX, EP, EQ, EA, EB, DX) and acceptance criterion (AC), the D1–D11 mockups | **Spec §n** |
| Design canvas, “Linen Lookup Peek” (private): https://claude.ai/artifact/1hNkpLTRs5oetjQ3CsHZt4 | 16 screens at full size, linked for play | **Canvas n** |
| Design system, “Linen” (private): https://claude.ai/artifact/HoeVrENXkSwrGqJVgxVKy4 | Tokens from `src/lib/theme/tokens.ts`, the brand book, and the SelectionBar, FootnotePeek and LookupPeek guidelines | **DS LookupPeek** etc. |
| `docs/decisions.md`, “Decided 2026-10-09 (Reading Lens, Stage 0)” | Host, AI, providers, distribution, MDX, peek placement, layout, menu, speak buttons, solid surfaces | **Decision 10-09** |

Canvas screens: 1 Select and choose Explain · 2 Pending · 3 Answer · 4 More · 5 Provider and language menu · 6 What was sent · 7 Needs more context · 8 First request to a host · 9 When it can’t answer · 10 Night · 11 Dictionary entry · 12 Settings › Extensions › Explain options · 13 Host permission and key · 14 Settings › Dictionaries · 15 Selection bar menu, Night · 16 Provider menu, Night.

### 1.2 What ships where

| Piece | Where the code lives | Ships in releases |
|---|---|---|
| Lookup peek, selection context, Host API 1.1 additions (LK) | This repository | Yes, from Stage 2 |
| Local MDX/MDD dictionaries (DX) | This repository, Rust core | Yes, from Stage 2 |
| Secrets, optional hosts, authorised fetch, options pages (LK6–LK8) | This repository | Yes, but only after Gate 0 passes |
| Explain extension (EX) | A separate **private** repository | No. Personal use until Gate 3 (Decision 10-09) |
| Test lookup provider used by the e2e checks | `src-tauri/tests/fixtures/ext-packages/` | Spike builds only |

This repository never holds Explain’s code, prompts or provider adapters before Gate 3, and never holds a licensed dictionary (DX12).

### 1.3 Out of scope

Dictionary text in model prompts and “dictionary supports” labels (Stage 5, only on evidence); proactive hints; chat; whole-book upload; vocabulary review; streaming; translating paragraphs; fixed-layout books; speak buttons; see-through or blurred surfaces; Windows and Linux.

## 2. Decisions this plan rests on

Settled (Decision 10-09): Linen hosts the work; AI only as an installable extension; provider presets DeepSeek, Qwen international, local Ollama, OpenAI, Anthropic and Gemini, each host granted by the reader; personal use until Gate 3; MDX/MDD read in the core, display only; the peek opens below the selection, laid out after the macOS Books Translate popover in Linen’s tokens; a menu switches provider and language; no speak buttons; solid surfaces only.

Open, with the stage that needs each (they go into `docs/pending-approvals.md` when that stage starts):

| # | Question | Needed by | Recommendation |
|---|---|---|---|
| O1 | Gate thresholds X and Y for G0, the severe-error bound for G2, and the margins for G3 | Stage 1 start | Set before any data is seen; the spec suggests none, by design |
| O2 | A licence for Linen (the repository has none) | Stage 2b start | Choose before taking a parser dependency or porting BSD-3 code |
| O3 | A Dictionaries section in Settings (amends G2) | Stage 2b | Approve as drawn on Canvas 14 |
| O4 | Dictionary entries on a light card in every theme (DX7) | Stage 2b | Approve; dictionary CSS assumes a light page |
| O5 | A shortcut for Look up | Stage 2a | None by default; avoid ⌃⌘D, which macOS uses for Look Up |
| O6 | Whether a copy of the Oxford data may be displayed in Linen | Before using it | The owner’s call; Linen’s MDX support doesn’t depend on it |
| O7 | The selection bar’s focus ring (2.24:1 Paper, 1.90:1 Night, under 3:1) | Any time | Fix separately with a bar-specific ring colour; it affects today’s app |
| O8 | What the peek shows when no dictionary has an entry | Stage 2b | “No entry for ‘…’ in your dictionaries”, plus “Explain in context” when Explain is installed (DX14) |
| O9 | Whether a clicked highlight offers Look up and Explain. Today `SelectionBar.svelte:159` shows extension actions only for new selections | Stage 2a | Yes, with Delete keeping its place (LK14) |
| O10 | Placement when neither side has room (large text, short window) | Stage 2a | The side with more room; the answer scrolls (LK13) |
| O11 | What else closes the peek besides Esc, a click outside, a page turn and a new selection | Stage 2a | Scrolling, resizing, text-size or Aa changes, opening the Navigator, jumps and leaving the book close it and cancel; a theme change doesn’t (LK12) |
| O12 | Books with no language, `und`, or a wrong one | Stage 2a | Report missing and `und` as `""` (LK9) and let Explain’s condition accept `""` as well as `en` |
| O13 | Look up in fixed-layout books | Stage 2a | Off in v1 for both providers, as Explain already is; revisit with fixed-layout zoom (E2, I17) |
| O14 | Web links inside dictionary entries | Stage 2b | Inert text (DX15); opening a browser from dictionary content is a tracking path |
| O15 | The provider label when only one provider applies | Stage 2a | Plain text, no chevron (LK15) |
| O16 | MDX record encodings | Stage 2b | UTF-8, UTF-16LE, GBK, GB18030 and Big5, decoded with a reviewed crate (candidate: `encoding_rs`; licence checked at review) |
| O17 | Local-model failure wording (Ollama not running, model missing) | Stage 3 | Specific messages in Explain, for example “Ollama isn’t running on this Mac · Try again” and “The model ‘…’ isn’t installed · Open options” |

## 3. Rule register

Each rule has an ID; tests name the ID they assert. The spec page has the long form and rationale; this register is the short form code cites. Stage numbers refer to §5.

### 3.1 Lookup peek and Host API 1.1 — LK

| ID | Rule | Source | Stage |
|---|---|---|---|
| LK1 | A manifest may contribute `lookups: [{ id, title, when }]`; Linen’s own dictionaries add a core “Look up”. Both appear in the selection “⋯” menu and ⌘K. The core draws the peek under the selection’s last line (above it near the page foot) with a pointer at the selection and `space-12` of clearance; it never covers the selected line. Width min(440 px, window − 32 px), height at most 50% of the window, `popover` surface, solid, one layer at a time (S2). Layout: the source word, a hairline, the answer under its provider menu, a footer of actions. The looked-up range keeps the active search mark while the peek is open | Spec §4.5; Canvas 1–11; DS LookupPeek; Decision 10-09 | 2a |
| LK2 | Providers return plain text in fixed fields `{ status: ok \| needs_context \| error, headword, term?, meaning, qualifier?, details?: [{ label, text }], source: { kind: ai \| dictionary, name, model? }, sent? }`; the core renders them. The source label comes from a closed set the core owns. Only the core dictionary provider may return sanitised HTML (DX6) | Spec §4.5 | 2a |
| LK3 | Pending: the core shows the source word, the selected sentence with the selection underlined, and “Asking ‹provider›…” as soon as the peek opens | Canvas 2 | 2a |
| LK4 | Each invocation has an ID bound to the book, the CFI and the peek. Closing cancels it (the Worker receives `cancelled`); a result for a closed or different ID is dropped without a message | Spec §4.5 | 2a |
| LK5 | `book.selection()` gains `context: { before, sentence, after, paragraph, chapter }`, derived from the DOM range with `Intl.Segmenter` sentence boundaries, capped at 1,200 characters (prov.), never crossing the chapter but continuing across L16 chunks and B8 scroll views inside it, never found by string search. Footnote markers and soft hyphens are removed from the context | Spec §4.5; review | 2a |
| LK6 | `optionalPermissions` (network hosts only) in the manifest; `linen.permissions.request(host)` opens a Linen sheet; Settings › Extensions lists granted hosts with Remove | Canvas 12, 13 | 2d |
| LK7 | `linen.secrets.request(name, { host, label })` opens a Linen sheet with a secure field and stores the value in the macOS Keychain; the extension never reads it. `net.fetch(url, { headers, auth: { secret, scheme } })` adds the secret only when the URL’s host equals the secret’s host; `scheme` is `bearer`, `x-api-key` or `x-goog-api-key`. Other headers are allowed except Authorization, Cookie, Host, Proxy-* and the secret schemes | Canvas 13 | 2d |
| LK8 | `contributes.options: { page }` adds Options… to the extension’s row, rendered in a sandboxed frame with the style kit | Canvas 12 | 2d |
| LK9 | `when` clauses gain `book.lang` (primary subtag, lowercase; `""` when the book has no language or `und`, prov., O12) and `selection.sentences` | Spec §4.5; review | 2a |
| LK10 | A lookup keeps the 10 s work budget (P6); the peek shows the timeout state; a stuck provider follows the watchdog and suspension rules | Canvas 9 | 2a |
| LK11 | The label above the answer is a menu naming the provider and language. It lists Explain in each language the reader uses, every installed dictionary with an entry for the selection, and Explain options…. Choosing Explain sends a request only then; a dictionary’s footer offers “Explain in context” | Canvas 5, 11, 16; Decision 10-09 | 2a |
| LK12 | (prov., O11) Besides Esc, a click outside, a page turn and a new selection, these close the peek and cancel its request: scrolling in Scroll mode, a window resize, a text-size or Aa change, opening the Navigator, any jump (Back, Contents, a search result, a link) and leaving the book. A theme change keeps it open and re-themes it | Review | 2a |
| LK13 | (prov., O10) When neither side has room for the peek at its natural height, it opens on the side with more room, takes that room minus 16 px, and its answer section scrolls; it still never covers the selected line | Review | 2a |
| LK14 | (prov., O9) A clicked highlight offers Look up and lookup extensions in its “⋯” menu, as a new selection does; Delete keeps its place | `SelectionBar.svelte:159`; review | 2a |
| LK15 | (prov., O15) When only one provider applies, the label above the answer is plain text, not a menu | Review | 2a |

### 3.2 Local dictionaries — DX

| ID | Rule | Source | Stage |
|---|---|---|---|
| DX1 | Settings › Dictionaries › Add dictionary… picks an `.mdx`; MDD files with the same base name (including `.1.mdd`, `.2.mdd`) come with it. Files are copied into `Dictionaries/` in the library folder, so one folder stays the whole backup (D2) | Canvas 14 | 2b |
| DX2 | An import builds a new, immutable generation that is validated and fully indexed before it replaces the old one. Failure, cancel or low disk space leaves the old one working; a source file that changes during the copy fails the import. A look-up and the resources it loads stay on the generation it started with; a replaced generation is deleted only when no open entry uses it | Spec §4.6; review | 2b |
| DX3 | MDX and MDD 1.2 and 2.0, `Encrypted` 0 and 2, zlib or uncompressed blocks; record text in UTF-8, UTF-16LE, GBK, GB18030 or Big5 (prov., O16). Registration-protected files and unsupported compression are refused before activation with a specific message, as DRM books are (B9) | Spec §4.6 | 2b |
| DX4 | Exact match first, then case-folded; duplicate headwords in source order. If neither hits: the dictionary’s redirects, then labelled English suffix rules (“for ‘invalidates’”). No remote fallback | Canvas 11 | 2b |
| DX5 | `@@@LINK=` redirects resolve up to eight hops with cycle detection; `entry://` links open inside the peek, with Back | Spec §4.6 | 2b |
| DX6 | The core sanitises entry HTML: no scripts, event handlers, forms, frames or remote URLs, including CSS `@import` and `url()`. It renders in a frame inside the peek under `default-src 'none'`; styles, fonts and images load only from `linen-dict://<generation>/`, paths normalised and traversal refused. Per-dictionary override styles are reviewed data | Spec §4.6; L13, L14 | 2b |
| DX7 | The entry sits on a light card in every theme (prov., O4) | Canvas 11 | 2b |
| DX8 | Pronunciation buttons inside entries say “Audio isn’t available” and nothing plays | Spec §4.6 | 2b |
| DX9 | Display only: no Host API returns dictionary text; it never enters a request, an extension, an export or the crash log. Copy copies only text the reader selects inside the entry | Spec §4.6–4.7 | 2b |
| DX10 | Entries are read by offset; nothing loads whole. Warm look-up p95 ≤ 20 ms (prov.); the 400 MB memory budget (§6.4) holds with three dictionaries installed; packages up to 1 GiB (prov.) | Spec §4.6 | 2b |
| DX11 | An ordered list in Settings; the peek opens on the first dictionary with a hit and lists the others that hit in its menu (LK11) | Canvas 5, 14 | 2b |
| DX12 | Linen ships no dictionary data. The parser is permissively licensed (a port of mdict_flutter, BSD-3, or new code from writemdict’s MIT format notes); GPL and AGPL parsers stay out. Test dictionaries are generated; no licensed dictionary is committed | Spec §4.6 | 2b |
| DX13 | (prov.) Size caps after decompression: header 1 MB, block 16 MB, entry 4 MB, resource 16 MB; key counts and offsets checked against the file’s length. A record over a cap is a data error, never shown or partly shown | TranslateDict limits; review | 2b |
| DX14 | (prov., O8) When no dictionary has an entry, the peek says “No entry for ‘…’ in your dictionaries” and, if Explain is installed, offers “Explain in context” | Review | 2b |
| DX15 | (prov., O14) Web links in entries are inert: shown as text, never opened. Only `entry://` links navigate | Review | 2b |

### 3.3 Privacy, honesty, accessibility and budgets — EP, EQ, EA, EB

| ID | Rule | Stage |
|---|---|---|
| EP1 | No network request on selecting, highlighting, copying, searching or opening a dictionary; only when the reader chooses Explain | 2a, 3 |
| EP2 | Requests go only to hosts the reader granted; the core enforces it | 2d |
| EP3 | Context never includes front matter, other chapters, highlights or notes | 2a |
| EP4 | An extension’s row in Settings says “Sends the text you explain to ‹host›” for each granted host | 2d |
| EP5 | The core keeps no record of lookups, and the crash log never contains selection text (D1) | 2a |
| EQ1 | AI answers are labelled “AI explanation · ‹model›”, dictionaries “Dictionary · on this Mac”; never “verified” or “source” | 2a |
| EQ2 | No percentages or confidence scores | 2a, 3 |
| EQ3 | A qualifier that changes the meaning appears in the first layer, never only under More | 2a, 3 |
| EQ4 | `needs_context` says what is missing; any wider context is the reader’s visible choice | 2a, 3 |
| EQ5 | The peek never covers the selected line | 2a |
| EA1 | The peek is a dialog labelled “Explanation of ‘…’” or “Look up ‘…’”; the answer is announced politely; Tab enters it; Esc returns focus to the selection | 2a |
| EA2 | Peek text holds 7:1 in Paper, Sepia and Night (X1); explanations carry their `lang` | 2a |
| EA3 | Under reduced motion the pending indicator is static and fades are 100 ms or less | 2a |
| EB1 | The peek is visible within 100 ms of the action; page turns and the memory budget are unchanged while a request is pending | 2a |

### 3.4 Explain — EX (private package)

EX1–EX10 are in Spec §4.5 and are implemented in the private Explain repository: the lookup and command and their `when` clause (EX1), permissions (EX2), one request per explanation through a per-preset adapter (EX3), the prompt’s contents and limits (EX4, EX5), What was sent (EX6), Copy (EX7), the first-request notice per host (EX8), the session cache (EX9) and the opt-in study log (EX10). This repository only provides the Host API they use and a test provider that imitates them.

## 4. Architecture

### 4.1 Rust core (`src-tauri/src`)

| Area | Change |
|---|---|
| `dictionaries/` (new) | `mdx.rs`: header (UTF-16 XML), key index, record blocks, `Encrypted=2` key-index decryption, zlib; `index.rs`: headword index per generation in SQLite, exact and folded lookups, redirects; `generation.rs`: stage → validate → index → activate → recover (DX2); `sanitise.rs`: entry HTML to safe HTML (DX6); `serve.rs`: the `linen-dict://` scheme; commands `dict_list`, `dict_import`, `dict_cancel_import`, `dict_remove`, `dict_reorder`, `dict_enable`, `dict_lookup` |
| `lib.rs` | Register `linen-dict` with `register_asynchronous_uri_scheme_protocol`, as `linen-book` and `linen-ext` are |
| `store.rs` | A migration adding `dictionaries` (id, name, files, generation, position, enabled, counts, status) and `extension_grants` (extension, host) tables; fixtures per §6.3 |
| `import.rs` (`Library`) | `dictionaries_dir`, resolved by file name inside the current library folder (D2) |
| `secrets.rs` (new, Stage 2d) | Keychain read and write through a reviewed crate (candidates: `security-framework`, `keyring`; licences checked at review). Only names and hosts are stored in SQLite, never values |
| `extensions/manifest.rs`, `extensions/registry.rs`, `ext_commands.rs` | `lookups`, `optionalPermissions` and `options` in the manifest; `net_fetch` gains allowed headers and `auth` (LK7), still host-checked and redirect-free; `permissions_request`, `secrets_request` commands that raise Linen sheets |
| `tauri.conf.json` | `frame-src` and `img-src` gain `linen-dict:` for the entry frame only |

### 4.2 Frontend (`src`)

| Area | Change |
|---|---|
| `reader/engine.ts` | `selectionContext(event)`: sentence, neighbours and paragraph from `SelectionEvent.range` with `Intl.Segmenter` (LK5); a range mark for the looked-up selection |
| `reader/LookupPeek.svelte` (new) | The peek of Canvas 2–11: source section, hairline, answer section with the provider menu, footer, pointer; states pending, answer, more, sent, needs_context, first-use, error; the dictionary frame; placement below with flip (LK1) |
| `reader/Reader.svelte` | Lookup invocation, the `peek` layer (S2) shared with the footnote peek, cancel and stale handling (LK4), Esc and focus order (EA1) |
| `reader/SelectionBar.svelte` | The core “Look up” item and extension lookups in the “⋯” menu |
| `lib/lookup/` (new) | The provider registry (core dictionaries plus extension lookups), the LK2 result type and its validator, the provider-menu model (LK11) |
| `lib/extensions/when.ts` | `book.lang`, `selection.sentences` (LK9) |
| `extensions/host.svelte.ts` | Lookup invoke with an invocation ID, cancel, `book.selection` context, the `permissions` and `secrets` RPCs (Stage 2d) |
| `prefs/DictionariesPane.svelte` (new), `prefs/ExtensionsPane.svelte` | Canvas 14; Options…, granted hosts and the “Sends to” line (Canvas 12, Stage 2d) |
| `lib/commands/registry.ts` | “Look up selection” and extension lookup commands in ⌘K |
| `lib/strings/en.ts` | Every new string, marked PROVISIONAL until approved |
| `docs/extensions/` | README and `linen.d.ts` for Host API 1.1, with `lookups`, `secrets` and `optionalPermissions` marked experimental until 1.2 |

No new design tokens are needed: the peek uses `popover`, `popover-border`, `hairline`, `control-track`, `ink-secondary`, `accent`, `search-active-*` and `shadow-peek`.

## 5. Stages

Sizes use the original plan’s S/M/L. Each stage ends with its Done-when list checked and recorded in a status file, as the original phases did.

### Stage 1 — Stuck-point diary (no code)

**Before starting:** O1 thresholds set.

- The owner and 3–5 readers read their own English technical EPUBs in Linen 0.1.1 for a week. A rose highlight marks “I got stuck”; its note says what they did, whether it worked, and whether the reader’s MDX dictionary had the right sense and listed it first.
- Export the W3C JSON from each reader and code every item with Report 1’s types (L1–L4, S1–S2, K1, X, G), noticed or not.
- Quick check before the week: do Look Up, Bob and Eudic work inside Linen’s book view on ten technical terms?

**Done when:** G0 is decided against the thresholds and recorded in `docs/decisions.md`. If G0 fails, Stages 2a and 2b still go ahead (they are on the 1.1 roadmap); Stages 2d–4 stop.

### Stage 2a — Lookup peek and selection context (M–L) · Canvas 1–11, 15–16

**Before starting:** Spike H (selection context, §6.5) passed. Strings drafted for approval.

- LK1–LK5, LK9–LK15; EP1, EP3, EP5; EQ1–EQ5; EA1–EA3; EB1. O9–O13 and O15 decided first.
- The `lookups` contribution and the core “Look up” item; ⌘K commands.
- `LookupPeek.svelte` in all states, Paper, Sepia and Night, at narrow widths.
- The sample Dictionary extension moves from its Navigator tab to a lookup.
- The test lookup provider (fixtures) with scripted results, latency, errors and `needs_context`.
- Host API 1.1 docs.

**Done when:**
- AC1–AC8, AC10–AC13 pass with the test provider (§6.2), 20 of 20 runs where stated.
- The Stage 2a checks in §6.6 pass, each alone and in the full suite (§6.10).
- Visual baselines for Canvas 2–9 equivalents, built from the test provider, are approved in Paper and Night.
- Page-turn and memory budgets hold (§6.4 of the original plan).

### Stage 2b — Local dictionaries (L) · Canvas 11, 14

**Before starting:** O2 (licence), O3, O4, O8, O14 and O16 decided. Spike I (dictionary frame, §6.5) passed. MDX fixtures generated, including the encodings and caps sets (§6.3).

- DX1–DX15: the Rust `dictionaries/` module, the `linen-dict://` scheme, the sanitiser, the Dictionaries section in Settings.
- The core dictionary provider in the lookup peek, its inflection label and its place in the provider menu.

**Done when:**
- AC14–AC19 pass.
- The Stage 2b checks in §6.6 and the data-safety tests in §6.7 pass.
- The hostile and broken dictionary fixtures in §6.3 make no network request (checked at the canary, §6.10), run no script and never leave the old generation unusable.
- The parser fuzz target (§6.1) runs for 30 minutes (prov.) with no crash, hang or cap breach.
- Canvas 11 and 14 baselines are approved.
- The memory budget holds with three dictionaries installed; warm look-up p95 is within DX10.

### Stage 2d — Secrets, optional hosts and options pages (M) · Canvas 12–13

**Before starting:** Gate 0 passed. Spike J (Keychain under ad-hoc signing, §6.5) passed.

- LK6–LK8; EP2, EP4; the Keychain module; the permission and key sheets; Options… and the “Sends to” line in Settings › Extensions.

**Done when:**
- AC9 passes, including a redirect and a different-host attempt.
- A test extension can’t read a secret, can’t send one to another host, and can’t set Authorization, Cookie or Host itself.
- The Stage 2d checks in §6.6 pass.
- Canvas 12 and 13 baselines are approved.

### Stage 3 — Explain and the offline check (private) · Canvas 2–10

**Before starting:** Stage 2d done; a provider account whose terms allow the use.

- The Explain extension in its private repository, against Host API 1.1: EX1–EX10, the six presets, the first-request notice per host (EX8 wording checked against each provider’s current policy).
- The provider contract tests (§6.9) in the same repository; O17 decided.
- An offline run of at least 60 items drawn from Stage 1’s real stuck points, two or three models, scored blind by two bilingual reviewers.

**Done when:** the contract tests pass for every preset, and G2 passes: severe-misleading rate under the owner’s bound, with its upper limit reported, and reviewer agreement α ≥ .67.

### Stage 4 — Pilot (no code)

- At most three arms (Explain, the same model through copy-paste chat, the reader’s real workaround), 6–8 readers, items rotated, the owner not a participant.

**Done when:** G3 is decided. If it passes, the owner decides whether Explain moves into public distribution, with provider terms and any regulatory review done first.

### Stage 5 — Dictionary evidence (only on evidence)

Not planned. It opens only if the offline set shows dictionary entries beating context alone, with a permitted, sense-segmented source, short candidate lists, a “none of these senses applies” answer and a “candidate entry” label (Spec §4.7).

## 6. Verification

### 6.1 Unit tests

- **Selection context (LK5):** sentences across inline elements, abbreviations, quotes, CJK and mixed text; caps; chapter boundary; L16 chunk boundaries and B8 scroll views; footnote markers and soft hyphens removed; ligatures kept; `<pre>` code, headings, list items, table cells and ruby.
- **Result validator (LK2, EQ1–EQ2):** unknown fields, custom labels and percentages are refused.
- **When clauses (LK9):** `book.lang` from `en`, `en-GB`, `EN-us`, `und` and a missing language; `selection.words` and `selection.sentences` at their boundaries (40 against 41 words).
- **Rust:** MDX header, key index, blocks and `Encrypted=2` against generated fixtures; every encoding in DX3; the DX13 caps, including key counts and offsets past the file’s end; redirects and cycles; the sanitiser against the hostile set in §6.3; path normalisation and traversal; generations, recovery and generation pinning (DX2); `net_fetch` header filtering and secret host matching; secret names isolated per extension and deleted with it.
- **Fuzzing:** a `cargo fuzz` target (nightly toolchain) for the MDX header, key index and record blocks, seeded with the generated fixtures. It runs before Stage 2b closes and after any parser change, not in every CI run.

### 6.2 In-app end-to-end checks (`src/spikes/e2e.ts`)

New checks run against the test lookup provider and generated dictionaries, never a real model or a licensed dictionary. Named by rule, as the existing checks are.

| Check | Asserts | AC |
|---|---|---|
| `EP1-no-request-on-select` | Selecting, highlighting, copying and searching make zero `net.fetch` calls | AC1 |
| `LK1-menu-items` | Look up and lookup extensions appear in “⋯” and ⌘K only when their `when` holds; none in safe mode | AC2 |
| `LK1-peek-placement` | Opens within 100 ms, never intersects the selected line; flips near the page foot; Pages, Scroll, spread, 800–2560 px | AC3 |
| `LK5-repeated-string` | A word three times in a chapter sends the sentence of the selected occurrence each time | AC4 |
| `LK5-context-bounds` | ≤ 1,200 characters, one chapter, equal to What was sent | AC5 |
| `LK4-cancel-stale` | With 3 s latency, Esc at 1 s closes, returns focus, and the late result is dropped, 20 of 20 | AC6 |
| `LK4-position-integrity` | CFI before equals CFI after, including a page turn while pending, 20 runs | AC7 |
| `EQ1-labels` | Every answer shows its label; no “%”; custom labels refused | AC8 |
| `LK10-failure-states` | Offline, timeout, 401, 429, bad JSON and a suspended provider each show their state and action | AC10 |
| `EA1-focus-and-announce` | VoiceOver announcement, focus order, axe pass, 7:1 in three themes | AC11 |
| `EB1-budgets-pending` | Page-turn and memory budgets hold with a request pending | AC12 |
| `DX1-import` | A generated MDX 2.0 (`Encrypted=2`) with MDD imports, indexes and shows an entry with its CSS and image; refused files keep the old dictionary | AC14 |
| `DX6-hostile-entry` | Script, handlers, remote image and remote `url()` run nothing and request nothing | AC15 |
| `DX6-traversal` | `../` and encoded traversal refused; only the active generation loads | AC16 |
| `DX5-redirects` | A cycle stops within eight hops; “invalidates” shows the “invalidate” entry with its label | AC17 |
| `DX9-never-sent` | No dictionary text in any extension message or request body | AC18 |
| `DX10-budgets` | Warm p95 within DX10; memory budget with three dictionaries | AC19 |
| `LK7-secrets` (Stage 2d) | Keychain only; header only to the matching host; no redirect | AC9 |

`D1-no-uncaught-errors` stays last and must still pass (AC13).

### 6.3 Corpus additions

`scripts/corpus/generate.py` gains generated dictionaries, written from the published format notes; no licensed dictionary is ever used:

- **Formats:** a plain MDX 2.0 with MDD (including split `.1.mdd` and `.2.mdd`), an MDX 1.2, an `Encrypted=2` one, a registration-protected one, an LZO-compressed one, a truncated one and a redirect cycle.
- **Encodings:** the same entries in UTF-8, UTF-16LE, GBK, GB18030 and Big5 (DX3).
- **Caps:** an oversized block, entry and resource, key counts and offsets past the file’s end, and a decompression bomb (DX13).
- **Hostile entries:** scripts, event handlers, `javascript:` links, `<base>`, `<meta http-equiv="refresh">`, prefetch and preload links, remote `img`, `srcset`, remote `url()` written plainly, with CSS escapes and inside `image-set()`, remote `@font-face`, `@import`, SVG with script inside the MDD, plain web links (DX15), and traversal paths. Every remote URL points at the harness canary (§6.10).
- **Content:** an entry with pronunciation buttons (DX8), one with large images, and headwords with curly and straight apostrophes, full-width characters, hyphens and NFC/NFD variants (DX4).

### 6.4 Visual baselines

Built from fixtures as the original §6.1 describes: the peek states with the test provider and the generated dictionary, Paper and Night (Sepia for one state), compared with the canvas by eye, then stored as baselines.

### 6.5 Spikes

| Spike | Question | Passes when |
|---|---|---|
| H — selection context | Does `Intl.Segmenter` in Linen’s WebKit give usable sentence boundaries over foliate ranges across inline markup, footnote markers and CJK? | The LK5 unit set passes in the app; 10 ms or less per selection |
| I — dictionary frame | Can a `linen-dict://` frame with `default-src 'none'` and no scripts render a sanitised entry with its CSS, fonts and images, keep text selectable and copyable, follow `entry://` links with Back, size itself inside the peek, and be read by VoiceOver? | All six work in WKWebView at the Safari 16.4 floor, or a fallback is chosen and recorded |
| J — Keychain under ad-hoc signing | Release builds are ad-hoc signed (`.github/workflows/release.yml`), and macOS ties Keychain items to the app’s signature. Across an update and a reinstall, does a stored key stay readable silently, prompt once, or fail? | The behaviour is recorded for install, update and reinstall, and LK7’s design adjusted if needed (for example, asking for the key again with a clear message) before Stage 2d |

### 6.6 Edge-case checks

In-app checks added by the review, named as in §6.2. Rules marked (prov.) are tested against the recommended default until the owner decides the matching O-item.

| Check | Asserts | Rule | Stage |
|---|---|---|---|
| `LK14-highlight-lookup` | A clicked highlight offers Look up and lookup extensions; Delete still replaces Search | LK14 (O9) | 2a |
| `LK13-no-room` | At 48 px text in a 600 px-high window, the peek opens on the side with more room, never covers the selected line, and its answer scrolls | LK13 (O10) | 2a |
| `LK12-close-triggers` | Scrolling in Scroll mode, a resize, ⌘+, an Aa change, opening the Navigator, Back, a Contents jump, a search result, a link and ⌘L each close the peek and cancel its request; a theme change keeps it open and re-themes it | LK12 (O11) | 2a |
| `LK9-language-fallback` | Books with no language, `und`, `en-GB` and `EN-us` report the LK9 value, and a lookup’s condition follows it | LK9 (O12) | 2a |
| `LK1-fixed-layout` | In a fixed-layout book no lookup is offered (or, if O13 decides otherwise, the peek places correctly at three zoom levels) | O13 | 2a |
| `LK15-single-provider` | With one provider the label is plain text, with no chevron | LK15 (O15) | 2a |
| `LK5-chunk-boundary` | In a chapter laid out in chunks (L16), a selection in a chunk’s first sentence gets the previous sentence from the previous chunk; the same across Scroll-mode views (B8) | LK5 | 2a |
| `LK5-context-cleanup` | Footnote markers and soft hyphens are absent from the context; ligatures and code keep their characters | LK5 | 2a |
| `LK1-layouts` | Placement and context on a spread’s right page, in Scroll mode, in vertical writing and in a right-to-left book | LK1, LK5 | 2a |
| `LK1-touch` | A touch selection, whose bar sits below it (A10), opens the peek clear of the selected line | LK1 | 2a |
| `EA1-keyboard-path` | F7 caret selection, F6, the arrows, Enter on Explain, Tab into the peek, Esc back to the selection, with no pointer | EA1 | 2a |
| `EA3-reduced-motion` | Under reduced motion the pending line doesn’t move and fades are 100 ms or less | EA3 | 2a |
| `X7-peek-forced-colours` | Under Increase Contrast and forced colours, the peek’s text, border, pointer and focus ring stay visible | EA2, X7 | 2a |
| `LK4-rapid` | A new selection or a second Explain while one is pending shows only the latest answer; earlier results are dropped | LK4 | 2a |
| `S14-warm-cancel` | ⌘L while a request is pending cancels it and hides the book from extensions; returning to the book shows no peek | LK4, LK12 | 2a |
| `N5-quit-pending` | Quitting while a request is pending quits at once and leaves no error in the crash log | LK4, D1 | 2a |
| `EP3-context-excludes` | Selections at a chapter’s first and last sentence, in front matter and next to a note send nothing from other chapters, front matter, highlights or notes (the test provider echoes its request) | EP3 | 2a |
| `EP5-crashlog-clean` | After forced provider errors and a crashed test provider, the crash log contains no selected or context text | EP5 | 2a |
| `DX14-no-entry` | With no entry in any dictionary, the no-entry state shows; “Explain in context” appears only when Explain is installed | DX14 (O8) | 2b |
| `DX15-entry-links` | Web links in an entry are inert; `testHooks.externalOpened` stays empty | DX15 (O14) | 2b |
| `DX3-encodings` | The UTF-8, UTF-16LE, GBK, GB18030 and Big5 fixtures show identical entries; an unsupported encoding is refused with its message | DX3 (O16) | 2b |
| `DX13-caps` | Blocks, entries and resources over the caps are refused as data errors and never shown | DX13 | 2b |
| `DX2-swap-while-open` | Re-importing a newer version while an entry with images is open keeps that entry whole; the next look-up uses the new generation; the old one goes when nothing uses it | DX2 | 2b |
| `DX1-duplicate` | Adding the same files again says the dictionary is already installed; a newer file with the same name replaces it through a new generation | DX1, DX2 | 2b |
| `DX1-restore-elsewhere` | With the library folder restored to another path (D2), dictionaries still open | DX1 | 2b |
| `DX7-night-card` | At Night the entry sits on the light card and keeps its own contrast | DX7 | 2b |
| `DX8-audio` | Pronunciation buttons say “Audio isn’t available” and request nothing | DX8 | 2b |
| `DX4-headword-variants` | Curly and straight apostrophes, full-width characters, hyphens and NFC/NFD forms find the same entry; a suffix rule never invents a wrong headword (“news”, “bus”, “data”) | DX4 | 2b |
| `LK6-deny-revoke` | Denying the host sheet sends nothing; removing a host while a request is pending cancels it; an update asking for a new host asks again | LK6, EP2 | 2d |
| `LK6-remove-extension` | Removing an extension while a request is pending cancels it and deletes its keys from the Keychain | LK6, LK7 | 2d |
| `LK7-keychain-denied` | A locked Keychain or denied access shows a clear state with “Open options” (wording prov.) and sends nothing | LK7 | 2d |

### 6.7 Data-safety tests

As the original plan’s §6.3:

- **Kill during an import** at each step (copy, validate, index, activate): the next launch shows the old dictionary or the new one, never a half-built one (`DX2-kill-mid-import`, using the harness’s restart).
- **Disk full during an import:** the Retry message; the old dictionary stays.
- **Migrations:** a database at today’s schema version migrates to the new `dictionaries` and `extension_grants` tables with no data loss, checked row by row.

### 6.8 Budgets added

Measured as the original §6.4 describes (release builds, reference machines, median and p95 of 20 runs).

| Measure | Budget | Starting point |
|---|---|---|
| First look-up after launch | ≤ 300 ms (prov.) | Launch, then Look up → first entry painted, three dictionaries installed |
| Import of a 1 GiB package | Time recorded; memory under 400 MB at p95 (prov.) | Add dictionary… → active |
| Cold start to library | < 1 s, unchanged, with three dictionaries installed | As the original §6.4 |

### 6.9 Provider contract tests (private repository, Stage 3)

- **Recorded responses** for each preset (DeepSeek, Qwen international, OpenAI, Anthropic, Gemini, Ollama), replayed through `net.fetch`, including non-JSON, partial JSON and empty answers.
- **Error mapping:** 401, 403, 404 (model missing), 429 with and without Retry-After, 500, connection refused (Ollama not running), DNS and TLS failures, and a captive-portal HTML page returned as 200 (O17).
- **Limits:** the 5 MB response cap and 10 s timeout of `net.fetch` (`src-tauri/src/extensions/registry.rs`); a model that answers after 10 s shows the timeout state.
- **No test sends real book text to a real provider.**

### 6.10 Harness rules for the new checks

- **Independent checks.** Each check installs its own test dictionary and test provider and removes them afterwards, so it passes alone (`scripts/e2e.sh r '^LK'`) and in the full suite, whatever ran before it.
- **No-network assertions use two witnesses:** the `net.fetch` spy and the canary server on 127.0.0.1:8765 (`src-tauri/src/spikes.rs`). Hostile fixtures point their URLs at the canary, so a load from the dictionary frame, which doesn’t go through `net.fetch`, is caught.
- **Existing harness rules hold:** no check opens the system browser (`testHooks.externalOpened`), every check restores the clipboard, and `D1-no-uncaught-errors` stays last.

## 7. Security

- **Dictionary content is untrusted input**, like book content (§7.1 of the original plan): sanitised in the core before it reaches WebKit, served from its own scheme, framed without scripts, with CSP `default-src 'none'` and only `linen-dict:` for styles, fonts and images.
- **Parser hardening:** size caps after decompression and offset checks against the file’s length (DX13), and a fuzz target (§6.1). An MDX file is as hostile as an EPUB.
- **Links in entries** are inert except `entry://` (DX15), so dictionary content can’t open a browser or a tracking URL.
- **Display only** is enforced in code, not by convention: no Host API path returns entry text (DX9), and the e2e spy proves it. No-network claims are checked at the canary as well as at `net.fetch` (§6.10).
- **Secrets** never enter the WebView or extension storage; the core adds them per request, only to the matching host, and never follows redirects (LK7).
- **Consent** is drawn by Linen, never by an extension: the host permission and key sheets are core UI (LK6, LK7).
- **Explain** stays out of this repository and its releases until Gate 3.

## 8. Risks

| Risk | Mitigation |
|---|---|
| Stage 1 finds that local stuck points are rare | Stop after Stage 2b; the peek and dictionaries still serve the 1.1 roadmap |
| WebKit can’t frame dictionary entries as needed | Spike I before Stage 2b; fallback: render a stricter subset of entry HTML in the core’s own markup |
| An unusual MDX breaks the parser | Generations keep the old dictionary; DX3 refuses what isn’t supported |
| Host API additions become permanent | Mark `lookups`, `secrets`, `optionalPermissions` experimental until 1.2 |
| Wrong answers delivered confidently | EQ1–EQ4 in the core; G2 before any pilot |
| Excerpts with a buyer’s name or other personal text sent to a provider | Paragraph-bounded context (EP3), the first-request notice, What was sent |
| Keys become unreadable or prompt after each update of an ad-hoc signed build | Spike J before Stage 2d; a clear “add the key again” state (`LK7-keychain-denied`) |
| Chinese MDX files in encodings Linen doesn’t decode | DX3 encodings and their fixtures; refuse anything else with a specific message |
| Context cut at a chunk boundary in long chapters | LK5 continues across L16 chunks and B8 views; `LK5-chunk-boundary` |
| Placement or close behaviour left undefined | O9–O13 and O15 decided before Stage 2a, with provisional rules LK12–LK15 tested meanwhile |

## 9. Design → implementation map

| Canvas | State | Component | Rules |
|---|---|---|---|
| 1, 15 | Selection bar menu with Explain and Look up | `SelectionBar.svelte` | LK1, EP1 |
| 2 | Pending | `LookupPeek.svelte` | LK3, EB1, EA3 |
| 3, 10 | Answer, Paper and Night | `LookupPeek.svelte` | LK1, LK2, EQ1–EQ3 |
| 4 | More | `LookupPeek.svelte` | LK1, EX5 |
| 5, 16 | Provider and language menu | `LookupPeek.svelte`, `lib/lookup/` | LK11 |
| 6 | What was sent | `LookupPeek.svelte` | EX6, EP3 |
| 7 | Needs more context | `LookupPeek.svelte` | EQ4 |
| 8 | First request to a host | `LookupPeek.svelte` (content from the provider) | EX8 |
| 9 | Errors | `LookupPeek.svelte` | LK10 |
| 11 | Dictionary entry | `LookupPeek.svelte`, `linen-dict://` frame | DX4–DX9, LK11 |
| 12 | Explain options | `ExtensionsPane.svelte`, the options frame | LK6, LK8, EP4 |
| 13 | Host permission and key sheets | Core sheets | LK6, LK7 |
| 14 | Settings › Dictionaries | `DictionariesPane.svelte` | DX1–DX3, DX11 |
