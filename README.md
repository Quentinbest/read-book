# Linen

A quiet EPUB reader for macOS: Tauri 2, Svelte 5 and foliate-js. The plan is `docs/implementation-plan.md`; where each phase stands is in `docs/PROGRESS.md`.

## System requirements

macOS 13 or later, **with Safari 16.4 or later installed**. Linen reads books with the system's WebKit, which comes with Safari. On an older WebKit, Linen says so and offers Software Update instead of opening a book (decision D7).

## Your library

- **Where it is.** Everything is in one folder, `~/Library/Application Support/app.linen.reader`: the database (positions, highlights, notes, settings), the books, the covers, and the extensions with their data. Settings › Library › Show in Finder opens it.
- **Backup.** Back up that folder; Time Machine does it already.
- **Restore.** Put the folder back, on this Mac or another, then open Linen.
- **Export.** Settings › Library › Export all highlights and notes writes one W3C Web Annotation file per book, which other apps can read.
- **Uninstall.** Moving Linen to the Bin leaves the folder in place. To remove everything, delete it too, and `~/Library/Logs/app.linen.reader`.

## Privacy

Linen sends nothing anywhere. If something goes wrong, the details go to a crash log on this Mac (`~/Library/Logs/app.linen.reader/crash.log`). Settings › About reveals it, so you can attach it to an email if you choose. Extensions reach the network only through hosts they declare, and only after you allow them (`docs/extensions/README.md`).

## Updates and releases

- **Updates.** Linen checks for an update once a day. It installs it in the background and says “Update ready · Restart”; otherwise the new version starts at the next launch. Updates must be signed with the Linen update key, and anything else is refused.
- **Releases.** Pushing a tag `vX.Y.Z` that matches `version` in `src-tauri/tauri.conf.json` runs `.github/workflows/release.yml`. It makes a draft GitHub Release with DMGs for Apple silicon and Intel, the signed update archives and `latest.json`; publishing the draft releases it. Until there is an Apple Developer ID the builds are ad-hoc signed, so macOS asks before the first open.
- **The update key** is `~/.tauri/linen-updater.key` (with `.password`) on the release Mac, and the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Keep a backup: without it, installed copies cannot be updated.

## Building

```sh
pnpm install
pnpm tauri dev                      # run
scripts/release-macos.sh --unsigned # a development DMG
scripts/release-macos.sh            # signed and notarised (needs the credentials it lists)
```

## Testing

```sh
pnpm test && pnpm check && pnpm lint                  # TypeScript
(cd src-tauri && cargo test --features spikes)        # Rust
LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes \
  --config src-tauri/tauri.spikes.conf.json           # the test build
scripts/e2e.sh                                        # the in-app suite, on Desktop 2
scripts/e2e.sh r '^(N3-bodymatter|B8)'                # some checks
scripts/e2e.sh v && node tests/visual/compare.mjs     # captures against the approved baselines
python3 scripts/perf-coldstart.py; scripts/perf-memory.sh   # budgets (§6.4)
```

The in-app suite runs on macOS Desktop 2 (`LINEN_SPACE`), so it never covers the screen in use. It keeps the display awake while it runs.
