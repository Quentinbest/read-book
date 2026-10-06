# Phase 13 status: sync (macOS)

- **Last updated:** 2026-10-06
- **Asked for:** the owner, “Make your best calls” (2026-10-06); decisions 45–50 in `docs/decisions.md` put sync before the iPad.
- **Plan:** `docs/next-steps-plan.md`, Phase 13.

## Built

| Part | Where | Tests |
|---|---|---|
| Hybrid logical clock that refuses readings more than a day ahead | `sync.rs` `Clock` | `sync::tests` |
| Records for positions and annotations, keyed by book identity (package identifier, else content hash) rather than device-local IDs | `sync.rs` `Change`, `BookKey` | `sync::tests` |
| Merge: newest wins; concurrent note edits keep both texts (a copy named after the losing version, logged by the device that lost); tombstones; undo as a newer change; newer-format records kept verbatim | `sync.rs` `merge`, `Replica::absorb` | `sync::tests`, including a three-device soak (40 seeds × 300 steps: skewed clocks, late, repeated and shuffled delivery) |
| Folder adapter: one log per device, written to a temporary name then renamed; iCloud conflict copies read; temporary files and placeholders ignored | `sync.rs` `FolderAdapter` | `sync::tests` |
| Device ID from the machine's IOPlatformUUID and the library's random ID, so a restored library on another Mac gets its own | `sync.rs` `device_id`, `sync_store.rs` `device` | `sync::tests` |
| Store integration: schema 5 (`sync_records`, `sync_outbox`, `sync_aside`); local writes logged only while sync is on; remote records applied without being logged back (no ping-pong); records for missing books wait for their import | `store.rs`, `sync_store.rs`, `commands.rs` | `sync_store::tests` (two libraries sharing a folder) |
| Background sync at launch and every minute; `sync-applied` event | `lib.rs` | — |
| Settings › Library › Sync: Choose Folder…, Sync Now, Turn Off (PROVISIONAL wording) | `Preferences.svelte`, `en.ts` | — |

## Not built yet

- **Updating an open book.** An applied change reaches the reader when it next reads its position or annotations. The reader doesn't listen to `sync-applied`, and the “Continue from page N (iPad)” offer isn't built.
- **No in-app check or Settings capture** for sync yet.
- **The Host API for extension adapters** (WebDAV and others) waits, like item 44.
- **Locking.** A sync holds the store lock while it reads the folder, so a slow iCloud read can delay a save briefly. Reading the folder outside the lock comes next.
- **Two real Macs over a week of reading** (plan Done-when) hasn't been done.

## Verification (2026-10-06, local)

- `scripts/ci-local.sh`: all green. `cargo test`: 85 unit tests and both migration tests passed (fixture `tests/fixtures/schema-v5.sql`). `cargo clippy --all-targets -- -D warnings`, with and without `--features spikes`, and `cargo fmt --check` are clean.
- `pnpm test`: 300 passed. `pnpm check`: 0 errors. `pnpm lint` is clean.
