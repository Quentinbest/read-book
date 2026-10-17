# Progress

The state of each phase of `docs/implementation-plan.md` (§5), macOS only (scope decision, 2026-09-24).

This file is updated at every phase boundary. Before the next phase starts, the previous phase is checked against its Done-when list; the evidence is in its status document. Owner decisions waiting are in `docs/pending-approvals.md`; decisions taken are in `docs/decisions.md`.

| Phase | State | Done-when | Status document |
|---|---|---|---|
| 0 Spikes | Done | Spikes A–F reported; decisions recorded | `docs/spikes/` |
| 1 Foundations | Done | Met | (git history, `docs/decisions.md`) |
| 2 Reading | Done | Met | `docs/phase2-status.md` |
| 3 Navigation | Done | Met; baselines approved 2026-09-25 | `docs/phase3-status.md` |
| 4 Search | Done | Met; baseline approved 2026-09-25 | `docs/phase4-status.md` |
| 5 Selection and annotation | Done | Met; baselines approved 2026-09-25 | `docs/phase5-status.md` |
| 6 Settings and library | Done | Met; baselines approved 2026-09-25 | `docs/phase6-status.md` |
| 7 Extensions | Done | Verified 2026-09-25 (98/98 on Desktop 2); baselines and the P6 memory budget approved 2026-09-26 | `docs/phase7-status.md` |
| 8 Hardening and release | Done | 2026-09-26: 107/107 on Desktop 2; unit tests and budgets pass; CI passes on GitHub; every approval item settled (9, 10, 21–30) | `docs/phase8-status.md` |
| Release 1.1 (owner, 2026-10-01; without TTS, sync and Developer ID signing) | Done; approved 2026-10-01 (items 31, 33–41); released as 0.2.0 on 2026-10-02 | Each feature's in-app check passes; local and GitHub CI green | `docs/release-1.1-status.md` |
| Reader review (owner, 2026-10-06: jumps, the wheel over Contents, minimum size, Scroll-mode reveal) | Done; approved 2026-10-06 (items 53, 54); minimum window 760 × 480; released as 0.2.1 on 2026-10-06 | The owner's items reproduced with real input, then pass; full suite and local CI green | `docs/reader-review-2026-10-06.md` |
| Scroll bars (owner, 2026-10-07: the overlay scroll bar in a recording) | Done; approved 2026-10-07 (item 55, option A: follow macOS); no product change | Measured in the app with real input, with this Mac's setting and with overlay bars (`scrollbar-survey`); local CI green | `docs/scrollbar-survey-2026-10-07.md` |

## After the plan

| Work | State | Plan and status |
|---|---|---|
| UI localisation (zh-Hans, zh-Hant, ja, es; L-1 to L-11, 2026-10-07) | Done: four languages signed off and registered 2026-10-08; released as 0.3.0 on 2026-10-09 | `docs/i18n-plan.md` (stage table at its end) |
| Reading Lens (lookup peek, local dictionaries, optional hosts and keys; 2026-10-09) | Stages 2a, 2b and 2d released as 0.4.0 on 2026-10-11; Stage 1 (the diary) running; Stage 3's Explain is private (`linen-explain`) and its offline check waits for the diary | `docs/reading-lens-plan.md`, `docs/reading-lens-status.md` |
| Compact selection bar (owner, 2026-10-12: icons with tooltips, Explain as an icon, the two “Explain” entries, the extension's name) | Done 2026-10-13 (#35): local CI green, in-app suite 172/175 plus the three canary checks rerun once their port was free; items 88–96 approved 2026-10-13, captures are baselines; released as 0.5.0 on 2026-10-13 and tested by the owner; Explain 0.2.0 (“Reading Lens”) merged (`linen-explain` #2) | `docs/selection-bar-plan.md`, `docs/selection-bar-status.md` |
| Sync across devices (#73; owner, 2026-10-14: everything but extensions and dictionaries, through iCloud Drive and Google Drive folders) | Plan approved and merged 2026-10-17 (#84); stages #91–#97; Stage 0 partly measured 2026-10-17 (`docs/spikes/sync-folders.md`); still open in #91: a second device, other macOS releases, iCloud Drive off or signed out, offline, full storage, two Google accounts, Drive's stream or mirror mode, the prompt's wording and what a refused app receives, and another app path; items 98–112 open; building waits for a second platform (#54, #80) | `docs/sync-plan.md` |

## How the work is checked

- **Unit tests.** `pnpm test` (Vitest) and `cargo test` (Rust); CI runs both on every push.
- **The in-app end-to-end suite.** `scripts/e2e.sh`, on the current desktop; `LINEN_SPACE=2` moves it to Desktop 2 (required 2026-09-25, optional since 2026-09-29). It has 124 checks in total (plus opt-in measurements such as `size-survey` and `scrollbar-survey`, run only by name; arguments after the check regex go to the app); the last fails on any uncaught error in the run. The safe-mode checks also run with `LINEN_SAFE_MODE=1`.
- **Visual captures.** `scripts/e2e.sh v`, compared with the approved baselines by `node tests/visual/compare.mjs`.
- **Budgets.** `scripts/perf-coldstart.py` and `scripts/perf-memory.sh`, on the reference machine.
