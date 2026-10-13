# Linen

**English** · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md) · [日本語](README.ja.md)

Linen is a quiet EPUB reader for macOS. It keeps out of the book's way: the text fills the window, the controls appear when you reach for them, and nothing you read leaves your Mac. It is built with Tauri 2 (a Rust core), Svelte 5 and foliate-js, and is open source under the MIT licence.

![Reading Moby Dick in Linen, with the reading settings open](docs/readme/reader.png)

- **Read your way.** Pages or Scroll, two-page spreads in wide windows, right-to-left and vertical writing, and fixed-layout books with zoom. Text size, line spacing, fonts (OpenDyslexic among them), page width, and three themes: Paper, Sepia and Night.
- **Find your way.** Contents, search in the book, Go to a percentage, chapter or print page, footnotes in a peek, and Back after every jump.
- **Highlights and notes** in four colours, gathered in the Notes tab and exported as Markdown or W3C Web Annotations.
- **Look Up** in your Mac's dictionaries or your own MDX dictionaries, without leaving the page.
- **Keyboard first.** Every command is in the command palette (⌘K), and every shortcut can be changed.
- **Extensions** add commands, selection actions, a Navigator tab, themes and exporters, each in its own sandbox and with only the permissions you allow.
- **Five languages:** English, Simplified Chinese, Traditional Chinese, Japanese and Spanish. VoiceOver reads all of it.

