# Development notes

The longer notes behind [`AGENTS.md`](../AGENTS.md): the commands in full, the in-app end-to-end suite, the architecture and how foliate-js is patched. Read the part you need.

**Rule IDs.** `docs/rule-index.md` lists every plan rule with the decisions that change it, which win over the plan. `python3 scripts/rule-index.py S9` prints one rule with its decisions, the code that cites it and its in-app checks. After a change to `docs/decisions.md` that names a plan rule, run `pnpm rules:index`; CI fails while the index is out of date. The index is found by matching text: it helps you find things, `docs/decisions.md` is the record, and its known gaps are in issue #60.

## Commands

```sh
pnpm install                      # applies patches/foliate-js@0.0.0.patch (pnpm-workspace.yaml)
pnpm tauri dev                    # run the app

# What CI runs (.github/workflows/ci.yml)
pnpm format:check && pnpm lint && pnpm check && pnpm test && pnpm design:check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings   # warnings fail CI
cargo test --manifest-path src-tauri/Cargo.toml
pnpm test:integration             # Playwright, tests/integration (gallery page, IPC mocked)

# Single tests
pnpm vitest run src/reader/layout.test.ts        # or: pnpm vitest run -t "name"
(cd src-tauri && cargo test import::tests::duplicates_and_updated_files_follow_b3)
(cd src-tauri && cargo test --features spikes)   # include spike-only code
```

The unit tests are `src/**/*.test.ts` (Vitest) and `#[cfg(test)]` modules in Rust. The Rust corpus tests need the test corpus: `python3 scripts/corpus/fetch.py` downloads `corpus/manifest.json` into `corpus/cache/` (hash-verified), and `python3 scripts/corpus/generate.py` makes the hostile, broken and large books in `corpus/generated/`. `scripts/ci-local.sh` and CI do both and set `LINEN_REQUIRE_CORPUS=1`, so a missing file fails there; a plain `cargo test` skips it. Project Gutenberg rebuilds its EPUBs from time to time: a MISMATCH from `fetch.py` means checking the new file and pinning its hash (`corpus/manifest.json`).

### The in-app end-to-end suite (macOS has no WebDriver for Tauri)

The real app runs its own suite: `src/spikes/e2e.ts`, loaded from `spikes.html`.

```sh
# 1. The test build. Always through the Tauri CLI: a plain `cargo build` gives a
#    binary that loads the dev URL and shows a blank page.
LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes --config src-tauri/tauri.spikes.conf.json
# 2. Run it
scripts/e2e.sh                              # the full suite (~30 min), results in docs/spikes/raw/e2e-reader.json
scripts/e2e.sh r '^(N3-bodymatter|B8)'      # checks whose id matches the regex
scripts/e2e.sh v && node tests/visual/compare.mjs   # captures against docs/visual/baselines/
python3 scripts/perf-coldstart.py; scripts/perf-memory.sh    # §6.4 budgets
```

- **Which desktop.** Runs happen on the current desktop; they no longer have to be on Desktop 2 (owner, 2026-09-29). `LINEN_SPACE=2` still moves the window to Desktop 2: it starts hidden, moves, then shows. Captures are of the Linen window only, via `CGSHWCaptureWindowList`.
- **Differences on Desktop 2.** Several baselines were captured there. The window there is never active: WebKit draws no focus rings, selections are grey, and the traffic lights look inactive. Visual diffs of those are expected; see `docs/visual/APPROVAL.md`.
- **Test data.** Each run uses a throwaway `LINEN_DATA_DIR`. `caffeinate -di` keeps the display awake, because a sleeping display stops `requestAnimationFrame` and stalls the run.
- **Order matters.** Checks depend on earlier ones: some need a highlight or an installed extension, and E2-fixed leaves a fixed-layout book open. A failure in a small subset can be an artefact of the order; confirm with the full suite.
- **The last check** (`D1-no-uncaught-errors`) fails if the run left any uncaught error in the crash log.
- **Opens are cold.** The harness sets `hooks.noWarm` so each open is cold; only `S14-warm-book` tests the warm book.
- **Harness rules.** Tests must not open the system browser (links are recorded in `testHooks.externalOpened`) and must restore the clipboard.

## Architecture

### Rust core (`src-tauri/src`)

