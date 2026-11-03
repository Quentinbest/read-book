# Sync, one pass (SP37) — execution plan

Issue #151 is the last part of Stage 1 (#92). Every Stage 1c module is pure library code today ("nothing is wired in"), and most of Stage 1's remaining checks need a whole pass. This plan says how the pass is built, over an in-memory folder, and which checks each stage closes. It adds no protocol rules: SP37 and the rules it cites stay as `docs/sync-protocol.md` states them. The IDs below (PS1…) name how the pass is put together, and code and tests cite them beside the protocol's (`// SP37 (6), PS4: …`).

## 1. What is there today (read 2026-11-03)

- **The modules** in `src-tauri/src/sync/` cover SP1–SP37 piecemeal:
  - membership, enrollment and recovery (`membership`, `enrollment`, `recovery`, `trust`);
  - device files and the device store (`device_file`, `device_store`);
  - change files and views (`chain`, `view`, `compact`, `read`, `prune`);
  - applying (`apply`, `guard`, `late`, `restore`, `backup`, `local`, `journal`);
  - protection (`hold`, `ages`, `weeks`, `cleanup`, `shared`, `freshness`).

  Each takes its inputs from the caller and returns a decision or bytes. None reads or writes storage.
- **Caller-owned state:** `hold::Ledger`, `weeks::KnownWeeks`, `freshness::Heard` and `ages::Marks` live in memory. Nothing persists them between passes or restarts.
- **No storage interface exists**, apart from `shared::SharedFolder` (#184), which covers books and blobs only. SY2's brokered folder (`list` with a cursor and opaque tokens for names the storage made, `read`, `write` through a temporary name and a rename, `remove`, `stat`) is Stage 2 (#93).
- **Checks:** `docs/sync-status.md` (2026-10-27) lists Stage 1's 31 checks: 3 covered, 19 partly, 9 not yet. Since then:
  - #179 covered C3 and C27;
  - #179 and #188 covered C2 except its book-level case (#187, Stage 4);
  - #184 covered C11's decision half.

  Section 4 maps every check to a stage.

## 2. Rules

| ID | Rule |
|---|---|
| PS1 | **The folder interface.** `sync::folder::Folder` mirrors SY2, so Stage 2's brokered folder implements it unchanged:<br>• `list(dir, cursor) -> page of (name, token)`, at most 1,000 a page;<br>• `read(token)`, `stat(token)`, `remove(token)`;<br>• `write(path, bytes)` through a temporary name and a rename;<br>• `replace` and `hash` for `shared::SharedFolder`.<br>Names the storage made come back only as tokens; `write` never takes one. Paths are relative and at most four segments deep. |
| PS2 | **The in-memory folder.** `sync::folder::Memory` implements PS1 for tests, with switches for what real storage does:<br>• half-synced files (wrong bytes for a while);<br>• conflict copies (`‹name› 2`);<br>• delayed, repeated and reordered visibility per reader;<br>• files that vanish;<br>• a rename that fails.<br>Several devices share one `Memory` through per-device views. It is test-only code. |
| PS3 | **SP7 in one place.** Reading and writing files (temporary names, a half-synced file skipped and read again on a later pass, a storage-made name read only as a candidate for the file it copies) live in one module that every step uses, not in each step. |
| PS4 | **One pass is one function.** `sync::pass::run(device, folder, clock, budgets) -> Report` runs SP37's steps (1)–(11) in order, calling the existing modules. It holds the store's lock only in steps (6) and (8), as SP37 says. It returns what it did and what it left: what waits for a budget, a body, the reader or a later pass. It never decides a rule itself; each decision stays in the module that has it. |
| PS5 | **What a pass keeps between runs.** The caller-owned state (`Ledger`, `KnownWeeks`, `Heard`, `Marks`) is saved in the library's database at the end of each step that changes it, in the same transaction as that step's other writes, and loaded at the start of a pass. A crash between steps leaves the state of the last completed step (item 119, approved (a)). |
| PS6 | **Budgets.** SY2's per-pass budgets (100,000 names listed, 32 devices, 256 MB read apart from books, 60 s) and #122's pass-level read budget are counted in `pass`, and a budget that runs out stops the pass where it is. Membership crowding (#122's priority) is read first and bounded first. Provisional values stay as `docs/decisions.md` records them. |
| PS7 | **Time and randomness are injected.** The pass takes the vouched clock and, in tests, a seeded random source, so the soak test (C29) is repeatable from its seed. Nothing in `pass` reads the wall clock or the OS random source directly. |
| PS8 | **Nothing new is wired into the app.** `pass::run` has no caller outside tests until Stage 2 provides the brokered folder and Stage 3 the setup. |

## 3. Stages

Each stage is one PR (two where noted), and ends with `scripts/ci-local.sh` green and its Done-when met before the next begins.

### P1 — The folder and SP7 (PS1–PS3)

- `folder.rs` (the trait, and `Memory` with its switches), and SP7's read and write helpers.
- `shared::SharedFolder` is implemented over PS1, so #184's rule runs over the in-memory folder.
- **Done when:**
  - every PS2 switch has a test;
  - SP7's rules each have a test that fails without them;
  - #184's tidy runs over `Memory`.

### P2 — The pass, steps (1)–(5) and (9)–(10): reading and publishing (PS4, PS5, PS6)

- Steps (1)–(5):
  - (1) the device store and marks;
  - (2) membership;
  - (3) device files;
  - (4) peers' views and changes;
  - (5) blobs and repair.
- Steps (9)–(10): publishing (reservations and holds, uploads, the naming change) and the device file.
- Persisted state (PS5, one migration).
- **Done when:**
  - two devices on one `Memory` exchange a note and a deletion through `pass::run`;
  - a device behind resumes from a view (C27 again, end to end);
  - each budget stops the pass with a test.

### P3 — The pass, steps (6)–(8) and (11): applying, books, compaction and cleanup

- Step (6): the small-things transaction with SY26's guard and SY27's backups under the lock.
- Steps (7)–(8): books within their budget.
- Step (11): compaction and cleanup, at most daily.
- **Done when:**
  - a pass that asks the reader (SY26) stops and resumes after the answer;
  - a backup is taken before a remote deletion applies;
  - compaction and cleanup run on the second day, not the first;
  - a kill between any two steps loses nothing already committed (C30's second half).

### P4 — The checks that need a pass (two PRs)

- The soak test (C29), with three devices on one `Memory`:
  - seeded random operations, offline spells, skewed clocks;
  - late, repeated and reordered delivery, restarts, compaction, and a device away for 8 days.

  It runs in CI with a fixed seed in at most 60 s (item 120, approved (a)), and an ignored long run takes a random seed it prints.
- The remaining Partly and Not yet checks that Stage 1 can reach (section 4), each as a test of whole passes.
- `docs/sync-status.md` updated to the result.
- **Done when:** every check section 4 assigns to Stage 1 is Covered, and the last PR closes #151 and #92.

## 4. The checks

Each check is either closed in this plan's stages, or left to a later stage because it needs that stage's features. The table names which; the per-check detail lives in `docs/sync-status.md` and is updated by P4.

| Check | Where it's closed |
|---|---|
| C1, C3, C10, C27 | Covered (C27 again end to end in P2) |
| C2 | Covered except its book case: #187, Stage 4 |
| C4, C7, C8, C14, C17, C19, C20, C22, C24 | P4, as whole-pass tests |
| C5, C28 | P3 (backups under the lock) and P4 |
| C6, C13 | P2 (publishing, blobs) and P4 |
| C9, C18 | P4 for the protocol part; the setup screens are Stage 3 |
| C11 | P1 over `Memory`; real conflict-copy names in Stage 6 (#97, #174) |
| C12, C15, C16, C21, C23, C25 | P4, with membership and enrollment in whole passes |
| C26 | Stage 4 (books apply only from Stage 4) |
| C29 | P4 (the soak test) |
| C30 | P3 (a kill during a pass) and the existing migrations test |
| C31 | P4 for joining; the book import side is Stage 4 |

## 5. Open questions

- **Pending item 119, where the pass keeps its state between runs (PS5).** My recommendation is the library's database, saved with each step's writes. The device store (the keychain item) is the alternative, but it's meant for small identity and marks.
- **Pending item 120, the soak test's budget in CI (C29).** My recommendation is at most 60 s with a fixed seed in `ci-local.sh` and CI, plus an ignored long run (`--ignored`, a random seed, 10 minutes) for occasional use.

Both were answered by the owner on 2026-11-03, as recommended: (a) for each.