| Contents                                                             | Highlights and notes                                                 |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| ![The Contents tab beside the text](docs/readme/contents.png)        | ![The Notes tab](docs/readme/notes.png)                              |
| **The selection bar**                                                | **Look Up**                                                          |
| ![The selection bar over a highlight](docs/readme/selection-bar.png) | ![A word looked up in the Mac's dictionary](docs/readme/look-up.png) |

## For readers

### Install

1. You need **macOS 13 or later, with Safari 16.4 or later.** Linen reads books with the system's WebKit, which comes with Safari. On an older WebKit, Linen says so and offers Software Update instead of opening a book.
2. Download the DMG from the [latest release](https://github.com/Quentinbest/read-book/releases/latest): `aarch64` for Apple silicon, `x64` for Intel Macs. Open it and drag Linen to Applications.
3. **The first open.** Linen is not yet signed with an Apple Developer ID, so macOS asks before opening it. Control-click Linen and choose **Open**, then **Open** again; or open it once, then choose **Open Anyway** in System Settings › Privacy & Security.

Linen checks for an update once a day and installs signed updates by itself. What changed in each version is in [`CHANGELOG.md`](CHANGELOG.md).

### Your first book

1. **Add books.** Drop EPUB files on the library, choose **Open…** (⌘O), or double-click an EPUB in Finder. Linen copies each book into its library.
2. **Read.** Click a cover, or **Resume reading** for the last book. Turn pages with → and ←, Space, Page Down or the trackpad; move the pointer to the top or bottom edge for the toolbars. ⌘L goes back to the library.
3. **Make it yours.** Click **Aa** for text size, theme, line spacing, font, page width, and Pages or Scroll. ⌘+ and ⌘− change the text size anywhere.
4. **Find your way.** ⌘T opens Contents, ⌘F searches the book, ⌘J goes to a page, chapter or percentage, and ⌘[ takes you back after a jump.
5. **Highlight.** Select text, and the selection bar offers four colours, Note, Copy, Search and Look Up. With text selected, H highlights and N adds a note. ⌘⇧A opens the Notes tab, where you can filter them and export them.
6. **Look up words.** Select a word and press ⌃⌘D. To use your own dictionaries, add MDX files in Settings › Dictionaries.
7. **Everything else** is in the command palette (⌘K). Press ? for the cheat sheet, and change any shortcut in Settings › Shortcuts.

**Language.** Linen follows your Mac's preferred languages. To choose another, go to Settings › General › Language; it applies after Linen restarts.

### Your library

- **Where it is.** Everything is in one folder, `~/Library/Application Support/app.linen.reader`: the database (positions, highlights, notes, settings), the books, the covers, and the extensions with their data. Settings › Library › Show in Finder opens it.
- **Backup.** Back up that folder; Time Machine does it already.
- **Restore.** Put the folder back, on this Mac or another, then open Linen.
- **Export.** Settings › Library › Export all highlights and notes writes one W3C Web Annotation file per book, which other apps can read.
- **Uninstall.** Moving Linen to the Bin leaves the folder in place. To remove everything, delete it too, and `~/Library/Logs/app.linen.reader`.

### Privacy

Linen sends nothing anywhere. If something goes wrong, the details go to a crash log on this Mac (`~/Library/Logs/app.linen.reader/crash.log`). Settings › About reveals it, so you can attach it to an email if you choose. Extensions reach the network only through hosts they declare, and only after you allow them.

![Linen's Night theme](docs/readme/night.png)

## For developers

### Set up

You need a Mac with the Xcode command-line tools, [Rust](https://rustup.rs) (stable), Node.js 24 and pnpm (`corepack enable` gives the version in `package.json`).

```sh
git clone https://github.com/Quentinbest/read-book.git
cd read-book
pnpm install        # also applies patches/foliate-js@0.0.0.patch
pnpm tauri dev      # run the app
```

### Find your way around

| Where                      | What                                                                                                               |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `src-tauri/src`            | The Rust core: IPC commands, the SQLite store, import and EPUB validation, the extension registry, AppKit bridges. |
| `src/reader/engine.ts`     | `ReaderEngine`, the one adapter around foliate-js: Pages, Scroll, Sideways Scroll, fixed layout.                   |
| `src/reader/Reader.svelte` | The reader's interface and its commands.                                                                           |
| `src/lib/commands/`        | Every command, declared once; the menu bar, ⌘K and the cheat sheet come from it.                                   |
| `src/lib/strings/`         | All interface text, one catalogue per language. Shared terms are in `docs/i18n/glossary.md`.                       |
| `src/extensions/`          | The extension host. The API reference is [`docs/extensions/README.md`](docs/extensions/README.md).                 |
| `docs/`                    | The implementation plan, the design, decisions and progress. Code comments cite the plan's rule IDs (`// B8: …`).  |

### Your first change

1. Branch from `main`, make the change, and add a test beside it (`src/**/*.test.ts` for Vitest, a `#[cfg(test)]` module in Rust).
2. Run what CI runs. Warnings fail CI.

   ```sh
   scripts/ci-local.sh          # format, lint, types, unit tests, design tokens, cargo fmt, clippy, cargo test
   scripts/ci-local.sh --full   # and the Playwright tests
   pnpm vitest run src/reader/layout.test.ts    # one test file
   ```

3. Open a pull request. It is merged once CI is green.

### Write an extension

An extension is a zip named `*.linenext` with a `manifest.json`, a worker script and optional pages. Start from the samples in [`examples/extensions`](examples/extensions) (a dictionary and the Night Owl theme), read [`docs/extensions/README.md`](docs/extensions/README.md), and install yours from Settings › Extensions › Install from file….

### Test in the real app

macOS has no WebDriver for Tauri, so the app runs its own end-to-end suite from a test build:

```sh
LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes \
  --config src-tauri/tauri.spikes.conf.json           # the test build
scripts/e2e.sh                                        # the in-app suite (about 30 minutes)
scripts/e2e.sh r '^(N3-bodymatter|B8)'                # the checks whose id matches
scripts/e2e.sh v && node tests/visual/compare.mjs     # captures against the approved baselines
python3 scripts/perf-coldstart.py; scripts/perf-memory.sh   # budgets (§6.4)
(cd src-tauri && cargo test --features spikes)        # Rust, with the test-only code
```

The suite runs on the current desktop and keeps the display awake; `LINEN_SPACE=2 scripts/e2e.sh` moves its window to Desktop 2 instead. The Rust corpus tests skip themselves until `python3 scripts/corpus/fetch.py` and `python3 scripts/corpus/generate.py` have made the test books.

### Build and release

```sh
scripts/release-macos.sh --unsigned # a development DMG
scripts/release-macos.sh            # signed and notarised (needs the credentials it lists)
```

- **Releases.** Bump `version` in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, add the version's summary to [`CHANGELOG.md`](CHANGELOG.md), then push a matching `vX.Y.Z` tag. `.github/workflows/release.yml` makes a draft GitHub Release with that summary, DMGs for Apple silicon and Intel, the signed update archives and `latest.json`; publishing the draft releases it. Until there is an Apple Developer ID the builds are ad-hoc signed.
- **The update key** is `~/.tauri/linen-updater.key` (with `.password`) on the release Mac, and the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Keep a backup: without it, installed copies cannot be updated. Never commit it.

## Licence

[MIT](LICENSE). The book in the screenshots is Herman Melville's _Moby Dick_, which is in the public domain.
