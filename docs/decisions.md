# Decision record

Owner decisions taken after plan revision D. IDs refer to `docs/implementation-plan.md` §10. The plan text is not edited; this file is the record.

Since 2026-10-14, a choice the owner left to Claude (“make your best calls”) is recorded as **Delegated to Claude**, not as approved, and a question nobody answered stays open and is not built (see “Decided 2026-10-14”).

| # | Decision | Date |
|---|---|---|
| Scope | Develop and verify on **macOS only** for now. Windows and Linux work (WebView2, WebKitGTK, NVDA, Orca, Windows high contrast, Linux packaging) is deferred, not dropped. Spike criteria that compare engines are recorded as deferred. CI builds macOS only. | 2026-09-24 |
| D3 | **Direct download:** a signed, notarised DMG, not the Mac App Store. No App Store sandbox, so the import pipeline and extensions can use normal file access. | 2026-09-24 |
| B3 | **Import semantics.** Books are copied to `~/Library/Application Support/app.linen.reader/Books/`. Same content hash → already in the library: no second copy, open the existing book. Same OPF `unique-identifier` with a different hash → an updated file: replace the copy and re-anchor annotations (anything unplaceable goes to “Couldn’t place”, A8). | 2026-09-24 |
| T5 | **UI font:** the platform font (`system-ui`, SF Pro on macOS), as V3 states. Literata (OFL) is bundled as the reading fallback. Instrument Sans is not shipped. | 2026-09-24 |
| B7 | **One window, one open book** in the MVP. | 2026-09-24 |
| D5 | **Accessibility target:** WCAG 2.2 AA, plus the design’s AAA 7:1 body-text contrast (X1). | 2026-09-24 |
| Dependencies | **How dependencies are kept current (R8; delegated to Claude, 2026-10-14).** The owner chose Dependabot and audits; the details are Claude's: weekly Dependabot PRs for Cargo, npm and GitHub Actions; minor and patch updates grouped per ecosystem, majors alone; at most five open per ecosystem; foliate-js ignored (pinned and patched). The Audit workflow (`pnpm audit --prod`, `cargo audit`) runs on every change and weekly, and is not a required check. | 2026-10-14 |
| Rust and corpus | **One Rust, and the corpus in CI** (owner: “Start Phase 1 fixes” of the process review). `rust-toolchain.toml` pins the Rust that CI, release builds and `scripts/ci-local.sh` use (1.99.0, CI's `stable` on 2026-10-13); a bump is its own PR. CI and `ci-local.sh` fetch and generate the test corpus, and with `LINEN_REQUIRE_CORPUS` its tests fail rather than skip when a file is missing. Downloading the public corpus in CI: delegated to Claude. | 2026-10-14 |
| Tauri updates | **Tauri is updated by hand, Rust and npm together (delegated to Claude, 2026-10-14).** The Tauri CLI refuses a build whose crates and npm packages differ in major.minor, and Dependabot updates one ecosystem at a time, so its Tauri PRs fail (#65). Dependabot ignores the `tauri*` crates and `@tauri-apps/*`; one hand-made PR moves both. `minisign-verify` and `base64`, used by `examples/verify_update.rs`, follow `tauri-plugin-updater`'s versions in that PR, so the release check verifies as the updater does. `tauri-apps/tauri-action` is also left out until its move to v1 is tried on a release (#75), and `@types/node` takes no major updates beyond the Node that CI and the app run (24). | 2026-10-14 |
| Old PRs | **The open PRs from 2026-10-02 (R7 triage).** #7 (author sort, sync, cross-engine results, TypeSafe experiments) is closed with its branch kept: its migrations 4 and 5 collide with main's (dictionaries, extension secrets), and any part still wanted returns as a small PR with migrations from 6. #8 (the icon on the macOS grid, item 42) is rebased and finished. #9 is closed: its AGENTS.md was superseded by #47 and NAVIGATION.md by `docs/development.md`; its rule index is redone on its own (#59). Delegated to Claude: the generated index names decision sections, not line numbers, so a new decision elsewhere doesn't make it stale; it writes each plan row under its own column headers; and `OTHER_SECTIONS` in `scripts/rule-index.py` names the decision sections whose IDs belong to other work (the Reading Lens gates, the reader-panel questions). The owner chose to defer the index's remaining gaps to issue #60 rather than keep polishing it: it is a helper, and this file is the record. | 2026-10-14 |
| Zip entry names | **How zip 9's fallible names are handled (delegated to Claude, 2026-10-14; #74).** A book or extension package with an entry name that is neither UTF-8 nor CP437 is refused as not a zip (`Rejection::NotZip`), at validation, before anything else reads it. A name stored in CP437 (no UTF-8 flag) is found by its decoded form, as zip 8 found it; the package parser builds that lookup once (`DecodedNames`), so a book with many such names isn't rescanned per lookup. | 2026-10-14 |
| After the sync protocol | **Delegated to Claude (owner, 2026-10-18: “Make your best calls”).** #103, which only records the owner's answers, is merged by Claude after clean reviews; #102 is fixed by updating `ammonia` to 4.2.4 (RUSTSEC-2026-0373) in its own PR, merged by Claude once CI and Codex are clean; Stage 1a (#92) starts next, built from `docs/sync-protocol.md`, with each Stage 1 PR waiting for the owner's merge. | 2026-10-18 |
| Audit ignores | **zip 4.6.1's advisories are ignored in the audit (owner, 2026-10-17, #88, option a).** RUSTSEC-2026-0367, -0368 and -0369 are in zip 4.6.1, which comes only from `tauri-plugin-updater` 2.13.3 (the latest stable) and only for Windows and Linux; the macOS app doesn't compile it, and the plugin's `zip` feature also brings the `tar` and `flate2` macOS updates use, so it can't be turned off. `.cargo/audit.toml` ignores exactly these three IDs, with the reason, until the updater depends on zip ≥ 9; any other advisory still fails the audit. | 2026-10-17 |
| A highlight's sync body | **Delegated to Claude (owner, 2026-10-18: “Make your best calls”; #108).** A highlight's body (A9's Web Annotation JSON) names its book by the book's sync ID (SY8), never the local ID, and carries no note text: the note is its own record (SY9). | 2026-10-18 |
| SY26's guard (Stage 1b.3a) | **Delegated to Claude (owner, 2026-10-18: “Make your best calls”).** How item 106's approved values are applied, before Stage 4 and Stage 3 add books and the question's wording. (1) The change that would take a count past a threshold isn't applied: it is rolled back and waits, with nothing more applied, until the reader answers; Apply lets exactly that change through, and it isn't counted again. (2) The thresholds counted now, each in 24 hours and in total since the last answer: more than 50 highlights and notes deleted, 200 added, 50 highlights, notes and settings rewritten, the places or reading states of 20 books, or 50 MB of highlights and notes added to one book. Books and book files are counted from Stage 4. (3) A first sync is counted like any other, so joining a large library asks once. (4) Stop syncing puts each record changed since the last answer back to what it was before the first of those changes, as a new local version, only where the library still shows the last of them and the reader hasn't changed it since. | 2026-10-19 |
| SY27's backups (Stage 1b.3b) | **Delegated to Claude (owner, 2026-10-18: “Make your best calls”).** How item 106's approved values are applied. (1) A first sync is a pass before which nothing has been received from storage. (2) A backup taken within the hour covers a pass if none of the records the pass deletes has a journal row newer than the backup; the journal records every local and remote change, so that is “holds every row the pass could remove”. (3) Until Stage 4 syncs book files, `Books/` is cloned whole (APFS clones, so no space until a file changes). (4) Of the backups from the last 7 days, the newest 10 are kept, the newest whatever its age, and, while SY26 asks, the newest from before the first change it counted. (5) Resume's two choices (selective restore, or this copy wins) are built with Restore in Stage 1b.3c. | 2026-10-20 |
| SY18 moves to Stage 4 | **Answered by the owner (2026-10-20, as recommended).** SY18 (a book's removal, its frontier over the book's records, and the conflict when another device changed the book) needs book records, which sync journals from Stage 4. It is built there, with them, not in Stage 1b. Stage 1b.3c builds Restore (SY19) and the deletion ages Restore's week depends on (SY17). | 2026-10-20 |
| When Resume is built (115, SY27) | **(a), approved by the owner (2026-10-20).** SY27's Resume (bring back what was removed or replaced, or make this copy win) is built in Stage 2, once the device's frontier (Stage 1c) and storage (Stage 2) exist, with SY27's restore check. Until then a library restored from a backup stays paused. | 2026-10-20 |
| Restore and deletion ages (Stage 1b.3c) | **Delegated to Claude (owner, 2026-10-18: “Make your best calls”).** (1) A deletion's age starts when it is first seen here: a local deletion when it is made, another device's when it is applied; the time is the one this device can vouch for (SY17), kept by Stage 3. (2) Restore brings back the newest version the tombstone descends from whose body is still here; Restore isn't offered for one whose body is gone (Recently deleted lists only what is still held, SY17). (3) A note is deleted with its highlight when its tombstone came in the same change (same writer and sequence, or both unsent), or when this device deleted the two together, even if one was then held for a choice and sent apart; Restore brings that note back too, or restores neither. A note is restored only where its highlight shows. (4) Where this device's version waits in a conflict, the restored version descends from every head, as a choice does, and is sent. (5) A body over its limit (SY28), never synced, is kept with its deletion; Restore brings it back on this device only, unsynced as before, and doesn't offer it again. | 2026-10-20 |
| SP4's exact inputs and the envelope (Stage 1c.1) | **Approved by the owner (2026-10-21: “Approve them”)**, after being Delegated to Claude (2026-10-18, “Make your best calls”). Where `docs/sync-protocol.md` leaves the bytes open. (1) SP6's signature input is `len‖"linen-sync/sig/v1" ‖ 0x00 ‖ len‖T ‖ 0x00 ‖` then the fixed fields: the domain and the type are length-prefixed like every variable field (SP4), and the two `0x00` bytes are literal. (2) `vroot` hashes the version list as one variable field, length-prefixed; each entry is the first 16 bytes of plain `H(key)`, then the version hash. (3) The recovery code's check, salt and Argon2id password are the 26 characters as ASCII, a fixed field with no prefix. (4) The fingerprint is shown as 4 groups of 4 separated by spaces, as the recovery code's 7 groups are; the short code as `472 913`, zero-padded. (5) An envelope with a field beyond SP5's four, no signatures, one device named twice or one signature twice is refused: nothing in the envelope but the payload and the signatures is signed, so nothing else is read. Several code keys may sign one file, as SP19 requires for a `remove`, `confirm` or `merge` (the current code's and the next code's); each is checked with the key its head names. *Amended 2026-10-21 after #116's security review: the text approved that morning refused “one signer twice”, which would have refused every removal SP19 describes. The owner confirmed the amendment the same day (“Follow SP19”).* (6) A device's signature counts only with the key its ID names (SP1), checked at every verification. (7) The suite's crates: `ed25519-dalek` 3, `x25519-dalek` 3, `chacha20poly1305` 0.11, `hkdf` and `hmac` 0.13, `argon2` 0.6, `getrandom` 0.4, the releases that share `sha2` 0.11 with the rest of the tree. `scripts/sync-vectors.py` makes the test vectors SP4 asks for with libsodium, the reference Argon2 and Python's hashlib, so the Rust is checked against other implementations of the same reading. | 2026-10-20 |
| The device file (Stage 1c.3) | **Delegated to Claude (owner, 2026-10-18: “Make your best calls”).** Where SP16 leaves the bytes open. (1) `clock` is `{ "hlc": [ms, n], "ahead": bool }`: its HLC reading (SP10) and whether it is marked as running ahead (SY17). (2) `name` is cut to 64 Unicode characters, not bytes. (3) When the file would pass 256 KB, whole lists move to overflow files, the largest first, each split into parts of at most 8 MB, and the moved list holds no items in the file itself. No manifest is written yet; a file whose overflow list alone is too long is refused when written. (4) Only this device's newest device file waits in the device store for upload: a newer one supersedes it, with its parts uploaded before the file that names them. (5) A peer's device file with a higher counter is refused when its `own` names a change before its floor's, the floor's sequence with another hash, or no change while the floor has one; accepting it raises the floor to its counter and, when newer, its `own`, once its overflow parts have all been fetched and checked. (6) One device file's overflow parts may build at most 250,000 list items and fields in memory (prov.); past that they are refused before the rest are built, and this device never signs a file past that budget. (7) One change file's payload may build at most 1,000,000 values (prov.): a peer's is refused past it, and this device numbers no more versions into one change than fit it. A run of a peer's changes is checked by its sequences, links and hashes alone, then each change is decoded and applied one at a time. | 2026-10-21 |
| The membership records (Stage 1c.4a) | **Delegated to Claude (owner, 2026-10-21: “Make your best calls”).** Where SP19 leaves the bytes open. (1) A record's `clock` is an HLC reading `[ms, n]`, as in versions (SP10). (2) A device enrollment's `reveal` is the base64url of the reveal's exact payload bytes, and `pop` the new key's SP6 signature over it (type `enroll-reveal`, seq 0). A code enrollment carries neither: the new key signs the record itself. (3) A `label` longer than 64 characters is refused, not cut. (4) A `confirm` carries exactly one of `confirms` and `admits`, and `admits` is not empty. (5) A rotation to the identity it rotates from is refused. (6) A record whose own checks fail (a key its ID doesn't name, a missing new-key or next-code signature, a reveal for another enroller) is refused when opened, before the membership looks at it. (7) A merge names at most 16 heads (prov.): it consumes the unmerged sealed tips of a library of at most 12 devices, and a bound keeps a folder writer from filling memory with merges of hundreds of made-up heads. | 2026-10-21 |
| Computing membership (Stage 1c.4b) | **Delegated to Claude (owner, 2026-10-21: “Make your best calls”).** How SP20's rules are read where it leaves room. (1) Records are judged in the order the graph allows, ties broken by when this device first saw them, then by hash; where two records conflict (a second rotation of one identity, a code enrollment and a removal on one head), the one judged first stands. (2) The enrollment quota: a device's enrollment `n` needs its lineage's `n − 1` held here, and its `n − 2` first seen at least 24 hours ago. Under the signer, and under each of its ancestors up to and including the first whose own enrollment was first seen more than 24 hours ago, at most 2 enrollments first seen in the last 24 hours count, this one included. (3) A rotation needs the code of the newest sealed head at its predecessor when its lineage already has 2 counting rotations first seen in the last 30 days. Rotations in a joining device's checkpoint count too, as first seen when it joined, so a device that just joined errs towards asking for the code. (4) A removal takes whole rotation lineages and every lineage they enrolled, directly or through others, whatever its `cascade` flags say. (5) A second rotation of one identity counts for nothing until 1c.6 makes an equivocation proof of it. (6) Sibling sealed heads, `confirm` and `merge` wait (`Fork`) until 1c.4c builds them, so nothing is trusted from them meanwhile. (7) One computation judges at most 1,024 records (prov.), in the order the graph allows, oldest first sight first; the rest, and records whose predecessor isn't held, wait unjudged. Records this device has accepted were seen before any new ones, so they are always inside the bound. | 2026-10-21 |
| Sibling sealed heads, confirm and the cap over siblings (Stage 1c.4c-i) | **Delegated to Claude (owner, 2026-10-23: “go”, under 2026-10-21’s “Make your best calls”).** How SP20 (1), (2), (4) and (4a) are read where they leave room. (1) This device's own view of which head is overtaken is the newest of the sealed heads its device store holds (SP21). A removal not already accepted, built where the newest sealed head at its predecessor is neither that head nor after it, waits for a `confirm`. A device that holds neither of two sibling removals, or holds both, sees them as siblings: both count and their removals unite. With siblings held, there is no newest, and a third removal on their common head counts as a sibling too. (2) A `confirm` links to the removal or enrollments it names: in the graph they are its ancestors, so they count from it on as if made on its `prev`, the confirm is newer than them, and a confirmed removal's own code is overtaken and never a code. (3) A `confirm` counts when its `prev` is a counting sealed head no other counting sealed head has overtaken, it carries that head's code, it names a removal (waiting for it, or counting already as a sibling) or enrollments the cap held back that fit under 12 together, and its device signer is trusted at `prev` and removed neither by the confirmed removal's closure (its targets resolved to lineages where the removal was made) nor by any counting removal. (4) Siblings for the cap are enrollments apart in the graph built on one sealed head or on sibling sealed heads; an enrollment on an older head than the other's isn't its sibling, so a newcomer never holds back a device enrolled long before. When one waits for the cap beside a counting sibling, all such contending enrollments wait, until removals make room for every one of them or a `confirm` admits some; those it didn't admit are refused, and no later confirm can admit them. A contender refused for its own reasons leaves the pool. All contending enrollments form one pool. (5) A code enrollment on a sibling this device treats as overtaken is refused, since that sibling doesn't count here. (6) Judging repeats, each pass using what the previous one found (removed devices, confirmed removals, admitted and contending enrollments, overtaken records, found again each pass) until nothing changes, at most twice the graph's size. (7) A removal built where a counting confirm or merge, after its base and neither below nor after it, consumed the base's code waits for a confirm, on every device: a confirmed removal's code never seals again. A confirm and a removal made apart on one head conflict, and the one judged first stands; an `admits` confirm counts only from the pass that finds the contention it settles, so a removal judged in the first pass can stand over it. | 2026-10-23 |
| Merge and suspension (Stage 1c.4c-ii) | **Delegated to Claude (owner, 2026-10-23: “go”, under 2026-10-21’s “Make your best calls”).** How SP20 (5) and its suspension are read where they leave room. (1) While the sealed heads that no other descends from (the tips, counting or waiting here for a confirm as a sibling this device found overtaken, but not removals on a code a confirm or merge consumed) number more than one, the fork is the records below every tip. A lineage whose first enrollment lies outside the fork was enrolled after it, on the branches of the tips it is below or after. This device (`Inputs::me`) suspends every such lineage when its own lineage was enrolled at or before the fork, or it hasn't joined, and otherwise only those sharing no branch with its own. A lineage enrolled on several branches (one key enrolled on each) is on all of them. A rotation after the fork of a lineage enrolled before it isn't suspended. (2) What a suspended lineage signs: its enrollments and rotations wait (`Wait::Suspended`), and `Membership::trusted` leaves it out. Its removals still remove (SP20: removals only take trust away). Its confirms are judged by the confirm's own rules, since judging starts with nobody suspended and a confirm that ends its signer's fork counts at the fixed point either way; #128 asks whether to bar them. (3) A merge's heads are exactly the current unmerged tips judged before it: sealed heads, each counting or waiting here for a confirm (not a removal on a code a confirm or merge consumed), which the merge then confirms. A merge that a sealed head after one of its heads overtook is refused, and so is a device enrollment not below the merge whose branch builds on a head the merge overtook, however many records after that head it is. (4) The merge's device signer is an identity of a lineage trusted at the fork that the branches' united removals don't take; otherwise every head's code must sign. (5) `keeps` names each device once, and exactly those whose enrollment lies below the heads, outside the fork, and isn't refused. A device not kept is removed as by a removal (its lineage and what it enrolled, except what the merge kept), with the keep's cutoff; no removal's cascade takes a device a merge kept, though a removal naming it does. (6) The cutoff exception for removals by a signer the reader never approved is left to 1c.6 (#128), with the reverting it governs. | 2026-10-23 |
| Frontiers and accepted by all (Stage 1c.6a) | **Delegated to Claude (owner, 2026-10-24: “go 1c.6”, “Make your best calls”).** Where SP29 leaves room. (1) A `frontier` or `inherited` entry not of SP16's shape (a field missing, a sequence of 0, a signature that doesn't read) vouches for nothing and is left out. (2) An entry whose change this device doesn't hold, or whose named device's key it doesn't know, is pending: neither counted nor refused. One whose hash, or whose signature, the chain contradicts is false, refused and its writer named. (3) A device file vouches for each device up to the higher of its resolved `frontier` and `inherited` entries. (4) The marks are kept in the device store's `acceptedByAll` as `{ ‹device›: { "s", "h" } }`. A mark never moves back, keeps its place when the change it would move to isn't held here, and stays for a device no longer enrolled, since removing one drops only what it alone vouched for. | 2026-10-24 |

Still open before Phase 1 exits: B1, B4, B9, B10, G4 import states, G6 (macOS-only scope reduces G6 to the macOS menu bar).

## Signed off 2026-09-24 (found during Phase 0–1 work)

The owner approved all four sign-offs. The values below are no longer provisional.

| # | Finding | Approved decision | Evidence |
|---|---|---|---|
| C7 | The approved Night highlight tints (S4, 16% alpha) give highlighted text 8.2–8.8:1, under X1's 9:1. | Night tints at 11% alpha (≥ 9.2:1 for all four colours, underlines unchanged). Sepia tints (not drawn in S4) use the Paper tints at 75% over the Sepia ground; the Sepia search outline is `#8C5A45` (3.9:1). Night search marks are derived from the Night accent. All marked PROVISIONAL in `src/lib/theme/tokens.ts`. | `src/lib/theme/tokens.test.ts` |
| B3-guard | B3 treats the same OPF identifier with different bytes as an updated file. Identifiers are sometimes reused or left as placeholders, so a different book would silently replace another. | An updated file must also have the same package title (case and spacing ignored); otherwise it imports as a separate book. | `src-tauri/src/import.rs` test `a_reused_identifier_with_a_different_title_is_a_new_book` |
| D-E1 | Plan §7.1 isolates book iframes by sandboxing them without `allow-scripts`. On WebKit that breaks every parent event listener (WebKit bug 218086), which foliate-js needs. | Isolate with the app CSP (`script-src 'self'`, `frame-src blob:`, `form-action 'none'` …), which WebKit applies to book blob documents, plus a `default-src 'none'` meta CSP injected into each book document. Spike E blocked every probe with this design. | `docs/spikes/e-content-isolation.md` |
| D7-WebKit | foliate-js's paginator uses a regex lookbehind, which WebKit parses only from Safari 16.4. macOS 13 shipped with Safari 16.0–16.3, so the reader engine fails to load on an un-updated macOS 13. | Minimum: macOS 13 **with Safari 16.4 or later installed**. Say so in the system requirements; on older WebKit the app should show an “update Safari” message rather than a blank reader (Phase 8). Found 2026-09-24 while preparing Spike B for a macOS 12 MacBook Air (Safari 15.6.1). | `node_modules/foliate-js/paginator.js:655` |
| D-D1, D-X1 | Spike D page turns across chapters (27 ms against 16 ms); XML entity hang in WebKit. | See `docs/spikes/decision.md`. | `docs/spikes/` |

## Recommended defaults adopted 2026-09-24

The owner chose the recommended defaults for the open behaviour questions (plan §10.3, §10.5). B11, B12 and B13 use the plan's own recommendations. The plan gave none for the others; the defaults below were proposed with this record and accepted on the owner's instruction to use recommended defaults. Any of them can be revisited.

| # | Question | Decision |
|---|---|---|
| B1 | What a “page” number means | Dynamic pages across the whole book at the current layout, used by the location line and the “Back to page N” chip (N2). Estimated from chapter sizes and shown with “≈” until pagination settles in idle time (L10, L16). Print page-list numbers appear only in Go to › Print page (N8), labelled “print page”. |
| B2 | “N min left in chapter” | Words left in the chapter ÷ the reader's pace. Pace is a rolling median words-per-minute over the last 30 minutes of page dwell times (each dwell counted only between 2 s and 2 min), kept as one number in `settings` on this device, with no per-session log. It starts at 250 wpm, and nothing is shown until 5 minutes have been read or when under 1 minute is left. |
| B4 | Highlight edge cases | Overlaps are allowed: selecting an existing highlight's exact range changes its colour, and a partial overlap creates a separate highlight (both drawn). A highlight lies within one chapter; a selection cannot cross a chapter boundary in Pages mode. No length limit beyond the chapter. Copy copies the plain text only. |
| B5 | Undo scope | Deleting a highlight, deleting a note and changing a highlight's colour are undoable (⌘Z, the Undo message, ⌘K › Recently closed). Typing inside a note uses the text field's own undo. |
| B6 | Search limits | The scan is not capped (Spike F: 19,959 hits for “the” in 24 ms). The Search tab renders 100 results per chapter, then “Show all N”. No space is reserved for regular-expression or whole-word options. |
| B7 | Windows and books | One window, one open book (decided earlier). |
| B8 | Scroll mode | One continuous scroll per chapter. At the end of a chapter, scrolling on moves into the next one, so reading is continuous without laying out the whole book. The location line and scrubber show the book-wide position, as in Pages. |
| B9 | DRM-protected files | Detected at import (`META-INF/encryption.xml` with any algorithm other than font obfuscation) and not added: ““file.epub” is protected by DRM and can't be opened in Linen.” No attempt to remove DRM. |
| B10 | UI localisation | Every UI string lives in one English message catalogue from the start, so translation is possible later. There is no translation or right-to-left UI mirroring in the MVP; book content keeps its own direction (I15). |
| B11 | What Esc from the Navigator returns to | The chrome state from before the Navigator opened (plan recommendation; implemented in `src/lib/reader/state.ts`). |
| B12 | Spread breakpoint | Measured on the width left after a docked Navigator, applied after the Navigator slide (plan recommendation). |
| B13 | History and Recently closed | Back history per book, in memory, last 50 jumps; Recently closed for the session, last 20 (plan recommendation; implemented). |
| T3 | Caret browsing (F7) on WebKit | Our own caret browsing in the book frame: F7 shows a caret, and ⇧ + arrows extend the selection with `Selection.modify`, which WebKit supports. |

Still needed before Phase 2 starts: Spikes B and C (owner at the machine) and the G8 designs (Scroll mode, two-page spread, fixed-layout zoom and pan, RTL and vertical writing, loading state and “≈” locations). G4 (library and import states) is needed by Phase 6, and its import states by Phase 1 exit; the import messages stay provisional until then.

**2026-09-24:** the owner approved the G8 designs (`docs/design/g8/APPROVAL.md`), which closes the last Phase 2 design input.

## M1 — How the memory budget is measured (2026-09-25)

The owner asked for the best call on the two questions raised by the memory investigation (`docs/phase2-status.md`, “Memory”).

| Question | Decision |
|---|---|
| RSS or physical footprint | **Both must be under 400 MB.** §6.4 names resident memory, so RSS stays. RSS alone is unreliable on a Mac under memory pressure: macOS compresses idle pages out of it, so the same run read 206–420 MB. The physical footprint (Activity Monitor's “Memory”, compressed pages included) is stable to within a few MB and is the stricter of the two, so a pass on both means the app really fits. |
| One run or many | **Median and p95 of 20 runs, as §6.4 says**, each in a fresh launch with a fresh data folder (`scripts/perf-memory.sh`). The budget holds when the p95 of both measures is under 400 MB. A single `LINEN_SPIKE=m` run stays as the quick check. |

The GPU process's fixed ~90 MB (window compositing, present before a book opens) counts, as §6.4 says “all processes”.

## Approved 2026-09-25 (Phases 3–6 review)

The owner reviewed the pending approvals and approved them as recommended. The designs and values below are no longer provisional; the PROVISIONAL notes in the code refer to this record.

| # | Approved |
|---|---|
| Baselines | Phases 3–6 visual baselines (see `docs/visual/APPROVAL.md`). |
| G10 | The `?` cheat sheet as built. |
| ⋯ menu | Every command, grouped, with shortcuts; unavailable ones at 40%; extension items join in Phase 7. |
| Footnote peek | Notes are copied as safe structure (paragraphs, emphasis), not in the book's own styles. |
| G5 colours | The derived Sepia and Night values for panels, segments, popovers, tooltips and the selection bar, and the Paper segment ring in secondary ink. |
| E3 card | The damaged-book card, built in Phase 6. |
| G11 | Re-attach: a pill (“Select the passage for …” · Cancel), then a bar with Attach here and Cancel. |
| Phase 5 details | Icons in the selection bar in every theme; Delete in place of Search for a clicked highlight; the note dot in the underline colour; the 3 px jump pulse; the note status wording; a steady 2 px caret. |
| D2 | One library folder is the whole backup; export of all highlights and notes as W3C JSON; restore by putting the folder back; uninstalling leaves the folder, and Settings says so. |
| G4 | Remove is immediate with Undo; the file and annotations go at the next launch; duplicates open the existing book; the item menu on right-click and a hover/focus button; a native sort menu. |
| G3 | The book info sheet as built. |
| G2 | Settings sections: General, Reading, Library, Extensions, Shortcuts, About. |
| G12 | The Aa code-and-tables hint and the fixed-layout line. |
| S14 transition | The cover-grow transition and a warm book after the library move to Phase 8. |

## Approved 2026-09-26 (Phases 7–8 review)

The owner reviewed items 9, 10 and 21–28 and approved them as recommended, with item 28 settled as below. The PROVISIONAL notes for these in the code refer to this record.

| # | Approved |
|---|---|
| D4 (21) | Extension packages: a `*.linenext` zip with `manifest.json` at its root; no signing in the MVP (“Not verified by Linen”); the API reference in `docs/extensions/`; English only. |
| G1 (22) | Install, update and remove sheets as built. |
| G7 (23) | Extension surfaces as built: Navigator tab after Notes (“More” beyond one), the selection “⋯” menu, pinned commands in the ⋯ menu, built-ins listed and not removable, the suspended box, frames on the extension's own origin. |
| Baselines (24) | Phase 7 baselines: Screen 11 with extensions, Screen 12's extension failure (see `docs/visual/APPROVAL.md`). |
| P6 memory (25) | Accepted for the MVP: a Worker's memory cannot be measured in WebKit; CPU is budgeted through the heartbeat. |
| D1 (26) | No telemetry. A crash log on this Mac only (`~/Library/Logs/app.linen.reader/crash.log`), revealed from Settings › About. |
| D6 (27) | Tauri's updater, checked once a day, updates signed with the Linen update key and served from GitHub Releases; downloaded and installed in the background with a quiet “Update ready · Restart” line; a minor release about every six weeks. |
| Signing (28) | No Apple Developer ID yet. Builds and signing need only meet what GitHub requires: the release workflow on GitHub builds ad-hoc-signed DMGs and signed updater archives. Developer ID signing and notarisation switch on when their secrets are added. |
| X3 re-check (9), oldest Mac (10) | Approved by the owner without a recorded run. |
| File associations | Tried by the owner on the installed app: Open With and a book dropped on the Dock icon both work (2026-09-26). |
| Update source (29) | The owner made `Quentinbest/read-book` public (2026-09-26), so installed apps can reach its releases. The update address stays `https://github.com/Quentinbest/read-book/releases/latest/download/latest.json`; no separate releases repository is needed. |
| Screen 08 (30) | The Notes tab with the Phase 7 export footer is the new baseline (see `docs/visual/APPROVAL.md`). |

## Decided 2026-09-29 (immersive reading, like macOS Books)

The owner asked for immersive reading to match the macOS Books app. These override the plan where they differ; the code cites this record.

| Rule | Decided |
|---|---|
| S9 (top edge) | The top edge reveals the top bar alone, naming the book only (no chapter). It stays while the pointer is on the bar or in its 64 px zone and hides as soon as the pointer leaves; the 3 s delay (S10) applies only to the full controls (Tab, ⌘J, Aa), which still show both bars and the chapter. |
| S9, S11 (bottom edge) | No bottom trigger: the pointer at the bottom edge reveals nothing, so the Dock exclusion of S11 no longer applies. The bottom bar comes only with the full controls. |
| L9, B2, G12 (location line) | Immersive reading shows no location line. The chapter and “N min left” are in the bottom bar's labels, and so are a fixed-layout book's real pages. The “Opening …” line of a slow open (G8) stays. |

## Decided 2026-10-01 (edge reveal brings the progress bar back)

Testing 0.1.1, the owner found the progress bar missing when the toolbar is revealed, and chose to restore the bottom edge. This replaces the S9 and S9/S11 rows of 2026-09-29; the location-line row stands.

| Rule | Decided |
|---|---|
| S9 (edge reveal) | Either edge, after the 150 ms dwell, reveals both bars, as before 2026-09-29. The top bar names the book only. The bars stay while the pointer is in an edge zone or on a bar and hide as soon as it leaves. The full controls (Tab, ⌘J, Aa) are unchanged: chapter in the title, 3 s delay. |
| S11 (bottom zone) | With the Dock hiding at the bottom (or in full screen), the bottom zone stays but leaves out the 6 pt strip where the Dock slides in, instead of switching off; otherwise the bottom edge could not reveal anything on a Mac with an auto-hiding Dock. |

## Approved 2026-10-01 (release 1.1, and item 31)

The owner approved items 33–40 as recommended (“Go ahead”, then “Approve and fix”), then items 31 and 41 the same day. The details are in `docs/pending-approvals.md` and `docs/release-1.1-status.md`; the code cites this record.

| # | Approved |
|---|---|
| 1.1 Font, Page width (33) | Aa and Settings › Reading, all books: Book, Literata, Sans, OpenDyslexic (bundled, OFL); Narrow 56 / Normal 66 / Wide 74 ch. |
| C5 Publisher styles (34) | Full, Balanced (default), Off in Settings › Reading; Simplify Styles for This Book (Off for one book), with Undo. |
| Dictionary peek (35) | Look Up in the selection bar, the context menu and ⌃⌘D, from this Mac's dictionaries; Open in Dictionary; Search the book. Extension peek providers wait for a Host API addition. |
| Library list view (36) | Covers / List switch in the library header, remembered. |
| Reading sessions (37) | Permission `reading.sessions`, activation `onReadingSessions`, `linen.reading.on('sessionEnded')`; the title only with `book.metadata`; 5-minute idle end, 10-second minimum. |
| C4 remapping (38) | Settings › Shortcuts as built: modifiers required (function keys alone allowed), the system's chords reserved, extensions never take core shortcuts, a core command can take another's. This lifts the MVP's “remapping is deferred” rule (§2.8). |
| B6 search options (39) | Whole words and regular expressions under the search field. This replaces B6's 2026-09-24 “no space reserved”. |
| Baselines (40) | The 1.1 captures become baselines: `01-library`, `03-more-menu`, `05-navigator-search`, `06-selection-bar`, `09-reading-settings`, `12-damaged-book`, `12-extension-failure`, `14-night-selection`, `g10-cheat-sheet` (see `docs/visual/APPROVAL.md`). |
| Quitting with unsaved notes (31) | As built (review fixes, 2026-09-26): Quit and Restart wait for the reader's writes; after a failed save Linen stays open with “Some notes aren’t saved, so Linen stayed open · Quit Anyway”. Approved with its wording. |
| Settings captures (41) | `11-settings-reading` and `11-settings-shortcuts` become baselines (see `docs/visual/APPROVAL.md`). |

## Decided 2026-10-01/02 (the Linen logo)

The owner accepted logo `linen-a-v002` (direction A, “The unhurried page”: two asymmetric rust pages with an open seam, Literata lettering in Ink) with “Go ahead” on 2026-10-01; it shipped in 0.2.0. Its working files (concepts, checks, the release package) are kept outside git in `logo-project/`, which is ignored. On 2026-10-02, under “Make your best calls”, the app icon was refitted to the macOS grid.

| Item | Decided |
|---|---|
| Logo files in the app | `public/brand/linen-a-v002/` (About, light and dark), `public/favicon.svg` and `.ico`. Artwork unchanged from the accepted release. |
| App icon | `src-tauri/icons/linen-a-v002-grid/`, generated from its `icon.svg`: the accepted flat icon with its tile moved to Apple's 824 px tile in a 1024 px canvas (it was 864, plus its hairline) and the Big Sur template shadow (black 30 %, 10 px down, 10 px blur). The page symbol keeps its paths and its size relative to the tile. `icons/linen-a-v002/` stays as accepted. |
| Not done | A layered `.icon` for macOS 26 (needs Icon Composer on macOS 26.4+); trademark or registration work. |
| Approved 2026-10-02 (item 42) | The owner approved item 42 (“Approve”): the grid refit stands, and the artwork stays as accepted, including the leaf-like right page and the left page's differing corners. |

## Decided 2026-10-06 (minimum window size, item 53)

The owner chose option A: “Go with option A, 760 × 480”. The measurements behind it are in `docs/reader-review-2026-10-06.md`.

| Rule | Decided |
|---|---|
| L8 (minimum window) | The main window, which holds the library and the reader, cannot be resized below 760 × 480 (`minWidth`/`minHeight` in `src-tauri/tauri.conf.json`). 760 is L8's narrow breakpoint and the width Screen 15 is drawn at; 480 is L9's height. The narrow forms below 760 px stay for page zoom (X6). Check `L8-minimum-size`. |

## Approved 2026-10-06 (reader review, item 54)

After item 53, the owner answered “Go ahead”, which approves the last waiting item as recommended (`docs/pending-approvals.md`). That reply did not say whether the steps were tried by hand.

| # | Approved |
|---|---|
| Reader fixes (54) | As built and verified in the app (`docs/reader-review-2026-10-06.md`). Contents and search jumps in Scroll mode reach the target in one step, with a brief blank while a chapter loads, as in Pages. The wheel over the Navigator scrolls only the Navigator. In Scroll mode both edges reveal both bars, as in Pages. |

## Decided 2026-10-06 (the review's side findings; the owner: “Make your best calls”)

These came up during the reader review (`docs/reader-review-2026-10-06.md`, Side findings). The owner left the calls to Claude. Check `S3-title-and-message`.

| Rule | Decided |
|---|---|
| Screen 03 (top-bar title) | The title stays centred on the window, inset on both sides by whichever side's buttons take more room, plus 12 px. A long title ends in “…”, the chapter part first. |
| X6 (library narrow form) | The narrow header starts 18 pt past the window buttons (90 CSS px from the window's edge), as the wide header and the reader's bar do. |
| M1 (messages and the bottom bar) | While the reader's bottom bar shows, a message sits 12 px above it, and returns to the foot when the bar goes. A pointer resting on a message does not reveal the bars, so the message never moves out from under it. |
| Pages-mode jumps | Unchanged: at most one blank frame while a chapter loads. Removing it would mean loading jumps into a hidden view and swapping (D-D1), for a frame or two. |

## Approved 2026-10-07 (scroll bars, item 55)

The owner answered “Approval item 55 and proceed”, which approves the recommendation, option A (`docs/pending-approvals.md`). The measurements are in `docs/scrollbar-survey-2026-10-07.md`. The reply did not say whether a resting pointer on the knob was tried by hand.

| # | Approved |
|---|---|
| Scroll bars (55) | Linen keeps WebKit's own scroll bars and follows macOS's **Show scroll bars** setting: overlay bars (hidden at rest, shown while scrolling, Scroll mode included) when macOS uses them; always-visible bars with **Always**, or with a mouse attached under **Automatically**. No custom indicator. Measurement `scrollbar-survey` (opt-in). |

## Decided 2026-10-07 (UI localisation: Simplified Chinese, Traditional Chinese, Japanese, Spanish)

The owner approved all eleven decisions of the localisation plan as recommended. They override the plan where they differ (B10, D4); the code will cite this record.

| # | Decided |
|---|---|
| L-1 (B10) | B10 is amended: the UI is translated now, into `zh-Hans`, `zh-Hant`, `ja` and `es`, with English as the source and fallback. Right-to-left UI mirroring stays out of scope; book content keeps its own language and direction (P§17, I15). |
| L-2 | Traditional Chinese is one `zh-Hant` catalogue in Taiwan usage (e.g. 檔案, 資料夾). `zh-TW`, `zh-HK` and `zh-MO` all resolve to it; a separate `zh-Hant-HK` only if readers ask. |
| L-3 | Spanish is one `es` catalogue in neutral wording that avoids regionalisms, addressing the reader as *tú*. Every `es-*` tag resolves to it; split into `es-ES` and `es-419` only if reviewers object. |
| L-4 | The language follows macOS (including the per-app language in System Settings) and can be overridden in Settings › General › Language, whose default is “System”. Stored as the `language` setting (`system` or a tag); an unknown value counts as `system`. |
| L-5 | A language change applies when Linen restarts; Settings offers Restart. No live switching for now. |
| L-6 | Translations are drafted (machine or professional) and reviewed in context by a native speaker of each language, against a shared glossary and per-language style guide. Each sign-off is recorded here. |
| L-7 | Library sorting by title and author follows the UI language (`Intl.Collator` with the UI locale). |
| L-8 | Interface text set in Literata (a Latin-only face) uses the system Chinese or Japanese font in those languages. Book text is unchanged. The resulting screens are provisional until their design is approved. |
| L-9 | Rollout: the first release with the translations follows the system language by default; English is always one setting away. Rollback is a patch release that drops a language; its stored setting then falls back to the system language, then English. |
| L-10 (D4) | Scope: the extension permission labels (Linen's own) are translated; text supplied by extensions stays as authored (`locales` still reserved, D4); the Finder file-kind name “EPUB book” is deferred. Documentation, the crash log and the app name stay English. |
| L-11 | Style: Japanese uses the polite です/ます form; Chinese and Japanese use full-width punctuation and their own quotation marks (“…” in Simplified Chinese, 「…」 in Traditional Chinese and Japanese). |

## Approved 2026-10-07 (extension diagnostics, item 56)

The owner answered “Approve item 56, then start Stage 3”, which approves the recommendation (`docs/pending-approvals.md`).

| # | Approved |
|---|---|
| Extension diagnostics (56) | The core's extension checks (manifest fields, permissions, Host API versions), the theme-pack contrast report and errors that reach extension code stay in English, as L-10 keeps extension-supplied text as authored. The sentence around them (““Name” can’t be installed”, “Turned off: …”) is translated. |

## Approved 2026-10-08 (Chinese and Japanese typography, item 57)

The owner answered “Approve item 57, then push i18n and open a PR”, which approves the recommendation (`docs/pending-approvals.md`). The screens it covers are no longer provisional.

| # | Approved |
|---|---|
| CJK typography (57) | In Chinese and Japanese, interface text that English sets in Literata uses the system font (L-8); lines break between phrases (after full-width punctuation and at spaces, `src/lib/strings/breaks.ts`, `src/app/base.css`); labels lose their letter-spacing. Book text keeps Literata and the book's own language. The native reviewers (L-6) may still change wording that breaks badly. |

## Signed off 2026-10-08 (the four translations, L-6)

The owner reported that the native reviewers finished and that all four catalogues are approved as drafted, with no changes: the glossary's proposed terms and macOS names included. The languages ship (Stage 6).

| Language | Signed off |
|---|---|
| Simplified Chinese (`zh-Hans`) | As drafted (`src/lib/strings/zh-Hans.ts`) |
| Traditional Chinese, Taiwan usage (`zh-Hant`) | As drafted (`src/lib/strings/zh-Hant.ts`) |
| Japanese (`ja`) | As drafted (`src/lib/strings/ja.ts`), 読字障害 and ゴシック for the font choices included |
| Spanish (`es`) | As drafted (`src/lib/strings/es.ts`), following macOS's `es` wording where regions differ |

## Decided 2026-10-09 (Reading Lens, Stage 0)

The owner settled Stage 0 of the Reading Lens assessment: in-place explanations and dictionary look-ups for readers of English technical books. The assessment and draft spec are kept outside this repository. Nothing below is built yet.

| Question | Decided |
|---|---|
| Host | Linen hosts the Reading Lens work. |
| AI | An exception to “Not planned: AI” (plan §1.3, P§20) for an **installable extension only** (Explain). Linen's core and built-in extensions stay without AI, and nothing AI is bundled. |
| Providers | Explain may offer presets for DeepSeek, Qwen (international, Singapore), a local Ollama on the reader's Mac, and OpenAI, Anthropic and Gemini. Every host still needs the reader's own grant, which names the host. |
| Distribution | Personal use until the pilot passes (gate G3). Explain stays out of this public repository until then. The Host API additions it needs, and the dictionary support below, may ship. |
| MDX/MDD | Linen reads MDX/MDD dictionaries in the core and shows their entries in the 1.1 lookup peek. Display only: entries never reach extensions or a model. Linen ships no dictionary data; readers add their own. |
| Peek placement | The lookup peek opens below the selection, with a pointer at it, as the footnote peek does (Screen 17). |
| Peek layout | After the macOS Books Translate popover, in Linen's tokens: the source word, a hairline, the answer, and a footer of actions. |
| Provider switching | A menu above the answer names the provider and language and switches between Explain languages and installed dictionaries. It replaces the proposed tabs. |
| Speak buttons | Not included for now. |
| Surfaces | Solid only: no see-through, blurred or vibrant panels, menus or bars. Compared against see-through versions on the design canvas and dropped. |

## Approved 2026-10-09 (Reading Lens, items 58–76)

The owner approved items 58–76 as recommended (`docs/pending-approvals.md`) and chose the MIT licence for item 60. The Stage 2a rules built to these recommendations (LK12–LK15, O9–O13, O15) are no longer provisional.

| # | Approved |
|---|---|
| Order (58) | Stage 2a first; Stage 1 starts once the gate thresholds are set. |
| Gate thresholds (59, O1) | Set before Stage 1 starts and before any data is seen. The values themselves are still to be given. |
| Licence (60, O2) | **MIT**, in `LICENSE`. Stage 2b may take a permissively licensed parser or port BSD-3 code (DX12). |
| Look Up and ⌃⌘D (61, O5) | The 1.1 Look Up peek becomes the lookup peek; this Mac's dictionaries are its first provider; ⌃⌘D stays. |
| No entry (62, O8, DX14) | “No entry for ‘…’ in your dictionaries”, plus “Explain in context” when Explain is installed (Stage 2b). |
| Highlights (63, O9, LK14) | A clicked highlight offers Look Up and lookups; Delete keeps its place. |
| No room (64, O10, LK13) | The side with more room, that room minus 16 px; the answer scrolls. |
| Closing (65, O11, LK12) | Scrolling, a resize, a text-size or Aa change, the Navigator, any jump and leaving the book close the peek and cancel; a theme change keeps it. |
| No language (66, O12, LK9) | `book.lang` is `""` for a missing language or `und`; Explain's condition accepts `""` and `en`. |
| Fixed layout (67, O13) | Extension lookups are off in fixed-layout books; this Mac's Look Up stays. Revisit with fixed-layout zoom (E2, I17). |
| One provider (68, O15, LK15) | The label is plain text, without a menu. |
| Links in entries (69, O14, DX15) | Inert text (Stage 2b). |
| MDX encodings (70, O16, DX3) | UTF-8, UTF-16LE, GBK, GB18030 and Big5 through a reviewed crate, `encoding_rs` the candidate (Stage 2b). |
| Local-model wording (71, O17) | As the plan drafts it, in Explain's own repository (Stage 3). |
| Oxford data (72, O6) | The owner's call; nothing in this repository depends on it. |
| Selection bar ring (73, O7) | A bar-specific ring colour, as a separate fix. |
| Peek strings (74) | The English is approved; the zh-Hans, zh-Hant, ja and es drafts go to the native reviewers (L-6) before Stage 2a ships. |
| Peek baselines (75) | Captured in the next session and compared with Canvas 2–9. |
| First-request notice (76, EX8) | A `notice` status is added to LK2 (title, text, host; the core draws Continue and Not now) before Stage 3. |

## Approved 2026-10-10 (Reading Lens, items 77–81)

The owner approved items 77–81 as recommended (`docs/pending-approvals.md`). The screens below are baselines now, and the Stage 2b designs built to O3 and O4 are no longer provisional.

| # | Approved |
|---|---|
| Peek baselines (77) | The 14 lookup peek captures (`rl-02` to `rl-11`, Paper, Night and Sepia), and the changed `11-settings-extensions` and `12-extension-failure`, are baselines (`docs/visual/APPROVAL.md`). |
| Settings › Dictionaries (78, O3) | As built after Canvas 14: add, order (Move up and down), On, Remove; amends G2's sections. |
| The light card (79, O4, DX7) | Dictionary entries sit on a light card in every theme. |
| Nightly fuzzing (80) | Nightly Rust and cargo-fuzz installed; the cargo-fuzz target runs for 30 minutes before Stage 2b ships. |
| Dictionary screens and strings (81) | `rl-11-dictionary-entry-paper`, `rl-11-dictionary-entry-night` and `rl-14-settings-dictionaries` are baselines; the English is approved; the four translations go to the native reviewers with item 74's. |

## Approved 2026-10-18 (the sync protocol, #101)

The owner reviewed the protocol, merged it (“gh pr merge 101”) and answered its two items (2026-10-18). Stage 1's code (1a, 1b, 1c) can now be built from it.

| What | Approved |
|---|---|
| The protocol (`docs/sync-protocol.md`, SP1–SP37) | Merged as written after Codex's review (58 rounds, #101), with its amendment to SY7 (a device's ID is derived from its key). Values marked (prov.) stay provisional. |
| A device that missed a removal (113, #86, SP31, SP32) | **(a).** Besides SP31, a device that has heard nothing for 7 days (prov.) from any enrolled device other than the writer and those it enrolled holds that writer's deletions, book-file replacements and changes to highlights and notes, and asks once. Option (c), 30 days of undo data for every remote deletion, was not chosen: SY26 keeps its 7 days (item 106). |
| The new device confirms the code too (114, SP24) | Approved. The new device shows the code with “Does ‹device› show this code? · It matches · Cancel” and joins only after It matches; its screen is built in Stage 3. |

## Approved 2026-10-17 (sync Stage 1, #92)

The owner asked for Stage 1 (“Work on issue #92”) and, when asked, chose to start it now and approved all seven of its open items as recommended (2026-10-17). Each item's text is in `docs/pending-approvals.md`.

| What | Approved |
|---|---|
| When sync is built | Stage 1 is built now. The wait for a second platform (2026-10-14) applies to shipping sync, that is Stage 6 and sync in release builds, not to building the core. |
| A change and a deletion made apart (102, SY13, SY20) | A conflict the reader decides, only when the other device changed the thing after the last sync it shared with the deleting device; otherwise the deletion applies. Nothing deleted comes back without the reader's choice. |
| Merge rules (105, SY12, SY14, SY16) | Newest wins for the place (even earlier in the book), each setting, last opened and finished; adding a book is never undone by a merge; two updated files imported apart: the newest import wins. |
| Safety values (106, SY10, SY17, SY24–SY28) | As listed in item 106. They stay marked (prov.) in the plan and PROVISIONAL in the code: approving them settles the rules, and the numbers can be tuned. |
| Encryption (108) | None in this plan; decided again before any provider other than the two folders. |
| One storage at a time (109, SY29) | One storage per library; switching keeps the old copy and joins the new storage as a first sync. |
| Trusting and removing devices (110, SY34) | Signed changes; enrollment by a short code confirmed on an enrolled device, or by the recovery code; Remove needs the recovery code and replaces it. |
| A deletion's week on a device back late (111, SY17) | **(a).** Other devices' reports never shorten this device's Restore; a device back late offers it for its own week, labelled with the reported age. |
| Not decided by this | The protocol document (`docs/sync-protocol.md`) and its items 113 and 114 waited for the owner's review (approved 2026-10-18, above). Items 98–101, 103, 104 and 107 (Stages 2–5) stay open. |

## Approved 2026-10-17 (the sync device ID, item 112)

| What | Approved |
|---|---|
| Where the sync device ID is kept (112, SY7) | **(a).** The login keychain, since Linen's ad-hoc builds can't use the data-protection keychain's *ThisDeviceOnly* class (`docs/spikes/sync-folders.md`). The ID records the Mac's hardware UUID, and a mismatch takes a new ID; storage holding newer changes under the ID than the device's own mark (a restored Mac) takes a new ID too. Move to the data-protection item once a release is shown to carry the keychain entitlement, which needs a provisioning profile as well as a Developer ID (#53). SY7 in `docs/sync-plan.md` is amended to match. |

## Decided 2026-10-17 (sync Stage 0's scope, #91)

The owner has no second device, iCloud account or Google account for now, and chose as recommended (2026-10-17). This changes Stage 0 in `docs/sync-plan.md`, which asked for two Google accounts, a second device and every supported macOS.

| What | Decided |
|---|---|
| Stage 0 (#91) | Done when the items one Mac can do are measured, with the owner at the Mac. In that session (2026-10-17) the owner chose to skip iCloud Drive off or signed out and full storage; the refusal and the prompt's wording weren't captured. Those, and an offline iCloud Drive read, also move to Stage 6. |
| Second-device items and other macOS releases | Move to Stage 6 (#97): arrival time and rename atomicity on another device, conflict-copy names, Migration Assistant or a restore to another Mac, and macOS 13, 15 and 26. |
| Several Google accounts | A unit test of root discovery over fake folders, in Stage 2 (#93). No second account is needed. |

## Approved 2026-10-17 (the sync plan, #84)

The owner approved the plan (2026-10-17: “Proceed with approved PR #84”), and it was merged.

| What | Approved |
|---|---|
| The plan | `docs/sync-plan.md` as merged: rules SY1–SY34 and stages 0–6. Its stages are #91 (Stage 0) to #97 (Stage 6), sub-issues of #73, unlabelled until the owner labels one to build. |
| Not decided by this | Items 98–111 in `docs/pending-approvals.md` stay open: approving the plan answers none of them, and nothing that depends on one is built. Values marked (prov.) stay provisional. Sync still waits for a second platform (2026-10-14). |

## Decided 2026-10-14 (sync across devices, #73)

The owner wrote these into issue #73 (“Already decided”); they are copied here as the record. The plan is `docs/sync-plan.md`; its open questions are items 98–111.

| What | Decided |
|---|---|
| What syncs | Everything: places, highlights and notes, settings, the library list and the books. Extensions and MDX dictionaries do not. |
| Storage | Linen runs no storage server. It uses existing storage services, iCloud and Google Drive first. |
| How it comes | As extensions through the design's sync slot (P§18, P§20); the iCloud and Google Drive providers come with Linen, listed as “Built-in”. |
| Folders | Both providers write into a folder on the Mac: the reader's iCloud Drive folder, and the Google Drive for desktop folder. Neither needs an iCloud container, an Apple Developer account or Google's API. |
| Conflicts | When two devices changed the same highlight or note, the reader decides which to keep. |
| Deletion | A deletion removes the local copy first, then the copy in storage. What was deleted is kept in the reader's storage for one week and can be restored during it. A device away for more than a week does not bring it back. |
| When | Sync waits for a second platform (#54, #80). |

## Decided 2026-10-14 (requirements as issues, worked by an agent)

The owner asked how requirements should reach agents, then said “start working based on the suggestions” (2026-10-14). This builds on R7 (one backlog).

| What | Who | Decided |
|---|---|---|
| Requirements as issues | Answered by the owner | The owner writes what is needed as a GitHub issue. A Claude Code session on this Mac works it (“work on issue #n”), so the in-app suite and real input stay available. |
| Which issues an agent works | Answered by the owner | Only those the owner marks with a label only collaborators can apply, or names in a session. Text from anyone else is never an instruction (`AGENTS.md`, “Issues”). |
| The label | Delegated to Claude | `ready-for-agent`. No issue template applies it, because a template's labels are added whoever files the issue. |
| The issue form | Delegated to Claude | `.github/ISSUE_TEMPLATE/feature.yml`: what is needed and for whom, Done when, out of scope, already decided, open questions, size. |
| Features get a plan first | Delegated to Claude | A plan PR (`docs/<feature>-plan.md`), as for the localisation, Reading Lens and selection-bar work. After it merges, one sub-issue per stage, unlabelled; the owner labels the stages to build. |

## Approved 2026-10-14 (Phase 3 of the process review)

The owner asked for Phase 3 and chose as recommended when asked (2026-10-14).

| What | Approved |
|---|---|
| One backlog (R7) | Open work lives in GitHub Issues: deferred review findings, follow-ups, checks still to do. `docs/pending-approvals.md` keeps only questions for the owner. This amends “Deferred” in the 2026-10-14 review-findings rule: an issue, or a pending item when the owner must decide. |
| The open PRs #7–#9 (R7) | Claude checks each against `main` and reports what still applies; the owner then decides each one: rebase and finish, or close with its branch kept. Nothing is closed without that answer. |
| Dependency updates and audits (R8) | Weekly Dependabot PRs for Cargo, npm and GitHub Actions, grouped; `cargo audit` and `pnpm audit --prod` in CI. foliate-js stays pinned. |
| Branch protection (R9) | Merging into `main` requires CI's three jobs to pass, for everyone, the owner included; no review approval is required. |

## Approved 2026-10-14 (Gate 0's waiver, item 97; Phase 2 of the process review)

The owner chose item 97's option (a) and, for Phase 2 of the development-process review, chose as recommended when asked (2026-10-14).

| What | Approved |
|---|---|
| What would undo Gate 0's waiver (97) | **(a).** Stage 2d (optional hosts, Keychain keys, options pages) stays whatever the diary shows: it is general extension plumbing, and removing it would break installed extensions. If the Stage 1 diary fails G0, Stages 3–5 stop: Explain stays private and is not offered to others, and the result is recorded beside the waiver. |
| Testing a release candidate (R4) | Before the owner publishes a draft release: Claude runs the full in-app suite on the release commit and writes 3–8 acceptance scenarios from its `CHANGELOG.md` entry; a script backs up the owner's library folder and installs the draft's DMG; the owner runs the scenarios by hand with real input; Claude records each result with its setup. Release builds keep ignoring `LINEN_DATA_DIR`, so the hand test uses the real library, after the backup. |
| Agent instructions in the repository (R5) | A one-page `AGENTS.md` is committed (Codex reads it); a committed `CLAUDE.md` imports it; the longer development notes move to `docs/development.md`. Nothing is kept private. |

## Decided 2026-10-14 (how changes are reviewed and decided)

The owner asked for Phase 1 of the development-process review (2026-10-14: “Start Phase 1 fixes”). Two defects had shipped that were visible before release: an automated review finding on #31 described the bug that 0.4.2 fixed, and a recommendation nobody answered was built in 0.4.1 (“Decided 2026-10-12”).

| What | Decided |
|---|---|
| Review findings | Before a merge is asked for, the automated review of the latest push has finished, and every finding has an answer in its thread: **Fixed** (the commit), **Rejected** (a one-line reason) or **Deferred** (a numbered item in `docs/pending-approvals.md`). The PR lists them (`.github/pull_request_template.md`). |
| Decisions | Only the owner's explicit answers are recorded as approved. A choice left to Claude is recorded as **Delegated to Claude**; a question nobody answered stays open and is not built. A PR built from a plan with open questions says which were answered, delegated or left open. A waived gate records what result would undo the waiver (item 97). |

## Decided 2026-10-13 (release 0.5.1)

The owner asked for release 0.5.1, a fix release after 0.5.0: highlights are drawn again on a page you return to from the Notes tab, or turn into, after reading elsewhere (#37, A8). No UI text was added, so no translation is pending.

## Decided 2026-10-13 (release 0.5.0)

The owner asked for the release after the compact selection bar (#35; “Go ahead”): 0.5.0, a minor release because it adds Host API 1.2 (a lookup's `icon`). The new strings (“Delete highlight”, “‹title› (‹name›)”) ship as machine drafts in the four languages, as 0.4.0's did, and the release notes say so. Explain 0.2.0 (“Reading Lens”, `linen-explain` #2) needs this release and ships after it is published.

## Approved 2026-10-13 (compact selection bar, items 88–96)

The owner approved items 88–96 as recommended, one by one (`docs/pending-approvals.md`; plan `docs/selection-bar-plan.md`, rules SB1–SB6). They amend A1 (the bar's actions are icons with tooltips) and LK1 (one lookup may have a button on the bar instead of an item in “⋯”).

| # | Approved |
|---|---|
| Icons in place of the bar's words (88, SB3) | Note, Copy, Search (Delete for a clicked highlight) and Look Up show their icon only, named for VoiceOver “Note”, “Copy”, “Search the book”, “Delete highlight”, “Look Up”; a divider before Look Up. Attach here and Cancel, and the items in “⋯”, keep their words. The new string “Delete highlight” goes to the native reviewers with item 74's. |
| The bar's tooltips (89, SB2) | Drawn by the bar, not by `title`: after a 500 ms rest, then at once on the next button, at once on keyboard focus; away from the selection; the tooltip colours. Copy names no ⌘C until ⌘C on a reader selection is checked to paste plain text. |
| Hit areas (90) | An exception to X4 for the selection bar: 32 px faces with no gaps between them, as since Phase 5. The bar is used with a pointer (touch is deferred). |
| Explain's button (91, SB4, SB5) | The first lookup that names an icon, applies and isn't stuck has a button after Look Up, “‹title› in context”; a stuck one goes back to “⋯”; “⋯” shows only with something in it. The peek's “in context” opens the same lookup. |
| Host API 1.2 (92) | A lookup may name `icon` (`lightbulb` only, drawn by Linen). Lookups stay experimental. Linen with 1.2 is released before Explain 0.2.0, which requires it. |
| The two “Explain” entries (93, SB1) | They were the harness's Test Lookup and Explain, both titled “Explain”. Lookups that share a title show their extension's name, “Explain (Test Lookup)”; the string goes to the native reviewers. The owner removes Test Lookup from their library. |
| Explain's icon (94) | The outline lightbulb, tooltip “Explain in context”. |
| The extension's name (95) | **Reading Lens**, in Explain 0.2.0: the display name only. The ID (`io.github.quentinbest.explain`), the action's title “Explain” and the approved notices (item 84) stay. |
| Captures (96) | `06-selection-bar`, `14-night-selection`, `12-extension-failure`, `sb-bar-lookup-paper`, `sb-bar-lookup-night`, `sb-bar-tooltip-paper` become baselines (`docs/visual/APPROVAL.md`). |

## Decided 2026-10-12 (release 0.4.2)

The owner asked for release 0.4.2, a fix release after 0.4.1: a click in the book text closes every reader panel, the docked Contents, Search and Notes included, and one click is enough with Contents open (#33). No UI text was added, so no translation is pending.

## Decided 2026-10-12 (release 0.4.1)

The owner asked for release 0.4.1, a fix release after 0.4.0: Explain answers again (#30), the wheel over a reader panel no longer turns the page or closes it (#30), panels close on an outside click, right-click offers the selection bar's actions, and a slow provider is no longer counted as an extension crash (#31). The owner confirmed Explain with DeepSeek in a local build of `main` (2026-10-12: HTTP 200 in about 2 s). No UI text was added, so no translation is pending.

## Decided 2026-10-11 (release 0.4.0)

The owner asked for release 0.4.0 and chose, when asked:

| Decision | What |
|---|---|
| Unreviewed translations ship as drafts | The zh-Hans, zh-Hant, ja and es text added since 0.3.0 (the lookup peek, Settings › Dictionaries, the host and key sheets, options pages; items 74, 81, 82, 83) ships as machine drafts, and the release notes say so. This amends item 74 (“before Stage 2a ships”): the native review (L-6) still happens, after the release. |
| Stage 2d ships | Optional hosts, Keychain keys and options pages ship in 0.4.0 as experimental Host API 1.1. Gate 0's waiver (2026-10-10) covers shipping them, not only building them. |

## Approved 2026-10-11 (Reading Lens Stage 3, item 87)

| # | Approved |
|---|---|
| How the offline check is scored (87) | As built in `linen-explain` (`offline/`, `docs/offline-run.md`): each reviewer marks every answer blind, `verdict` correct, partly or wrong, and `severe` when it is wrong and shows no doubt (no qualifier, not “needs more context”); `term_ok` for the Chinese term. Krippendorff's α on `severe` must be at least .67 before any rate is used. An answer is severe when either reviewer marks it. The rate and its one-sided 95% Clopper–Pearson bound are reported per model; G2 passes for a model when the bound is at most 5% over at least 60 items. Items are every local stuck point from the coded diary, with their context widened from the book to the sentence and one on each side. |

## Approved 2026-10-11 (Reading Lens Stage 3, items 84 and 85)

| # | Approved |
|---|---|
| Explain's live check and notices (84) | The DeepSeek live check passed (one fixed sentence; `deepseek-flash`, 1.7 s, a valid answer, recorded in `linen-explain`). Each preset's first-request notice is approved as written in `linen-explain/docs/providers.md` (checked 2026-10-11). |
| Where the offline items come from (85) | The Stage 1 diary runs now, with the owner as one of the readers (`docs/reading-lens-stage1.md`). Its local stuck points are the offline check's items; G2 waits for them and for two bilingual reviewers. |

## Approved 2026-10-11 (Reading Lens Stage 3, items 83 and 86)

| # | Approved |
|---|---|
| “Not set up” and “‹name› options…” (83) | A lookup may answer `not_configured`, shown with Open options; the provider menu ends with “‹name› options…”. Experimental Host API 1.1. The English is approved; the four translations go to the native reviewers with item 82's. |
| The peek's languages (86) | Extension lookups are offered in Linen's UI language, then macOS's preferred languages in their order, then English, at most four (`readerLanguages`). Chinese keeps its script (zh-Hans, zh-Hant). |

## Decided 2026-10-12 (reader panels and the lookup budget, after 0.4.0)

The owner reported that Explain didn't work with DeepSeek, that the selection bar and Aa vanished when the wheel was used over them in Pages mode, and asked that reader panels close on an outside click. The causes were fixed in #30. Then the owner decided:

| Question | Decided |
|---|---|
| Selection interaction (Q1) | Keep the selection bar on mouse-up (A1, unchanged). The right-click menu offers everything the bar does, including Look Up with each extension lookup (Explain) and extension actions; while re-attaching it offers Attach here and Cancel. A setting that turns the bar off is not built. |
| Outside clicks (Q4) | Every floating layer but the modals (S8) and the image view closes when the pointer goes down outside it or its toggle button: Aa, Go to, ⋯, the selection bar, the note card (it saves first), the footnote and lookup peeks, and the floating Navigator. The docked Navigator closes the same way (owner, 2026-10-12, after 0.4.1: a real click in the text left Contents, Search and Notes open on a wide window; keeping it open had been recommended but never approved). When Aa, Go to, ⋯, the note card or the floating Navigator closes this way, the closing click does nothing else on the text or a margin (no page turn, link or highlight); on the chrome it goes through. The selection bar and the peeks follow the text, so their closing click still acts (A5, N9: found by the in-app suite). A drag still selects (S7). A click inside a panel but outside its open menu closes the menu only. |
| The lookup budget (Q5) | `net.fetch` gives up after 8 s, inside the 10 s work budget (LK10), so a slow provider ends as the extension's own error. A watchdog timeout while one of the extension's requests is in flight still ends the call, but doesn't count toward suspension (P6). |

## Approved 2026-10-10 (Reading Lens Stage 2d, item 82)

The owner approved item 82 as recommended (`docs/pending-approvals.md`).

| # | Approved |
|---|---|
| Stage 2d screens and strings (82) | `rl-13-host-sheet`, `rl-12-settings-sends-to`, `rl-12-options-dialog` and `rl-09-error-keychain-paper` are baselines (`docs/visual/APPROVAL.md`). The key is typed into a native dialog, not an HTML sheet; the host sheet quotes the extension's purpose; “Sends to” reads “what you choose to send”; the options dialog is the extension's frame and Done. The English `extAccess` strings are approved; the four translations go to the native reviewers with items 74 and 81. |

## Decided 2026-10-10 (Reading Lens gate thresholds, item 59, O1)

The owner set the thresholds as recommended, before any Stage 1 data exists. A gate that fails stops the stages after it; the work before it stands.

| Gate | Passes when |
|---|---|
| G0 (after the Stage 1 diary) | Local stuck points (L2 technical sense of a known word, L3 term mapping, L4 multiword or idiom, S1 compressed noun phrase, S2 clause, negation or reference) are **at least 40%** of all stuck points, **and** the reader's current tool failed on **at least 30%** of those local stuck points. Floors: at least **30** local stuck points in all, from at least **3** readers. “Failed” means the reader's note says the tool did not settle it, or they gave up on it. Coded with Report 1's types, noticed or not, before the counts are looked at. |
| G2 (Stage 3 offline check, at least 60 items) | The severe-misleading rate's **upper 95% bound (Clopper–Pearson) is at most 5%**, reported with the observed rate: with 60 items that means no severe error; with 120, one. Reviewer agreement α ≥ .67 before any score is used. |
| G3 (Stage 4 pilot, 6–8 readers, three arms) | Explain is **ahead for at least 5 of 6 (or 6 of 8) readers**, where ahead means more items understood correctly than **each** other arm, or equal correctness and at least 20% less time; any other tie counts against Explain. **No more wrong answers adopted:** Explain's pooled count of wrong answers the readers took up is no higher than the best other arm's. |

## Decided 2026-10-10 (Reading Lens Gate 0, waived by the owner)

The owner approved Gate 0 and asked to proceed (“Approve Gate 0 and proceed”) **without the Stage 1 diary**. It is recorded as a waiver, not a pass: no stuck-point data exists, so the thresholds set the same day were not applied. Stages 2d onward go ahead. The diary (`docs/reading-lens-stage1.md`) can still run, and its counts would then be recorded beside this; Gates 2 and 3 are unchanged and still need their data.

