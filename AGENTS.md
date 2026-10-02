# AGENTS.md

Guidance for coding agents (Claude Code, Codex and others) working in this repository. Claude Code loads it through `.claude/CLAUDE.md`. A root `CLAUDE.md` is git-ignored and stays for the owner's private, local notes (pending approval 42, approved 2026-10-02).

Linen is a quiet EPUB reader for macOS, built with Tauri 2 (Rust core), Svelte 5 (runes) and foliate-js. Only macOS is supported for now; Windows and Linux are deferred.

- **The plan** is `docs/implementation-plan.md`, and it is **read-only**. Its rule IDs (I16, B8, S14, L16, D6…) appear throughout the code comments and tests. Look one up with `python3 scripts/rule-index.py S9`: it prints the plan's text, **the owner decisions that changed it (these win over the plan)**, the code that cites it and its e2e checks. Never act on the plan's wording alone.
- **Where things are:** `docs/NAVIGATION.md` maps the folders, says which document is current, and shows how to move around the three files over 2,000 lines (`src/spikes/e2e.ts`, `src/reader/Reader.svelte`, `src/reader/engine.ts`).
- **Where things stand:** `docs/PROGRESS.md`, then the newest status document it names (`docs/release-1.1-status.md` after Phase 8); `docs/decisions.md` for owner decisions; `docs/pending-approvals.md` for what waits on the owner.
- **The design** is in `docs/design/`, with a text export in `docs/design/text/`. "P§22" means section 22 of the proposal, and "Screen 05" means a screen in the screens bundle.

## Commands

```sh
pnpm install                      # applies patches/foliate-js@0.0.0.patch (pnpm-workspace.yaml)
pnpm tauri dev                    # run the app

# What CI runs (.github/workflows/ci.yml); scripts/ci-local.sh runs the same
pnpm format:check && pnpm lint && pnpm check && pnpm test && pnpm design:check && pnpm rules:check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings   # warnings fail CI
cargo test --manifest-path src-tauri/Cargo.toml
pnpm test:integration             # Playwright, tests/integration (gallery page, IPC mocked)

# Single tests
pnpm vitest run src/reader/layout.test.ts        # or: pnpm vitest run -t "name"
(cd src-tauri && cargo test import::tests::duplicates_and_updated_files_follow_b3)
(cd src-tauri && cargo test --features spikes)   # include spike-only code

# After editing docs/decisions.md (CI checks the index)
pnpm rules:index
```

The unit tests are `src/**/*.test.ts` (Vitest) and `#[cfg(test)]` modules in Rust. The Rust corpus tests skip themselves when the corpus is missing. To get it: `python3 scripts/corpus/fetch.py` downloads `corpus/manifest.json` into `corpus/cache/` (hash-verified), and `python3 scripts/corpus/generate.py` makes the hostile, broken and large books in `corpus/generated/`.

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

- **Which desktop.** Runs happen on the current desktop (owner, 2026-09-29). `LINEN_SPACE=2` moves the window to Desktop 2: it starts hidden, moves, then shows. Captures are of the Linen window only, via `CGSHWCaptureWindowList`.
- **Differences on Desktop 2.** Several baselines were captured there. The window there is never active: WebKit draws no focus rings, selections are grey, and the traffic lights look inactive. Visual diffs of those are expected; see `docs/visual/APPROVAL.md`.
- **Test data.** Each run uses a throwaway `LINEN_DATA_DIR`. `caffeinate -di` keeps the display awake, because a sleeping display stops `requestAnimationFrame` and stalls the run.
- **Order matters.** Checks depend on earlier ones: some need a highlight or an installed extension, and E2-fixed leaves a fixed-layout book open. A failure in a small subset can be an artefact of the order; confirm with the full suite.
- **The last check** (`D1-no-uncaught-errors`) fails if the run left any uncaught error in the crash log.
- **Opens are cold.** The harness sets `hooks.noWarm` so each open is cold; only `S14-warm-book` tests the warm book.
- **Harness rules.** Tests must not open the system browser (links are recorded in `testHooks.externalOpened`) and must restore the clipboard.
- **Waiting.** The full suite takes about 30 minutes and a CI run about 4. Run them in the background and wait for them to finish; don't poll with `sleep`.

## Quality gates (CI treats warnings as errors)

- Before every commit, run `scripts/ci-local.sh` (the CI steps above; `--full` adds Playwright).
- Never dismiss a warning as "pre-existing": CI fails on it. Fix it, or flag it explicitly to the owner.
- After any `git push`, run `gh run list --limit 3` and `gh run watch` on the latest run. A task is done only when that run is green; if it is red, investigate immediately.

## Architecture

### Rust core (`src-tauri/src`)

