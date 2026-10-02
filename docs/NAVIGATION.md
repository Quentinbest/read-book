# Finding your way around

Where things are, and which document to trust. For people and coding agents alike; the commands are in `README.md`, and the rules for coding agents in `AGENTS.md`.

## Which document is current

| Question | Read | Not |
|---|---|---|
| What does rule S9 (or L14, B8…) require now? | `python3 scripts/rule-index.py S9`: the plan's text, then every owner decision that changed it, newest last | The plan alone: it is read-only, so changed rules still read as first written |
| All rules and which ones decisions changed | `docs/rule-index.md` (generated; `pnpm rules:index`, checked in CI) | |
| Where the work stands | `docs/PROGRESS.md`, then the newest status document it names | Older `docs/phaseN-status.md` files: each describes its phase when it closed |
| What the owner decided | `docs/decisions.md`; a later section overrides an earlier one | |
| What waits on the owner | `docs/pending-approvals.md` (the top says whether anything is waiting) | |
| What the design shows | `docs/design/text/` (searchable export; "P§22" is proposal section 22, "Screen 05" a screen) | The HTML bundles, unless you need the picture |
| Which captures are approved | `docs/visual/APPROVAL.md` | |

## Where the code is

| Path | What it holds |
|---|---|
| `src/App.svelte`, `src/main.ts` | The main window: library or reader, ⌘K, the message bar |
| `src/app/` | App-level pieces: `Library.svelte`, `CommandPalette.svelte`, `CheatSheet.svelte`, `ipc.ts` (every Rust command), `menubar.ts`, `settingsSync.ts`, `testHooks.ts` |
| `src/components/` | Shared UI primitives: buttons, popovers, sheets, tabs, icons |
| `src/reader/` | The reader: `Reader.svelte` (its UI and commands), `engine.ts` (the only foliate-js adapter), panels and popovers, and per-feature logic (`layout.ts`, `pages.ts`, `content.ts`, `styles.ts`…) |
| `src/lib/<area>/` | Logic with unit tests and no UI: `reader/state.ts` (the chrome/Navigator state machine), `commands/registry.ts`, `annotations/`, `search/`, `library/`, `input/`, `extensions/`, `strings/en.ts` (all UI text), `theme/tokens.ts` |
| `src/extensions/host.svelte.ts` | The extension host (Workers, frames, watchdog, `bridge`) |
| `src/prefs/` | The Settings window (`settings.html`) |
| `src/spikes/` | Test-build only: the in-app e2e suite (`e2e.ts`), visual captures, spikes |
| `src/gallery/` | Dev-only component gallery, used by Playwright's axe checks |
| `src-tauri/src/` | The Rust core: `commands.rs` (IPC, the `linen-book://` protocol), `store.rs`, `import.rs`, `epub.rs`, `updater.rs`, `dictionary.rs`, `native*.rs`, `extensions/`, `spikes.rs` (test-build only) |
| `scripts/` | `e2e.sh`, `ci-local.sh`, `perf-*`, `release-macos.sh`, `rule-index.py`, `export-design-text.py`; the corpus tools are in `scripts/corpus/` |
| `corpus/` | Only `manifest.json` is tracked; `cache/` and `generated/` come from `scripts/corpus/fetch.py` and `generate.py` |
| `tests/` | `integration/` (Playwright, gallery page) and `visual/` (`compare.mjs`) |

Paths that are often guessed and do not exist: `corpus/generate.py` (it is `scripts/corpus/generate.py`), `src/library/` (`src/app/Library.svelte` and `src/lib/library/`), `docs/phase9-status.md` (after Phase 8 the status document is `docs/release-1.1-status.md`).

Code comments cite rule IDs (`// S9: …`), so `git grep -nw S9 -- src src-tauri/src` finds a rule's code; `scripts/rule-index.py S9` prints the same list with the e2e checks.

## The three long files

`src/spikes/e2e.ts`, `src/reader/Reader.svelte` and `src/reader/engine.ts` are each over 2,000 lines. Find your place by name, not by reading them through:

- **`Reader.svelte`** has section markers: `grep -n '// ----' src/reader/Reader.svelte` lists them with their rule IDs (chrome reveal S9–S11, page turns I1–I10, search F1–F8, …).
- **`engine.ts`**: one class, `ReaderEngine`; search for the method (`grep -n 'setHighlights\|#drawMarks' src/reader/engine.ts`).
- **`e2e.ts`**: the helpers are at the top (`showControls`, `openFromLibrary`, `selectPhrase`); each check is an object whose `id` starts with its rule, so `grep -n "id: 'S9" src/spikes/e2e.ts` finds it. The visual captures are in `spikeVisual`, further down.

Before a scripted find-and-replace in these files, read the exact lines again; after a long session or a context summary, the remembered text is usually out of date.

## Long-running work

The full in-app suite takes about 30 minutes, and a CI run about 4. Start them in the background and wait for them to finish; don't poll with `sleep`.