| Module                           | What it holds                                                                                                                                                                                                                                                                                                    |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `commands.rs`                    | Most IPC commands. `AppState` (the SQLite `Store`, the `Library` folders, the open book's zip). The `linen-book://` protocol serves book media straight from the zip (no blobs; §6.4 memory).                                                                                                                    |
| `store.rs`                       | SQLite in WAL mode, one transaction per write, versioned migrations in `PRAGMA user_version`.                                                                                                                                                                                                                    |
| `import.rs`, `epub.rs`           | Validation (zip bombs, traversal, XML entities, DRM) and metadata. Books are copied into the library folder. `Library::book_file` and `cover_file` resolve stored paths **by file name inside the current library folder** (D2: a folder restored elsewhere must still open). Never trust stored absolute paths. |
| `extensions/`, `ext_commands.rs` | The extension registry, manifest validation and `when` clauses. The `linen-ext://<id>/` protocol serves each extension on its own origin. Storage is capped at 10 MB; `net.fetch` reaches declared hosts only; safe mode.                                                                                        |
| `native.rs`, `native_input.rs`   | AppKit: wheel and trackpad events (sent as `native-scroll`), the pasteboard, VoiceOver detection, window buttons.                                                                                                                                                                                                |
| `crashlog.rs`                    | D1: a local crash log only. No telemetry.                                                                                                                                                                                                                                                                        |
| `updater.rs`                     | D6: the daily update check runs in the core; pages never get the updater plugin.                                                                                                                                                                                                                                 |
| `spikes.rs`                      | Behind `--features spikes`: the test harness (moving to another desktop, captures, canaries, corpus access). Everything test-only belongs there.                                                                                                                                                                 |

### Frontend (`src`)

The app has three pages: `index.html` → `main.ts`/`App.svelte`, `settings.html` → `src/prefs/` (the Settings window), and `spikes.html` (spike builds only). A fourth, `gallery.html`, is for tests only: the Playwright accessibility checks (`tests/integration/gallery-a11y.spec.ts`) render the shared components there, so a change to them shows up in that suite. Settings changes reach other windows through the `settings-changed` event (`app/settingsSync.ts`).

- **`App.svelte`.** The library or the reader, the command palette, the message bar.
  - **S14:** the last book stays mounted behind the library ("warm") in a `.reader-layer.warm` wrapper, hidden with `opacity: 0` and `inert`. It can't use `display: none`, which would re-lay the book out at zero size, or `visibility`, which the engine's views override.
  - **The Reader's `active` prop.** While `active` is false, the Reader must take no keys, wheel or commands, and extensions must not see its book.
- **`reader/engine.ts`, `ReaderEngine`.** The one adapter around foliate-js; the rest of the app never talks to foliate directly. Its modes:
  - **Pages:** the current view plus two pre-laid-out neighbour views, so a turn is a swap (D-D1).
  - **Scroll:** a stack of views in a scrolling host, with a pinned navigation target so relayouts don't move the place (B8).
  - **Sideways Scroll:** for vertical writing, using foliate's scrolled flow (I16).
  - **Fixed layout** (E2, with I17 zoom), and **chunked long chapters** (L16).
- **`reader/Reader.svelte`.** The reader's UI and all of its reader commands.
  - **Commands** are registered through a local `handle()` that attaches and detaches with `active`.
  - **The book prop is taken once** (`untrack`). Each Reader instance is keyed to one book, and a live prop would name the next book during teardown.
- **`reader/content.ts`.** Every book document passes through `transformContent`: a per-document CSP and a CSS sanitiser (L13, L14). `reader/styles.ts` forces the theme's ink over publisher colours.
- **`lib/commands/registry.ts`.** Every command is declared once (IDs, chords, single-key rules, menu placement). Features attach handlers; the menu bar, ⌘K and the cheat sheet all come from it. Detaching removes only the handler that was attached.
- **`lib/strings/en.ts`.** All UI text. `lib/theme/tokens.ts` holds the design tokens (tests enforce the contrast ratios).
- **`extensions/host.svelte.ts`.** The extension host: one Worker per extension, frames on the extension's own origin, a watchdog (2 s for UI, 10 s for work; three crashes in ten minutes suspend it), and `bridge`, the reader's book as extensions may see it. The API is in `docs/extensions/`.
- **`app/testHooks.ts`.** Exists only when the harness sets `__LINEN_E2E__`. It is the test seam; the product never reads it otherwise.

### foliate-js

foliate-js is pinned to a GitHub commit and patched through `patches/foliate-js@0.0.0.patch` (every change is marked `Linen (...)`). To change it:

1. `pnpm patch foliate-js@0.0.0 --edit-dir <dir>`
2. **Apply the existing patch first**, with `patch -p1 -d <dir> < patches/foliate-js@0.0.0.patch`, because the edit dir starts from pristine sources.
3. Edit, then `pnpm patch-commit <dir>`.

Its paginator needs regex lookbehind (Safari 16.4+). On older WebKit, `lib/reader/webkit.ts` shows a message instead of a blank reader.