| Module                           | What it holds                                                                                                                                                                                                                                                                                                    |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `commands.rs`                    | Most IPC commands. `AppState` (the SQLite `Store`, the `Library` folders, the open book's zip). The `linen-book://` protocol serves book media straight from the zip (no blobs; §6.4 memory).                                                                                                                    |
| `store.rs`                       | SQLite in WAL mode, one transaction per write, versioned migrations in `PRAGMA user_version`.                                                                                                                                                                                                                    |
| `import.rs`, `epub.rs`           | Validation (zip bombs, traversal, XML entities, DRM) and metadata. Books are copied into the library folder. `Library::book_file` and `cover_file` resolve stored paths **by file name inside the current library folder** (D2: a folder restored elsewhere must still open). Never trust stored absolute paths. |
| `extensions/`, `ext_commands.rs` | The extension registry, manifest validation and `when` clauses. The `linen-ext://<id>/` protocol serves each extension on its own origin. Storage is capped at 10 MB; `net.fetch` reaches declared hosts only; safe mode.                                                                                        |
| `native.rs`, `native_input.rs`   | AppKit: wheel and trackpad events (sent as `native-scroll`), the pasteboard, VoiceOver detection, window buttons.                                                                                                                                                                                                |
| `dictionary.rs`                  | Look Up (release 1.1): definitions from this Mac's dictionaries; nothing is sent anywhere.                                                                                                                                                                                                                       |
| `crashlog.rs`                    | D1: a local crash log only. No telemetry.                                                                                                                                                                                                                                                                        |
| `updater.rs`                     | D6: the daily update check runs in the core; pages never get the updater plugin. An install that can't replace itself announces the update once per version (item 32).                                                                                                                                           |
| `spikes.rs`                      | Behind `--features spikes`: the test harness (moving to another desktop, captures, canaries, corpus access). Everything test-only belongs there.                                                                                                                                                                 |

### Frontend (`src`)

There are three pages: `index.html` → `main.ts`/`App.svelte`, `settings.html` → `src/prefs/` (the Settings window), and `spikes.html` (spike builds only). Settings changes reach other windows through the `settings-changed` event (`app/settingsSync.ts`).

- **`App.svelte`.** The library or the reader, the command palette, the message bar.
  - **S14:** the last book stays mounted behind the library ("warm") in a `.reader-layer.warm` wrapper, hidden with `opacity: 0` and `inert`. It can't use `display: none`, which would re-lay the book out at zero size, or `visibility`, which the engine's views override.
  - **The Reader's `active` prop.** While `active` is false, the Reader must take no keys, wheel or commands, and extensions must not see its book.
- **`reader/engine.ts`, `ReaderEngine`.** The one adapter around foliate-js; the rest of the app never talks to foliate directly. Its modes:
  - **Pages:** the current view plus two pre-laid-out neighbour views, so a turn is a swap (D-D1).
  - **Scroll:** a stack of views in a scrolling host, with a pinned navigation target so relayouts don't move the place (B8).
  - **Sideways Scroll:** for vertical writing, using foliate's scrolled flow (I16).
  - **Fixed layout** (E2, with I17 zoom), and **chunked long chapters** (L16).
- **`reader/Reader.svelte`.** The reader's UI and all of its reader commands; `grep -n '// ----'` lists its sections.
  - **Commands** are registered through a local `handle()` that attaches and detaches with `active`.
  - **The book prop is taken once** (`untrack`). Each Reader instance is keyed to one book, and a live prop would name the next book during teardown.
- **`reader/content.ts`.** Every book document passes through `transformContent`: a per-document CSP and a CSS sanitiser (L13, L14). `reader/styles.ts` forces the theme's ink over publisher colours.
- **`lib/commands/registry.ts`.** Every command is declared once (IDs, chords, single-key rules, menu placement). Features attach handlers; the menu bar, ⌘K and the cheat sheet all come from it. Detaching removes only the handler that was attached. User remapping is in `lib/commands/remap.ts` (C4).
- **`lib/strings/en.ts`.** All UI text. `lib/theme/tokens.ts` holds the design tokens (tests enforce the contrast ratios).
- **`extensions/host.svelte.ts`.** The extension host: one Worker per extension, frames on the extension's own origin, a watchdog (2 s for UI, 10 s for work; three crashes in ten minutes suspend it), and `bridge`, the reader's book as extensions may see it. The API is in `docs/extensions/`.
- **`app/testHooks.ts`.** Exists only when the harness sets `__LINEN_E2E__`. It is the test seam; the product never reads it otherwise.

### foliate-js

foliate-js is pinned to a GitHub commit and patched through `patches/foliate-js@0.0.0.patch` (every change is marked `Linen (...)`). To change it:

1. `pnpm patch foliate-js@0.0.0 --edit-dir <dir>`
2. **Apply the existing patch first**, with `patch -p1 -d <dir> < patches/foliate-js@0.0.0.patch`, because the edit dir starts from pristine sources.
3. Edit, then `pnpm patch-commit <dir>`.

Its paginator needs regex lookbehind (Safari 16.4+). On older WebKit, `lib/reader/webkit.ts` shows a message instead of a blank reader.

## Conventions

- **Comments cite rule IDs** from the plan (`// B8: …`, `// S14: …`); keep doing so. Designs built before the owner approved them are marked `provisional`; `docs/decisions.md` records the approvals.
- **Decisions.** Anything that needs an owner decision goes into `docs/pending-approvals.md`, numbered and with a recommendation, and work continues on everything else. Record approvals in `docs/decisions.md`, then `pnpm rules:index`.
- **Codex.** Call it only non-interactively: `codex exec … -s read-only` or `codex review`. Never run bare `codex`, never change `~/.codex/`, and never pass `--dangerously-bypass-approvals-and-sandbox`.
- **Releases.** Bump `version` in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, then push a matching `vX.Y.Z` tag. `.github/workflows/release.yml` builds a draft release (ad-hoc signed until there is a Developer ID) with signed updater archives and `latest.json`; publishing the draft releases it.
- **The update key.** It is in `~/.tauri/linen-updater.key` (with `.password`) and in the repository secrets. It must never be committed: the repository is **public**.
- **Stray files.** The `*.txt` files in the repository root are session exports, `codex-review.md` is a local review, and `logo-project/` is the brand record kept outside the repository; don't commit them.
- **Phased work.** Implement only the phases the owner names, then stop and summarise: checks passed, CI status, open gaps, and whether the changes were committed.
- **Scope for tooling tasks.** For guides, AGENTS.md and skill installs, do only what was asked; add no install or test steps unasked. When installing a skill, verify its SKILL.md frontmatter parses and the skill loads, or say it was not verified.
- **Keep this file current.** When a module, command or document named here changes, update this file in the same commit.
