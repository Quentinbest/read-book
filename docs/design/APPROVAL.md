# Design approval record

| Item | File | Status | Date | Approver |
|---|---|---|---|---|
| 17 screens (committed in `3f5da5e` as `Quiet EPUB Reader.html`, renamed unchanged in `e0c38fe`) | `Quiet EPUB Reader (screens).html` | Approved | 2026-09-24 | Project owner |
| Design canvas written proposal, §1–25 (behaviour, shortcuts, extension API, permissions, failure table) | `Quiet EPUB Reader (proposal).html` | Approved with the screens | 2026-09-24 | Project owner |
| System spec, S1–S5 (information architecture, state model, canvas anatomy, visual system, extension architecture) | `Quiet EPUB Reader (system).html` | Approved with the screens | 2026-09-24 | Project owner |

Notes:

- The proposal and system spec have been in this repository since `e0c38fe`. The implementation plan cites them directly (`docs/implementation-plan.md` §1–2), so they no longer need to be copied into `docs/spec/behaviour.md`.
- The files are self-extracting bundles that render only with JavaScript. A plain-text export for review and diffing is planned for Phase 0 (implementation plan §5).

## Later decisions that supersede the approved design

| Superseded statement | Decision | Date | Ref |
|---|---|---|---|
| Failure table (proposal §22): title and author can be edited in Book info | Metadata is read-only | 2026-09-24 | Q4 |
| Proposal §2: the resume chip lasts four seconds | It is a queued message with the standard 10 s timer, pausing on hover or focus | 2026-09-24 | C1 |
| Screen 16: ⌘T labelled “Go to chapter…” as a separate command | One command: “Go to chapter…” in ⌘K opens the Contents tab focused on the current chapter; ⌘T runs it | 2026-09-24 | C2 |
| S5 and Screen 11: session events and the `background` permission in the MVP | Annotation events only in the MVP; session events and `background` come in 1.1. Reading Time on Screen 11 is illustrative | 2026-09-24 | C3 |
| Proposal §18: extension commands with user-assigned shortcuts in the MVP | No shortcuts for extension commands until remapping arrives | 2026-09-24 | C4 |
| Failure table (proposal §22): per-book “Simplify styles” as a recovery | Deferred to 1.1 with the Publisher styles setting | 2026-09-24 | C5 |
| Proposal §10 and S1: “Show in Finder / Explorer” and “Show file” used interchangeably | Menus use the platform label (Show in Finder / Show in Explorer / Show in Files); the damaged-book card keeps “Show file” | 2026-09-24 | C6 |

Refs point to `docs/implementation-plan.md` §10.1 (Q) and §10.2 (C).
