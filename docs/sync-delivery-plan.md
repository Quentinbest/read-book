# Sync, the road to a release — revised plan

Status: drafted 2026-11-12 from the progress review of 2026-11-11. The owner approved items 121–125 the same day, each as recommended (`docs/decisions.md`), so SD2–SD10 stand as written. A stage is still built only when the owner labels or names it, and each run in a real storage folder still waits for the owner's agreement (SD3).

This plan changes the **order** in which sync is built and the **gates** on the way. It adds no protocol rule and removes no check:

- `docs/sync-plan.md` (SY) and `docs/sync-protocol.md` (SP) stay as they are, apart from the lines section 6 lists.
- `docs/sync-pass-plan.md` (PS) stays, with P3 split in two, P4's two PRs named P4a and P4b, and the order changed (section 3).
- The IDs below (SD1…) name how the remaining work is ordered and gated. Code and tests cite them beside the others (`// SY2, SD4: …`).

## 1. What the review found (read 2026-11-11)

- **Nothing has reached a reader since v0.5.1 (2026-10-13).** Four batches of security fixes are merged and unreleased (#153).
- **Sync is in Stage 1 of 6.** `src-tauri/src/sync/` is 45,210 lines in 42 files with 637 tests; the rest of the Rust core is 12,128 lines. Nothing in the app calls it (PS8).
- **Stage 1's checks:** 3 of 31 Covered on 2026-10-27 (C1, C10, C24; `docs/sync-status.md`). Since then, as `docs/sync-pass-plan.md` §1 records: #179 covered C3 and C27, #179 and #188 covered C2 except its book case, and #184 covered C11's decision half.
- **Stages keep dividing.** Stage 1 was sized as three PRs and is past 55. P2 was one PR and is six.
- **Sync cannot ship as planned.** Stage 6 needs "a Mac and the second platform" (Decision 10-14), and both platform issues (#54, #80) are deferred.
- **The sync code has never met a real folder.** Every test runs over `folder::Memory` (PS2). Real iCloud Drive and Google Drive behaviour is first met in Stage 2, after all of Stage 1.
- **The owner's queue is long:** #153 (open since 2026-10-25), the Audit workflow (red on `main` since 2026-11-03, now for two advisories: #201's and RUSTSEC-2026-0459), #154's list, #171, and the hands-on checks #50, #51, #56 and #57.

## 2. Rules

| ID | Rule |
|---|---|
| SD1 | **The queue first.** Part A (section 3) is done before new sync work is labelled, so `main` is green on every workflow and a release is out. |
| SD2 | **A slice before the rest of Stage 1 (item 122).** After P2b and the applying half of P3, a thin slice runs end to end in the test build: a place and a highlight between two libraries, through a real folder on disk. P4 and the rest of P3 follow it, so what real storage does shapes the remaining tests. |
| SD3 | **The slice never touches the reader's own library or storage folder.** It runs on test libraries (`LINEN_DATA_DIR`) and in a `Linen-spike` folder that is deleted afterwards, as Stage 0 did. Nothing is written to either storage before the owner agrees to the run. |
| SD4 | **The disk folder is built once.** `sync::folder::Disk` implements PS1's `Folder` over a directory with SY2's confinement, and is the bottom layer Stage 2's brokered folder uses unchanged. In the slice the core calls it directly, in the test build only; the provider Worker and Host API 1.3 (SY1, SY3) come in Stage 2. A release build has no caller (PS8 stays). |
| SD5 | **No check is dropped; P4 is split (item 123).** P4a holds the soak test and the checks of devices that behave. P4b holds the checks of trust under attack. All 31 are Covered before Stage 6 and before any run on a reader's own library. Section 4 lists them. |
| SD6 | **A review point after the slice.** The slice ends with a report and stops. The owner then chooses: go on as planned, change the plan from what the report found, or park sync. No later stage is labelled before that. |
| SD7 | **What "two devices" means for the release (item 121).** As recommended: two Macs. Stage 6's week of real reading and its moved Stage 0 measurements run on two Macs signed in to the same accounts; another platform brings its own checks with #54 or #80. Stage 6's repeats on macOS 13, 15 and 26 stay as `docs/sync-plan.md` lists them, and two Macs may not give all three. |
| SD8 | **Review rounds for code no release build calls (item 124).** As recommended: two automated rounds a PR. A P0 or P1 finding is always fixed. After the second round, other findings are answered **Deferred** with an issue. The limit ends with the first PR that gives sync a caller in a release build, and Stage 6 starts with one full review of `sync/`. |
| SD9 | **A stage says when it grows.** A stage that needs more PRs than its plan names gets a line in the plan saying why, in a PR of its own or the next stage PR, before the extra PR opens. |
| SD10 | **Readers keep getting releases (item 125).** As recommended: a release at least every two weeks while `main` holds a reader-facing change or a security fix, and one small reader-facing issue labelled beside each sync stage. |

## 3. The work, in order

Each line is one PR unless it says otherwise, and ends with `scripts/ci-local.sh` green. The owner labels what is built (AGENTS.md › Issues); this is the order recommended.

### Part A — The queue (SD1)

The owner's, except where it says agent.

| # | What | Notes |
|---|---|---|
| A1 | Merge #205 (P2b-1) | CI's three jobs pass; Codex found nothing on `9a8048e`; only Audit is red (A2 and A3). |
| A2 | Decide #201 (RUSTSEC-2026-0429, `zip` 4.6.1 through `tauri-plugin-updater`) | Checked 2026-11-12: `tauri-plugin-updater` 2.14.1 (2026-10-28) depends on `zip` ^9, and `Cargo.toml` already allows it, so option (a) is a lockfile update. In 2.13.3 the ZIP code is Windows-only (`cfg(windows)`, a Windows-only dependency), so macOS never compiles it. Recommended: (a), with the release checks, since 2.14.0 changed `Update::install`; `.cargo/audit.toml`'s three older `zip` ignores then go too. |
| A3 | #171 (rusqlite 0.41, build fails since 2026-10-28), #209 (build and audit fail), #210 (audit only) | Read 2026-11-12, nothing built. **#171:** one compile error, `src/sync/backup.rs:99` (`backup`'s progress argument needs a type in 0.41). It is also the fix for RUSTSEC-2026-0459 (`libsqlite3-sys` 0.38.2), the second advisory that keeps Audit red; its branch is from 2026-10-28 and needs a rebase. **#209:** `zip` 9.4.2 fails `epub::tests::a_cp437_name_is_found_by_its_decoded_form`; why, and whether `DecodedNames` is still needed, is not yet looked into. **#210:** nothing of its own; it fails only on the two Rust advisories. |
| A4 | Decide #153, and if yes cut v0.5.2 | AGENTS.md › Releases; the candidate is tried as `docs/release-acceptance.md` says. |
| A5 | Tick off and close #154 | Most of its list is done; what isn't has its own issue. |
| A6 | The hands-on checks #50, #51, #56, #57 | Open since 2026-10-14. Do them, or close each with the reason. |
| A7 | Housekeeping (agent, if the owner agrees) | `docs/PROGRESS.md` and `docs/sync-status.md` brought up to date; local `main` fast-forwarded; the 31 worktrees of merged branches removed after each is checked for uncommitted work. |

**Done when:** every workflow on `main` is green (Audit needs both A2 and #171), v0.5.2 is published or declined, and #154 is closed.

### Part B — To the slice (SD2–SD5)

| Stage | What it builds | Done when |
|---|---|---|
| B1 = P2b-2 | Steps (9)–(10): publishing and the device file. | As `docs/sync-pass-plan.md` P2 says. |
| B2 = P2b-3 | Step (5)'s later fetches and repair, views in step (4), step (3)'s unknown identities and tails. | Two devices on one `Memory` exchange a note and a deletion through `pass::run`; a device behind resumes from a view; each budget stops the pass; a restart keeps a deletion's age and a witness timeout. |
| B3 = P3a | Step (6): the small-things transaction, with SY26's guard and SY27's backups under the lock. | A pass that asks the reader stops and resumes after the answer; a backup is taken before a remote deletion applies; a kill between any two built steps loses nothing committed (C30's second half). |
| B4 | `folder::Disk` (SD4): PS1 over a directory. Temporary name and rename; storage-made names only as tokens; SY2's confinement. | Stage 2's confinement checks that need no Worker pass over a temporary directory: `..`, absolute paths, symbolic links in and out, hidden and over-long names, objects over the limits. PS3's SP7 tests pass over `Disk` as they do over `Memory`. |
| B5 | The slice, test build only. Test commands set up a library on a folder, enrol a second by code (no screens), and run one pass. | In-app checks `SY-place-resume` and `SY-highlight-roundtrip` (Stage 3's, by the same names) pass with two library folders on one temporary folder. With no setup, no file under either root is opened (`SY-nothing-without-setup`). |
| B6 | The real-folder run and its report (no product code). With the owner's agreement (SD3): the two test libraries on one Mac through `Linen-spike` in iCloud Drive, then Google Drive; on two Macs if a second is at hand. | `docs/spikes/sync-slice.md` records, for each storage: whether both checks pass, how long a change takes to arrive, and everything the folder did that `Memory` has no switch for. Each such thing is a new PS2 switch with a test, or an issue. |

B2 grew (SD9, 2026-11-14): B1 came to about 2,090 changed lines and B2 has more in it, so it is built as three PRs. B2a: views in step (4), which answers #208. B2b: step (5), the later fetches of bodies and repair. B2c: step (3)'s unknown identities and removed devices' tails, with P2's whole-pass Done-when tests (two devices exchanging a note and a deletion, a restart keeping a deletion's age and a witness timeout). B2 is done when all three are.

Books (steps (7)–(8)) and compaction and cleanup (step (11)) are not in the slice: the slice syncs no book file, and it runs for days, not weeks.

#197 (P2) was closed on 2026-11-08, before P2b was built; B1 and B2 need it reopened or issues of their own (section 6).

**Then stop (SD6).** The summary names what the report found and recommends one of: go on, change, park.

### Part C — After the review point

In this order if the owner goes on. Stages 2 to 5 are as `docs/sync-plan.md` §6 says, with their own issues (#93–#96).

| Stage | What | Notes |
|---|---|---|
| C1 = P3b | Steps (7)–(8) and (11): books within their budget; compaction and cleanup, at most daily. | The rest of P3's Done-when. |
| C2 = P4a | The soak test (C29, item 120 (a)) and the checks of devices that behave (section 4), as whole-pass tests. | Includes every switch B6 added to `Memory`. |
| C3 = Stage 2 (#93) | Host API 1.3, the two built-ins, the Worker over B4's `Disk`. | B5's direct call is removed here. |
| C4 = Stage 3 (#94) | Settings › Sync and the setup screens; B5's test commands are replaced by them. | |
| C5 = P4b | The checks of trust under attack (section 4). Closes #151 and #92. | May run beside C3 and C4; it touches only `sync/` tests. |
| C6 = Stages 4 and 5 (#95, #96) | Books; deletion, Restore and conflicts. | |
| C7 = Stage 6 (#97) | Real devices and release, as item 121 is answered. Starts with SD8's full review. | Not before every check in section 4 is Covered. |

## 4. The checks (SD5)

The numbers are `docs/sync-status.md`'s. Each check's text stays as `docs/sync-plan.md` §6 states it.

| Checks | Where they're closed |
|---|---|
| C1, C3, C10, C27 | Covered: C1 and C10 in `docs/sync-status.md`, C3 and C27 by #179 since (C27 again end to end in B2) |
| C2 | Covered except its book case: #187, Stage 4 |
| C11 | P1 over `Memory`; real conflict-copy names in B6 and Stage 6 (#174) |
| C30 | B3 (a kill during a pass) and the migrations test |
| C29 | C2 = P4a, the soak test |
| C4, C5, C6, C7, C8, C13, C14, C19, C20, C24, C28, C31 | C2 = P4a: devices that behave (clocks, backups, compaction, blobs, restore, repair, joining) |
| C9, C12, C15, C16, C17, C18, C21, C22, C23, C25 | C5 = P4b: trust under attack (untrusted writers, rollbacks, removals, recovery, forged sequences and ancestry, copied devices) |
| C26 | Stage 4, as `docs/sync-pass-plan.md` §4 has it (books apply only from Stage 4); its in-app check is Stage 5's |

C24 is already Covered by unit tests (`docs/sync-status.md`); it stays in P4a because `docs/sync-pass-plan.md` §4 asks for it again as a whole-pass test. Where one check has both kinds of part (C19's forged ages, C15's guards), the part goes with its kind and the check is Covered when both are.

## 5. Risks

1. **The slice finds that real folders break an assumption of the protocol.** That is what it is for, and SD6 is where the owner decides what follows. Finding it after P4 would cost more.
2. **B5's scaffolding outlives the slice.** Its test commands and direct `Disk` call are marked `// SD4: removed in Stage 2/3` and listed in C3's and C4's Done-when.
3. **Deferred findings pile up under SD8.** Each is an issue, and Stage 6's full review reads them all before sync has a release caller.
4. **No second Mac.** Then B6 stops at one Mac with two libraries, and item 121 (a) cannot finish Stage 6 until one is found. The slice is still worth its cost: it runs the pass over real storage.

## 6. What changes elsewhere

Done with the approval (2026-11-12):

- `docs/sync-plan.md`: “When it is built”, §6's opening on order, and Stage 6's first bullet.
- `docs/sync-pass-plan.md` §3: a note that P3 and P4 are built as P3a, P3b, P4a and P4b in this plan's order.
- `AGENTS.md` › Done means, 3: SD8's limit.
- `docs/decisions.md`: the answers to items 121–125.

After this plan merges:

- Issues: #198 (P3) and #199 (P4) each split in two; new unlabelled issues for B4, B5 and B6; #197 reopened, or new issues for B1 and B2.

## 7. Open questions

None. Items 121–125 in `docs/pending-approvals.md` were approved on 2026-11-12, each as recommended:

| Item | Question | Approved |
|---|---|---|
| 121 | Does a second Mac count as Stage 6's second device? (SD7) | Yes: two Macs |
| 122 | A slice over a real folder before the rest of Stage 1, with `Disk` called directly in the test build? (SD2, SD4) | Yes |
| 123 | P4 split in two, with the attack checks allowed to finish beside Stages 2 and 3? (SD5) | Yes; all 31 before Stage 6 |
| 124 | Two review rounds for code no release build calls? (SD8) | Yes, P0 and P1 always fixed |
| 125 | A release at least every two weeks, and a reader-facing issue beside each sync stage? (SD10) | Yes |
