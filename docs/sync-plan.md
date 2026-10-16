# Sync across devices — plan

Status: draft for owner review, 2026-10-16 (#73). Nothing here is built. Values marked (prov.) are provisional until the owner approves them, as in `docs/implementation-plan.md` §1.2. The open questions are items 98–110 in `docs/pending-approvals.md`; nothing that depends on one is built while it is open.

Sync lets one reader keep their place, highlights and notes, settings, library list and books the same on every device they use, through storage they already have: their iCloud Drive or Google Drive folder. It comes as provider extensions in a new sync slot of the Host API, the two providers listed as Built-in. This file is the rule register that code and tests for this work cite (`// SY14: …`). It doesn't change `docs/implementation-plan.md`, which stays read-only.

**When it is built.** The owner decided on 2026-10-14 that sync waits for a second platform (#54, #80). After this plan merges, each stage in §6 gets its own sub-issue of #73, unlabelled; a stage is built when the owner labels it.

## 1. Sources and scope

### 1.1 Sources

| Source | What it gives this plan | Cited as |
|---|---|---|
| Issue #73 | The requirement: what syncs, Done when, out of scope | **#73** |
| The owner, 2026-10-14 (in #73's “Already decided”; recorded in `docs/decisions.md` by this PR) | Everything but extensions and MDX dictionaries syncs; no Linen storage; providers as Built-in extensions in the sync slot; folders, not containers or APIs; the reader decides conflicts; deletion order and the week; sync waits for a second platform | **Decision 10-14** |
| Proposal P§18, P§19, P§20 | Sync providers, a “storage adapter for progress and annotations”, scheduled for 1.2; permissions; “Design the sync-provider API first so sync can be an extension”; no accounts | **P§n** |
| `docs/implementation-plan.md` | §1.3 (sync in 1.2), §4 (UUIDs and timestamps “so sync can come later”), §6.3 (data safety), §7.1 (book content is untrusted), §7.2 (extensions get no file system) | **Plan §n** |
| `docs/decisions.md` | B3 and B3-guard (which book is which), D1 (nothing leaves the Mac), D2 (the library folder is the whole backup), G2 (Settings sections), G4 (Remove with Undo), G7 (built-ins), A7 (delete with Undo), A8 (nothing unplaceable is dropped), A9 (two anchors) | **B3** etc. |
| Closed PR #7, branch `worktree-typesafe-experiments` (commits `96538c0`, `7eba837`) | A prototype: `sync.rs` (a hybrid logical clock, per-device change logs, conflict copies, a machine-bound device ID, a three-device soak test) and `sync_store.rs`. Its merge rules (that branch's items 51 and 52) were never approved and differ from Decision 10-14: it kept both texts of a conflict, and synced neither books, settings nor removals | **Prototype** |

The prototype is design input only. Any of its code that is reused comes back in this plan's stages with migrations numbered from 6 (`docs/decisions.md`, “Old PRs”).

### 1.2 What syncs (Decision 10-14)

| Syncs | Stays on each device |
|---|---|
| The place in each book | Extensions: packages, on or off, grants, storage, keys |
| Highlights and notes | MDX dictionaries and their order |
| Settings, the ones SY15 lists (item 103) | Device-only settings (SY15), and the sync setup itself |
| The library list: which books, last opened, finished | Covers and the search cache, which are rebuilt from the book |
| The books themselves (EPUB files) | The crash log (D1) |

### 1.3 Out of scope

- Accounts, or storage run by Linen (P§20; Decision 10-14).
- Syncing extensions or MDX dictionaries (Decision 10-14).
- Building other platforms (#54, #80).
- Other providers (WebDAV, Dropbox and others). The slot leaves room for them (SY3); none is built.
- Encryption by Linen of what is in storage (item 108).
- Sharing between different readers, and syncing to two storages at once (item 109).
- Versions or history beyond the week that deleted things are kept.

## 2. What is there today (read 2026-10-16)

- **The store** (`src-tauri/src/store.rs`, schema 5). Books have a local UUID, a unique `content_hash` and the OPF `package_identifier`. A book's place is `positions` (CFI, fraction, `updated_at`, chapter label). Annotations already have global UUIDs, `created_at`, `updated_at` and `deleted_at`. A deleted annotation stays, marked, for Undo, and goes only with its book. `settings` is keys and values. `book_settings` and one setting key (`simplifyStyles:<bookId>`) name local book IDs, which differ on each device.
- **Removing a book** (G4): `remove_book` marks it; the next launch's `purge_removed` deletes the row, its annotations and place (cascade), its file and its cover.
- **Import** (`src-tauri/src/import.rs`): B3 and B3-guard. The same hash means the book is already in the library. The same identifier and title (case and spacing ignored) with a different hash means an updated file: the copy is replaced and annotations re-anchor when the book opens. Anything else is a new book.
- **The library folder** (D2): `~/Library/Application Support/app.linen.reader/`, holding `Books/`, `Covers/`, `Extensions/`, `Dictionaries/` and the database. Files are found by name inside it, so a restored folder works anywhere (`Library::book_file`).
- **Extensions**: Host API 1.2.0 (`manifest.rs`). Built-ins are compiled in (`ext_commands.rs` `BUILTINS`), have IDs starting `app.linen.`, and can be turned off but not removed (G7). Markdown Export holds `annotations.read` and `files.export`. Manifests refuse unknown fields, so a provider manifest needs a Linen that has the slot. An extension has no file system; `files.export` reaches only a file the reader picks in the system dialog.
- **The app is not sandboxed** (no entitlements file), so the core can reach `~/Library/Mobile Documents` and `~/Library/CloudStorage`. macOS may still ask the reader's permission first (Stage 0 finds out).
- **The test build** honours `LINEN_DATA_DIR`, so the in-app suite can run two library folders, one after the other, against one storage folder.
- **Nothing is sent anywhere** (the README's Privacy section; D1).

## 3. Decisions this plan rests on

Settled (Decision 10-14 and earlier):

- Everything in §1.2's left column syncs; extensions and MDX dictionaries do not.
- Linen runs no storage. It uses the reader's iCloud Drive and Google Drive, by writing into their folders on the Mac. No iCloud container, Apple Developer account or Google API.
- Sync comes through the sync slot (P§18, P§20). The iCloud and Google Drive providers come with Linen, listed as Built-in, like Markdown Export.
- When two devices changed the same highlight or note while apart, the reader decides which to keep.
- A deletion removes the local copy first, then the copy in storage. What was deleted is kept in storage for one week and can be restored during it. A device away for more than a week does not bring it back.
- Nothing leaves the device unless the reader chooses it (D1). Extensions reach the file system only through the host and its permission check (P§19, plan §7.2).
- B3 and B3-guard decide which book is which. A9 stores highlights as W3C Web Annotation JSON with two anchors.

Open, each with the stage that needs it (pending items 98–110):

| Item | Question | Needed by | Recommendation |
|---|---|---|---|
| 98 | The new permissions (SY1, SY2) | Stage 2 | `sync.provider`, at install, strongly warned; `folder:icloud-drive` and `folder:google-drive`, for a `Linen` folder at the top of that storage only, consented in Linen's setup sheet when the reader turns sync on |
| 99 | Where sync is set up (SY6, SY31) | Stage 3 | A new Settings section, Sync, after Library (amends G2). Built provisional; captures for approval |
| 100 | Where the reader decides a conflict (SY13) | Stage 5 | A “Changed on two devices” group at the top of the Notes tab, as “Couldn't place” is (A8), with both versions and Keep this; one quiet message after the sync that found them |
| 101 | Where deleted things are restored (SY19), and where a conflict about a book removed on this device is decided (SY18) | Stage 5 | Settings › Sync › Recently deleted…, a sheet listing the week's deleted books, highlights and notes with Restore. A book removed here but changed on another device is listed at its top (“Removed here, changed on ‹device›” · Keep the book · Remove), and the message after the sync that finds it opens the sheet. The library and the Notes tab are unchanged |
| 102 | A change and a deletion made while apart, including on a device away for more than a week (SY13, SY20) | Stage 1 | A conflict the reader decides, but only when this device changed the thing after the last sync it shared with the deleting device. Otherwise the deletion applies without asking. Nothing comes back without the reader's choice |
| 103 | Which settings sync, and text size between a Mac and an iPad (SY15) | Stage 3 | The list in SY15. Mac and iPad text sizes are decided with #80 |
| 104 | A newer place arriving for the open book (SY12) | Stage 3 | The book never moves under the reader. A message (M1): “You read further on ‹device› · Go there”; Back returns (N1) |
| 105 | Merge rules other than highlights and notes (SY12, SY14, SY16) | Stage 1 | Newest wins: the place (even when earlier in the book), each setting, finished and last opened. Adding a book is never undone by a merge. An updated file (B3) on two devices while apart: the newest import wins |
| 106 | Safety values (SY10, SY26, SY27) | Stage 1 | A remote clock more than 5 minutes ahead is named in Settings › Sync; more than 24 h ahead, set aside. A sync that would remove more than 10 books, more than a quarter of the library or more than 50 highlights waits for the reader. A consistent copy of the database and the books before the first sync, such a removal, or any sync that replaces or removes a book file, kept 7 days (the newest always). Deleted content leaves storage only when two clocks agree the week has passed |
| 107 | Timing (SY32) | Stage 3 | At launch, 10 s after the last local change, every 5 minutes while Linen is open, on quit (at most 2 s), and on Sync Now |
| 108 | Encrypting what is in storage | Stage 2 | No. The storage is the reader's own; a passphrase Linen asks for would lose the synced copy when forgotten. Revisit before any provider other than the two folders |
| 110 | How a new device is trusted (SY34) | Stage 3 | Signed changes. A new device is enrolled by a short code confirmed on an enrolled device, or by the library's recovery code shown once at first setup. Without either (every enrolled device lost, and the code too), the reader starts a new library from that device, and the old one stays readable in storage but isn't joined |
| 109 | One storage at a time, and switching (SY29) | Stage 3 | One. Switching keeps the old copy in the old storage (the reader may delete its `Linen` folder) and joins the new one as a first sync |

## 4. Rule register — SY

Each rule has an ID that code comments and tests cite. A rule built from an open item is marked with the item and stays provisional until the item is answered.

### 4.1 The slot, the folders and the providers

| ID | Rule | Source | Stage |
|---|---|---|---|
| SY1 | **The sync slot (Host API 1.3).** A manifest may contribute `syncProviders: [{ id, title, description }]` and needs the permission `sync.provider` (item 98). It activates on `onSync:<id>`. In its Worker, `linen.sync.registerProvider(id, { list(prefix), read(name), write(name, body), remove(name) })` makes it a store of named objects. A provider never merges, decides or reads Linen's data: the core keeps the journal, merges, conflicts and the week (§4.2–4.4) | P§18, P§20; Decision 10-14 | 2 |
| SY2 | **The brokered folder (item 98).** `folder:icloud-drive` and `folder:google-drive` give `linen.folder.list`, `.read`, `.write`, `.remove` and `.stat`, confined to a `Linen` folder at the top of that storage, which the host finds (iCloud Drive: `~/Library/Mobile Documents/com~apple~CloudDocs/Linen`; Google Drive: `~/Library/CloudStorage/GoogleDrive-‹account›/My Drive/Linen`, the account chosen at setup). Paths are relative and at most four segments deep; no `..`, and no symbolic link is followed. A name the extension makes up (to write) is `[A-Za-z0-9._-]` only, with no hidden names. A name the storage made (a conflict copy such as “changes 2.json” or “state (1).json”) can only come back from `list`, as an opaque token the host issued; `read` and `remove` accept that token, and `write` never does. A write goes to a temporary name and is renamed. Objects over 1 GiB (book files; the zip limit, plan §7.1) or 16 MB (everything else) (prov.) are refused; SY28 sets tighter limits per type | P§19, plan §7.2; Decision 10-14 | 2 |
| SY3 | **Handles, not bytes.** Object bodies cross into the Worker as opaque handles: `read` returns one and `write` takes one, so a folder provider passes handles from `linen.sync` to `linen.folder` without ever seeing a book, a note or a setting. The host moves the bytes and checks each object's hash (SY23). Reading a handle's bytes in the Worker, which a network provider would need, is reserved and not in 1.3 | P§19 | 2 |
| SY4 | **The built-ins.** `app.linen.sync-icloud` (“iCloud Drive”) and `app.linen.sync-google-drive` (“Google Drive”) are listed as Built-in in Settings › Extensions and can be turned off, not removed (G7). Being installed and on doesn't sync: neither activates, and the host opens no file in either storage, until the reader sets that storage up (SY6) | Decision 10-14; G7 | 2 |
| SY5 | **Budgets and safe mode.** Brokered folder and sync calls are the host's work and don't count toward the Worker's 10 s (P6). A file the storage hasn't downloaded yet is waiting, not a failure (SY30). In safe mode (P7) nothing syncs; local changes stay in the journal and go at the next normal launch | P6, P7 | 2 |
| SY6 | **Only storage the reader chose (D1).** Sync runs only after the reader chooses a storage in Settings › Sync (item 99) and confirms a setup sheet that names the folder, what will be copied (“12 books, 840 MB, and your highlights, notes, places and settings”) and who keeps it (“Apple keeps it in your iCloud account” / “Google keeps it in your Google Drive”). With none chosen, Linen opens no file in either storage and sends nothing, as today | D1; #73 | 2, 3 |

### 4.2 Identity and records

| ID | Rule | Source | Stage |
|---|---|---|---|
| SY7 | **Devices.** A device's ID is random, made on first use of sync and kept outside the library folder, in the platform's own per-device store (on macOS, the Keychain item `app.linen.reader/sync-device` in the data-protection keychain with a *ThisDeviceOnly* accessibility class, so neither iCloud Keychain nor Migration Assistant copies it to another Mac; Stage 0 confirms this). Every file a device writes also carries a writer token kept with the ID. A device that finds its ID in storage with another token (an ID copied some other way) takes a new ID at once, and it never replaces a file that carries another writer's token. A library folder restored on another device (D2) finds no ID there and makes its own, so two devices never write as one. A library restored on the same device from an older copy is caught too: when storage already holds later changes under this ID than the library knows of, the device takes a new ID. Its name comes from the platform (on macOS, the computer name), kept in the storage for conflicts and messages. Nothing in the sync format is specific to the Mac | Prototype; D2 | 1 |
| SY8 | **Which book is which (B3).** A synced book has a sync ID (a UUID from the device that first stored it) and an identity: its content hashes, and its OPF identifier with its title folded as B3-guard folds it. Two books are the same when a content hash matches, or when the identifier and the folded title both match. When two libraries stored the same book under two sync IDs before they first met, the smaller ID wins on every device and the other becomes its alias; highlights, notes and places from both are kept. Local book IDs never leave the device: records name sync IDs, and per-book keys (`book_settings`, `simplifyStyles:<bookId>`) are translated | B3, B3-guard; #73 | 1 |
| SY9 | **Records.** Each synced thing is one record: `place:‹book›`, `annotation:‹uuid›` (a highlight: anchors and colour, as A9's JSON), `note:‹annotation uuid›` (its note, a record of its own, so deleting or emptying a note while the highlight stays (A6) is a deletion kept for SY17's week and restorable), `setting:‹key›`, `bookSetting:‹book›:‹key›` (one per per-book setting), `book:‹book›` (the sync ID, its identity aliases from SY8, added), `bookFile:‹book›` (the current file's hash with everything read from that file: title, authors, language, OPF identifier, layout; SY16), `bookRead:‹book›:opened` and `bookRead:‹book›:finished` (SY14). What a file determines travels with that file, so a merge can't pair one device's EPUB with another's title. Things with different merge rules are never in one record, so a merge of one (a new edition) can't undo another (finished, made apart on another device). A version is `{ format: 1, key, clock, device, history, body }`, or `deleted` in place of `body`. `history` is a version vector (for each device, the latest of its versions of this record that this one descends from), so “descends from” (SY13) is decided from the version alone and never needs an older version to still exist. Unknown fields are kept and passed on unchanged. A record in a newer major format is kept aside, never applied or rewritten, and Settings › Sync says “‹device› uses a newer Linen · Update Linen” | Plan §4; Prototype | 1 |
| SY10 | **The clock, and what “newest” means.** Versions carry a hybrid logical clock. It keeps cause and effect in order: a change made after a device has seen another version always sorts after it, whatever either system clock says. Between concurrent versions (made apart, neither seeing the other), “newest” means the higher clock reading, ties broken by device ID, so every device picks the same one. That reading follows each device's system clock, so a device whose clock runs ahead can win a concurrent change it made earlier. Two guards: a remote version whose clock is more than 5 minutes ahead of this device's clock on arrival marks its device as running ahead, and Settings › Sync says “‹device›'s clock is ahead · Set it to automatic”; one more than 24 h ahead is set aside and applied once the clock catches up (both prov., item 106). Highlights and notes never rely on this: concurrent changes to one go to the reader (SY13) | Prototype; review | 1 |
| SY11 | **The folder's layout.** `Linen/libraries/‹library›.json` (format, a library's sync ID, created): a device setting up an empty folder writes its own claim, never a shared file. When two devices set up the same folder at once, every device uses the claim with the smallest ID, and the device whose claim lost removes it; since everything else is filed by device, nothing else moves; `devices/‹device›.json` (name, last sync, format); `changes/‹device›/‹sequence›.json` (each device's changes, in order); `state/‹device›/` (that device's compacted view, at most once a day, after which it deletes its own older change files. The view is split into shards of at most 8 MB (prov.), with records assigned by a hash of their key; `index.json` names the shards with their hashes and the last change sequence the view includes. A reader takes changes up to that sequence from the view, and waits only for missing change files after it, so a device that was behind resumes correctly across a compaction. A new generation of shards is written first and the index replaced last, so a reader never sees half a view; the view keeps each record's latest versions with their `history` (SY9), so a device back with a branch from a compacted version is still told apart from a descendant; it also keeps, for each thing deleted in the last 7 days, its last body and, for a book, its highlights, notes, place and per-book settings, so compaction never shortens SY17's week); `books/‹sha256›.epub`; `blobs/‹sha256›.json`, holding every record body (quotes, notes, book metadata, settings): versions, change files and shards hold only the body's hash, so no device's own files ever keep a reader's text. Blobs are shared and content-addressed. Any device may remove one, once no live version anywhere refers to it and the week has passed by SY17's two clocks. A device writing a version uploads any blob it refers to that is missing, so a device back from offline with a concurrent change never refers to a removed body. A blob is held to its type's limit (SY28). Stage 0 measures thousands of small files on both storages; if they are too slow, blobs are grouped into packs, removed when none of their bodies is live. Each device writes only its own files under `libraries/`, `devices/`, `changes/` and `state/`, so none of those has two writers, and a conflict copy of one (“2.json”, “(1).json”) is read, merged and removed by the device that owns it. `books/` and `blobs/` are shared, but each object's content is fixed by its name, so two writers can only write the same bytes. Any device that finds a conflict copy of a shared object checks its hash. If the copy matches its name and the object does too, the copy is removed; if only the copy matches, it replaces the object; if neither does, both go and SY23's repair applies. Each change file carries its own hash; one that doesn't match (half synced) is skipped and read later | Prototype | 1 |

### 4.3 Merging

| ID | Rule | Source | Stage |
|---|---|---|---|
| SY12 | **The place.** The newest version wins, even when it is earlier in the book (item 105). Opening a book uses the newest place, so a book opened on a second device resumes where the first left it. An open book never moves under the reader (item 104). Applying a remote place is not a local change, so two devices with the book open don't bounce it between them | #73; Prototype | 1, 3 |
| SY13 | **Highlights and notes.** Two versions of a highlight, or of a note, conflict when neither descends from the other and their contents differ; equal contents are not a conflict. The reader decides (item 100). The choice is a new version descending from both, so it settles on every device. Until then every device keeps both, and the one changed last shows in the text; neither is lost or exported as deleted. A change and a deletion conflict in the same way, within item 102's limit: until the reader chooses, the thing stays deleted in the library and the changed version waits in the conflict, on every device. Deleting a highlight deletes its note, and carries a causal frontier covering both, as SY18's does for a book: a note added or changed apart, even the highlight's first note, which the deleting device never saw, makes the deletion a conflict. Three or more devices: the reader chooses one of all of them | Decision 10-14; A8 | 1, 5 |
| SY14 | **The library list.** A book added on any device joins every library; its file comes from storage (SY23). Last opened and finished: the newest wins (item 105). Only a deletion removes a book (SY17) | #73 | 1, 4 |
| SY15 | **Settings (item 103).** Newest per key, for these keys only: `theme`, `fontPx`, `lineSpacing`, `fontFamily`, `pageWidth`, `publisherStyles`, `singleKeyShortcuts`, `pageTurnCrossfade`, `pageTurnAnnouncements`, `language`, `librarySort`, `libraryView`, `readingPace`, and per book its Pages or Scroll and Simplify styles. Device-only: `openAtLaunch`, `pinnedExtensions` (extensions don't sync), the docked Navigator tab (it depends on the window's width), safe mode and the sync setup. A key on neither list stays on the device, so a new setting syncs only when someone adds it here | Decision 10-14 | 3 |
| SY16 | **An updated file (B3) across devices.** When one device imports a new file of a synced book, the book's current hash changes; the other devices replace their copy and re-anchor, and anything unplaceable goes to “Couldn't place” (A8). Two new files imported while apart: the newest import wins (item 105). The other file leaves storage with SY17's week | B3, A8 | 4 |

### 4.4 Deletion

| ID | Rule | Source | Stage |
|---|---|---|---|
| SY17 | **Order and the week.** A deletion removes the local copy first, as today (A7's Undo; G4's purge at the next launch), and then marks the record deleted in storage. Its content (a book's file, its metadata and the records that go with it; a highlight's quote and note) stays in storage for 7 days from the deletion, through any compaction (SY11). A book file that isn't in storage yet (imported or updated while offline, or while the storage was full) is moved aside in the library folder rather than purged, kept out of the library, until its copy in storage is confirmed; with sync off, G4's purge is unchanged. **Expiry is skew-safe:** a device removes the content only when 7 days have passed since the deletion by its own clock *and* by the latest clock reading of another device that has synced since the deletion, when there is one; a device marked as running ahead (SY10), or whose clock jumped by more than 5 minutes against its monotonic clock since its last pass, removes nothing. A clock moved forward therefore never shortens the week on other devices. The record's tombstone (key, version, time, and no text) stays for good, so a device back after any length of time learns of the deletion | Decision 10-14 | 1, 5 |
| SY18 | **A book's removal** removes its highlights, notes, place and per-book settings on every device, as G4 does locally; they are kept and restored with it. The removal carries a causal frontier for the whole book: for each device, the latest of its changes to the book or anything in it that the removing device had seen. Any record of that book with a version outside the frontier (its file, SY16; its highlights and notes, perhaps with a UUID the removing device never knew; its place, reading state and per-book settings) is not removed with it. Instead it makes the removal a conflict the reader decides, as item 102 says (“‹Title› was removed on ‹device›, but it changed here”), so a new edition is never discarded silently. On the device that removed it, where the book's Notes tab is gone, the conflict is decided in Recently deleted (item 101). A book open on another device stays open there and goes when it closes | G4 | 1, 4 |
| SY19 | **Restore (item 101).** Within the week, Restore makes a new version descending from the tombstone, so the thing comes back on every device; a book comes back with its highlights, notes and place. After the week, Restore isn't offered | Decision 10-14 | 5 |
| SY20 | **A device away for more than a week.** It applies every tombstone it finds before it sends its own changes, and never sends a version that would bring back what it hadn't changed. A thing it did change after the last sync it shared with the deleting device (item 102) is sent as a conflicting version, with its body: it doesn't come back in the library, but every device can show the conflict (SY13, SY18), and the reader's choice is sent as a version descending from both | Decision 10-14; #73 | 1, 5 |
| SY21 | **Missing is not deleted.** An object missing from storage, a storage that's gone or a file that can't be read never deletes anything on the device; only a tombstone does | Prototype; §6.3 | 1 |

### 4.5 Books

| ID | Rule | Source | Stage |
|---|---|---|---|
| SY22 | **Small things first.** Places, highlights, notes and settings go before book files, so a large upload never delays them | #73 | 4 |
| SY23 | **Book files.** Each is stored once as `books/‹sha256›.epub`. A file from storage is imported through the same pipeline as a file from Finder: validation, zip and XML limits, DRM refusal, B3 (plan §7.1). Its hash must match its name and its record, or it is refused. The refusing device notes the bad object in its `devices/` file. Any device that holds a good copy with that hash replaces the object (written to a temporary name and renamed), and the refusing device fetches it again once it has been replaced, so a damaged object is repaired rather than refused forever | B3; plan §7.1 | 4 |
| SY24 | **A book still on its way** (prov.) appears in the library when its file has arrived; until then its place and highlights wait (Prototype), and Settings › Sync says “Copying 3 books”. No new tile state | Prototype | 4 |
| SY25 | **Full storage.** When a book doesn't fit but there is room for small objects, everything else keeps syncing. When nothing fits, every change waits in the journal, in order, and goes when there is room. Either way, Settings › Sync says what waits (“iCloud Drive is full · 4 books not copied”, or “iCloud Drive is full · changes wait on this Mac”), a quiet message says so once, and nothing on the device changes | #73 | 4 |

### 4.6 Safety

| ID | Rule | Source | Stage |
|---|---|---|---|
| SY26 | **Large removals wait (item 106).** A sync that would remove more than 10 books, more than a quarter of the library or more than 50 highlights asks first: “This would remove 14 books from this Mac · Remove · Stop syncing”. Stop syncing keeps everything | P0 (AGENTS.md, review guidelines) | 1, 5 |
| SY27 | **A consistent copy before every risky moment (item 106).** Before the first sync, before an SY26 removal, and before any sync pass that applies a remote deletion or replaces (SY16) or removes a book file, the library folder gets `Backups/‹date-time›/`. It holds the database, taken with SQLite's online backup API while the store's write lock is held, so rows still in the WAL are in it. It also holds APFS clones of `Books/`, which take no space until files change. A book file that pass replaces or removes is then in that backup, and the backup's database names it. Backups are kept 7 days, and the newest is always kept, whatever its age. Restoring one, as D2 describes (quit, put the database and `Books/` back), is a tested procedure, not only a promise. Each backup's database is marked as a backup, so a library restored from one starts with sync paused (“This library was restored from a backup · Resume · Turn off sync”) and applies nothing from storage. Resume sends the restored contents as new versions descending from what storage holds, as Restore does (SY19), so the removal or replacement that was undone is not replayed. Anything changed elsewhere since the backup, and not undone by it, still arrives | D2 | 1 |
| SY28 | **Storage is untrusted input.** Every record and change file is checked (its format, sizes, keys, versions, hash and signature, SY34) before it is used. One that fails is set aside, never applied, and counted in Settings › Sync. Each body type has its own size limit (prov., item 106), checked from the object's size before it is read and again while reading: a note or a quote at most 4 MB, a book's metadata 1 MB, a setting 64 KB, a change file or shard 8 MB; only book files may be larger (SY2). A local note longer than its limit stays on this device, and Settings › Sync says “1 note is too long to sync” | Plan §7.1, §6.4 | 1 |
| SY29 | **Joining, switching and turning off (item 109).** The first sync of a library that has books, into a storage that already has a library, merges the two (SY8); nothing is replaced wholesale. A storage in a newer major format is not joined. Turning sync off stops all reading and writing and keeps everything, on the device and in storage; turning it on again joins as above, with no duplicates | Prototype | 1, 3 |
| SY30 | **The storage's own states** (Stage 0 confirms each): a file not downloaded yet (an iCloud placeholder, a File Provider file without its data) is waited for and asked for, never treated as missing (SY21); iCloud Drive turned off or signed out, Google Drive for desktop not running, the `Linen` folder moved or deleted: a quiet line in Settings › Sync, nothing on the device lost, and sync resumes when the storage is back | Prototype | 0, 2 |

### 4.7 Status, privacy and trust

| ID | Rule | Source | Stage |
|---|---|---|---|
| SY31 | **Quiet status.** Settings › Sync (item 99) shows one status line (“Synced 2 minutes ago”, “Syncing…”, “Copying 3 books, 120 MB”, or what is wrong) and Sync Now. Nothing shows while reading except the messages SY12, SY13, SY25 and SY26 name: no badge, no icon in the reader, no notification | P§1 (“Nothing — notification, badge or extension UI — appears uninvited”) | 3 |
| SY32 | **When it syncs (item 107):** at launch, 10 s after the last local change, every 5 minutes while Linen is open, on quit (at most 2 s), and on Sync Now. Sync reads and writes storage outside the store's lock; applying a batch is one transaction | Prototype (lock finding) | 3 |
| SY33 | **What logs hold (D1).** No title, quote, note or setting value enters the crash log or a status line; they name counts and devices. The README's Privacy section says what sync copies and where, in the release that ships it | D1 | 1, 6 |
| SY34 | **Signed changes and enrolled devices (item 110).** Each device has an Ed25519 key pair; the private key is kept with its ID (SY7, *ThisDeviceOnly*), and the public key goes in its `devices/` file. Every change file, shard index and library claim is signed by its writer, and blobs and books are covered through the hashes that signed versions name. A device's key is accepted only when an enrolled device has signed it in. The first device enrols itself. A new device is enrolled when the reader confirms, on any enrolled device, a short code shown on both (prov.), or enters the library's recovery code, which the first setup sheet shows once. Until it is enrolled, a device's changes are set aside, never applied. So someone who can write to the folder, but holds no enrolled device, can't add a device, forge a deletion or change anything; at worst they can delete files, which SY21 never treats as a deletion | Security review | 1, 3 |

## 5. Architecture

### 5.1 Rust core (`src-tauri/src`)

| Area | Change |
|---|---|
| `sync/` (new) | `clock.rs` (SY10); `record.rs` (SY9, SY28: formats and checks); `merge.rs` (SY12–SY16, SY13's conflicts); `retention.rs` (SY17–SY21); `identity.rs` (SY7, SY8); `engine.rs`, one sync pass: read others' changes through the provider, merge, apply, write this device's changes, compact, purge the week (SY11, SY22, SY26, SY32) |
| `sync/folder.rs` (new) | The brokered folder: finding each root, path confinement, atomic writes, placeholders and downloads (SY2, SY30) |
| `store.rs` | Migration 6: `sync_device`, `sync_versions` (key, clock, device, history, body, deleted, sent), `sync_seen` (device, sequence), `sync_books` (sync ID, local book ID), `sync_conflicts`, `sync_aside`. Local writes of synced things add a version in the same transaction; applying a remote version doesn't (SY12) |
| `import.rs` | Import from a storage handle (SY23); B3 matching shared with SY8 |
| `extensions/manifest.rs` | `syncProviders`; `sync.provider`, `folder:icloud-drive`, `folder:google-drive` in `permission_consent`; `API_VERSION` 1.3.0 |
| `ext_commands.rs`, `extensions/worker.js`, `host.js` | `linen.sync` and `linen.folder`, permission-checked on every call; handles (SY3) |
| `extensions/builtin/sync-icloud/`, `sync-google-drive/` (new) | Two small providers that map `linen.sync` to `linen.folder` (SY1, SY4) |

### 5.2 WebView (`src`)

| Area | Change |
|---|---|
| `src/prefs/` | A Sync pane (item 99): storage menu, setup sheet (SY6), status (SY31), Sync Now, Turn Off, Recently deleted… (item 101) |
| `src/reader/` | The Notes tab's “Changed on two devices” group (item 100); the open-book message (item 104); listening for applied changes, so an open book's highlights update in place |
| `src/lib/strings/` | English strings; machine drafts in the four languages, for native review (#50) |
| `docs/extensions/` | A Host API 1.3 section and `linen.d.ts` |

### 5.3 How a sync pass runs

```
core engine ──► host ──► provider Worker (list/read/write/remove)
                              │
                              └──► linen.folder.* ──► host ──► sync/folder.rs ──► ~/…/Linen/
```

The engine holds every decision. The Worker sees names and handles only (SY3). The host checks each call against the provider's grants, as every Host API call is (P2).

## 6. Stages

Each stage is one PR (two where noted, to stay under about 1,000 lines) with its own Done-when list. Sync stays out of release builds (Settings › Sync and the two built-ins appear only in the test build) until Stage 6 is done.

### Stage 0 — The two folders on this Mac (spike, S)

- On macOS 13, 14 and 15 (D7: macOS 13 with Safari 16.4 is the minimum), with the owner's iCloud Drive and Google Drive for desktop, in a `Linen-spike` folder that is deleted afterwards. Nothing is written to either storage before the owner agrees to the spike.
- **To measure:**
  - Where each root is, and how the core finds it, including Google Drive with two accounts, and its stream and mirror modes.
  - Whether macOS asks the reader's permission before an unsandboxed app reads each folder, and what the prompt says.
  - Files whose data isn't on the Mac (“Optimise Mac Storage”, Google Drive streaming): how to tell, how to ask for a download, and what reading one does offline.
  - Whether a rename is atomic as another Mac sees it, and how long a new file takes to arrive there.
  - The names of conflict copies, and case-insensitivity.
  - That a *ThisDeviceOnly* Keychain item is not copied by Migration Assistant or a Time Machine restore to another Mac (SY7).
  - What happens offline, with iCloud Drive turned off, signed out, with the Google Drive app quit, and when the storage is full.
- **Report:** `docs/spikes/sync-folders.md`, with the rule changes it implies listed for the owner.
- **Done when:** each item above has a measurement on this Mac, and on a second device where an item needs one.

### Stage 1 — The sync core (L, Rust only, two PRs)

- **1a:** records, the clock, devices, identity, merge and conflicts, the week and tombstones (SY7–SY13, SY17, SY20, SY21, SY28), as pure code with an in-memory storage.
- **1b:** migration 6, the journal of local writes, applying remote versions, the engine, joining, backups and the large-removal guard (SY14, SY18, SY26, SY27, SY29), still with no UI.
- **Checks:**
  - Unit and property tests of the merge: commutative, idempotent, and the same result in any order of delivery.
  - Compaction then restore: a book with highlights deleted, its device compacted, then restored on another device, comes back whole.
  - Compaction then a late branch: a device back with an edit made from a compacted version is a conflict, and a descendant is not.
  - Expiry under skew: one clock 8 days ahead removes nothing.
  - Backups: with places and notes still in the WAL, and a book replaced and another removed by a sync weeks after the first, the newest backup restores all of them (SY27).
  - Compaction past the size limit: a library whose view exceeds one shard (SY11) compacts, publishes and is read back whole; so does a 9 MB note.
  - A note compacted into an offline device's view, then deleted elsewhere: after the week its text is nowhere in storage, and the offline device, back, applies the deletion (SY11, SY17).
  - Two devices setting up one empty folder at once end in one library (SY11).
  - A new edition imported on one device while another, apart, finishes the book: both survive (SY9, SY14, SY16).
  - Two different new editions imported apart: every device ends with the same file and that file's title and authors (SY9, SY16).
  - A highlight deleted on one device while another, apart, adds its first note: a conflict, and the note isn't lost (SY13).
  - Two devices uploading the same book or blob at once: one object stays, and no conflict copy remains (SY11).
  - A device that was never enrolled writes valid-looking changes, a deletion among them: nothing is applied (SY34). A 900 MB note blob is refused before it is read (SY28).
  - A note deleted while its highlight stays is restorable for the week (SY9, SY19).
  - Two library folders with one device ID and different writer tokens (a migrated Mac): one takes a new ID and no file is overwritten (SY7).
  - A book removed on one device while another, apart, adds a highlight to it or imports a new edition: the removal becomes a conflict and nothing is lost (SY18).
  - A device that was behind when another compacted resumes from the view and the later change files, missing nothing and waiting for nothing already in the view (SY11).
  - A backup restored after a sync removed one book (below SY26's threshold), relaunched with the storage available: sync is paused; after Resume the book is back on every device.
  - The soak test: three devices, seeded random operations with offline spells, clocks out by hours, late, repeated and reordered delivery, restarts, compaction and a device away for 8 days. After each run every device holds the same data, nothing deleted came back, and no note text was lost except by a deletion. It runs in CI with a fixed seed.
  - Data safety (plan §6.3): migration 6 from every earlier schema's fixture; a kill during a sync pass loses nothing committed.
  - B3 joins: two libraries with the same book under different local IDs; a reused identifier with another title stays two books (B3-guard).
- **Done when:** the checks pass, and `scripts/ci-local.sh` is green.

### Stage 2 — The slot and the providers (M)

- Host API 1.3 (SY1–SY6), the two built-ins, `docs/extensions/` updated. In the test build, an override points both roots at temporary folders.
- **Checks:**
  - Manifests: `syncProviders` accepted with `sync.provider`, refused without it; 1.2 extensions still install.
  - Confinement: `..`, absolute paths, symbolic links in and out, hidden and over-long names, and objects over the limits are all refused (SY2).
  - The Worker never receives a body's bytes (SY3).
  - Before setup, no file under either root is opened (SY4, SY6), shown by the host's log of folder calls in the test build.
  - Safe mode: nothing syncs (SY5).
- **Done when:** the checks pass and CI is green.

### Stage 3 — Turning it on: places, highlights and notes, settings (M–L)

- Settings › Sync, the setup sheet, status, Sync Now, Turn Off (items 99, 109); enrolling a device and the recovery code (item 110); timing (item 107); the settings list (item 103); the open-book message (item 104).
- **In-app checks** (two library folders, run one after the other against one temporary storage. In the test build, each library's device store is a file beside its `LINEN_DATA_DIR`, outside the library folder, so each library is one stable device and SY7's renewal is exercised only on purpose):
  - `SY-place-resume`: read to a place in library A and quit; open the same book in library B, and it resumes there (#73).
  - `SY-highlight-roundtrip`: a highlight and its note made in A appear in B; editing it in B shows in A.
  - `SY-settings`: a theme and text size set in A apply in B; a device-only setting doesn't.
  - `SY-open-book`: with the book open in B, a newer place from A shows the message and doesn't move the page.
  - `SY-nothing-without-setup`: with no storage chosen, no file is opened under either root, and `EP1`-style network checks still see no request.
- Captures for approval: Settings › Sync (off, set up, an error) and the setup sheet, in Paper and Night.
- **Done when:** the checks pass, the full in-app suite passes, and the captures wait for approval.

### Stage 4 — The library list and books (M)

- SY14, SY16, SY18, SY22–SY25.
- **In-app checks:**
  - A book imported in A appears in B with its highlights.
  - An updated file in A replaces B's copy, and its highlights re-anchor or go to “Couldn't place”.
  - A book file damaged in storage is refused and fetched again.
  - With room for small objects but not for a book: places and notes still sync, and the message shows once. With no room at all: nothing is lost, and every change goes, in order, once there is room.
  - A book object damaged in storage is replaced by a device with a good copy, and the refusing device then receives it.
  - A book removed in A while open in B stays open, and goes when it closes.
- **Done when:** the checks pass, and a 500-book library (the cold-start fixture) syncs without breaking the memory budget (plan §6.4).

### Stage 5 — Deletion, restore and conflicts (M)

- Recently deleted (item 101), the conflict group and its message (items 100, 102), the large-removal prompt (SY26).
- **In-app checks:**
  - A highlight deleted in A goes from B; restoring it in B within the week brings it back in A.
  - With both libraries' clocks moved 8 days on (an injected clock in the test build), the deleted things are no longer restorable and don't come back; with only B's clock moved, nothing is removed from storage and A can still restore.
  - A book imported while the storage is unavailable, deleted, Linen restarted, the storage back: the book is restorable.
  - The same note changed in A and B apart: the group shows both; Keep this settles it in both.
  - A deletion in A and a change in B apart: as item 102 says.
  - Removing 11 books asks first.
  - A book removed in A while B, apart, adds a highlight: A's Recently deleted lists it with Keep the book; B's Notes tab shows the conflict; either choice settles both.
- Captures for approval: the conflict group, Recently deleted and the large-removal prompt.
- **Done when:** the checks pass and the captures wait for approval.

### Stage 6 — Real devices and release (M)

- Two real devices (a Mac and the second platform, per Decision 10-14) stay in step over a week of real reading through iCloud Drive and three days through Google Drive. The record goes in the status document; it confirms the soak test in real use and doesn't replace it.
- `docs/release-acceptance.md` gains the sync checks; the README's Privacy section says what sync copies and where (SY33); sync appears in release builds.
- **Done when:** the record is complete, the owner has approved the captures, and the release candidate passes acceptance.

## 7. Edge cases the checks must cover

From the prototype's list, with Decision 10-14's rules:

- **A cloned device.** A library folder restored on a second device, or an older copy restored on the same one, gets its own device ID (SY7).
- **A clock ahead.** Minutes ahead: named in Settings › Sync, and a concurrent place may go to the device that is ahead (SY10). More than a day: set aside, never applied silently.
- **Compaction during the week.** A thing deleted, then compacted away from the change files, can still be restored with all it had (SY11, SY17, SY19).
- **Two full libraries meeting.** No duplicate books or highlights (SY8, SY29).
- **A newer format.** Records from a newer Linen are kept and passed on, never rewritten or dropped (SY9).
- **Ping-pong.** The same book open on two devices (SY12).
- **Reading backwards on purpose.** The newest place wins (SY12).
- **Undo.** Undo after a delete is a newer version, so the thing comes back everywhere (SY19).
- **A conflict about a conflict.** Two devices settle the same conflict differently while apart: shown again (SY13).
- **A different edition on each device.** Unplaceable highlights go to “Couldn't place”, never dropped (SY16, A8).
- **The storage misbehaving.** Placeholders, half-synced files, conflict copies, the folder moved, the storage full or signed out (SY11, SY25, SY30).
- **A storage that lost files.** Never a deletion (SY21).
- **A device away for months.** Tombstones last (SY17, SY20).
- **Per-book keys.** `simplifyStyles:<bookId>` and `book_settings` name local IDs and are translated (SY8).
- **Safe mode, and a provider turned off.** Nothing syncs; Settings › Sync says why; nothing is lost (SY5).
- **A provider stuck or suspended (P6).** Sync pauses, with Restart in Settings › Sync, as other slots show.

## 8. Risks

1. **Losing the reader's books, notes or place** is P0. Mitigations: missing is never deleted (SY21); tombstones only; a book file is never purged before it is in storage, and expiry needs two clocks (SY17); a consistent backup of the database and the books before every pass that replaces or removes a book (SY27); large removals wait (SY26); the soak test (Stage 1).
2. **The folders behave differently across macOS versions** (iCloud Drive and Google Drive both use File Provider, which changes). Stage 0 measures them; SY30 keeps every storage problem quiet and harmless.
3. **The iPad (#80).** iPadOS reaches these folders only through the document picker and security-scoped bookmarks. Decision 10-14 says no iCloud container is needed; whether a folder kept open that way can be written in the background, and whether Google Drive's Files provider allows it, is unchecked. It must be checked when #80 is planned, before Stage 6.
4. **Size.** Books can be large and iCloud's free tier is 5 GB. The setup sheet says the size (SY6); a full storage only stops books (SY25).
5. **A slow storage blocks saving.** The prototype held the store's lock while reading the folder; SY32 forbids it.

## 9. #73's Done-when, traced

| #73 says | Rules | Stage |
|---|---|---|
| A sync slot in the Host API; iCloud and Google Drive as Built-in provider extensions | SY1–SY5 | 2 |
| iCloud into the iCloud Drive folder; Google Drive into the Google Drive for desktop folder | SY2, SY30 | 0, 2 |
| Opening a book on another device resumes at the place reached | SY12 | 3 |
| Highlights and notes, settings, the library list and books the same everywhere; extensions and dictionaries not | SY13–SY16, SY22–SY24; §1.2 | 3, 4 |
| The reader decides between two changes made apart | SY13 (items 100, 102) | 5 |
| Deletion: local first, then storage; kept a week, restorable | SY17–SY19 (item 101) | 5 |
| A device away for more than a week brings nothing back | SY17, SY20 | 1, 5 |
| Only storage the reader chose; with none, nothing is sent | SY4, SY6 | 2, 3 |
| The same book recognised by B3 | SY8 | 1 |
