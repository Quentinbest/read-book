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

## How the work is checked

- **Unit tests.** `pnpm test` (Vitest) and `cargo test` (Rust); CI runs both on every push.
- **The in-app end-to-end suite.** `scripts/e2e.sh`, on Desktop 2 (owner's instruction, 2026-09-25). It has 107 checks in total; the last fails on any uncaught error in the run. The safe-mode checks also run with `LINEN_SAFE_MODE=1`.
- **Visual captures.** `scripts/e2e.sh v`, compared with the approved baselines by `node tests/visual/compare.mjs`.
- **Budgets.** `scripts/perf-coldstart.py` and `scripts/perf-memory.sh`, on the reference machine.
