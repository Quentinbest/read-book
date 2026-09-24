---
name: execute-plan
description: Execute an inline implementation plan, ordered plan files, or a scoped task request through authorized changes, recovery, verification, and an evidence-based handoff. Use only when explicitly invoked as `/execute-plan`. Do not activate automatically for ordinary implementation or planning-only, review-only, explanation-only, or monitoring-only work.
disable-model-invocation: true
argument-hint: "<task request, implementation plan, or ordered plan-file paths>"
---

# Execute Plan

Invoke as `/execute-plan <task request, implementation plan, or ordered plan-file paths>`. Invocation authorizes only requested, in-scope work.

## Resolve the input

Input: $ARGUMENTS

1. Classify the input as a scoped task request, an inline plan, or one or more plan-file paths. Do not interpret a path-like argument as prose when that choice would change the work.
2. When plan files are named, validate that every file exists and is readable, then read every file in full before changing implementation files. Preserve the user-provided order as dependency order. Check cross-plan dependencies and checkpoint before proceeding when files are missing, unreadable, ambiguous, or contradictory.
3. For multiple plan files, finish implementation, verification, and gap closure for each plan before starting the next unless the plans explicitly require another safe order.

## Establish the outcome and boundary

1. Locate the workspace and repository root, if present. Capture version-control or equivalent baseline state and treat unrelated state as user-owned.
2. Confirm applicable `CLAUDE.md`, `AGENTS.override.md`, and `AGENTS.md` instructions, including nested instructions governing in-scope paths.
3. Inspect relevant documentation, configuration, files, dependencies, tests, commands, and external systems before changing them.
4. Define deliverables, acceptance criteria, constraints, non-goals, scope, and required verification evidence.
5. Honor the request's mutation boundary. Invocation alone does not authorize destructive actions or external effects such as deployment, publication, messaging, purchases, access changes, or production-data mutation.

## Normalize and track the plan

1. For a supplied plan, preserve intent while checking order, dependencies, scope, risk, acceptance criteria, and verification. Record deviations; checkpoint before any material change to the outcome, scope, risk, or external effects.
2. For a task request, derive a concrete, ordered, scope-bounded plan covering inspection, implementation, and verification before making changes.
3. Convert each plan into traceable work items with acceptance criteria and stable identifiers. Include the plan name in identifiers when handling multiple files. If a supplied plan yields no reliable work items, show what was parsed and checkpoint instead of inventing or silently skipping work.
4. For non-trivial work, state the goal, material assumptions, acceptance criteria, and short plan. Maintain a live checklist (use the task/todo tool when available), respect tracker constraints, and complete items only with evidence.
5. Continue until every safe, authorized, in-scope item is complete or an evidenced blocker requires user action.

## Decide whether to proceed

- **Proceed** under reasonable assumptions when work is safe, reversible, authorized, and in scope.
- **Checkpoint** for material ambiguity, difficult recovery, destructive or irreversible action, new authority or scope, missing credentials or secrets, or an unapproved external effect. State the action, impact, recovery options, evidence, and smallest user decision needed; complete independent safe work first.
- **Stop** prohibited or inherently unsafe actions. Higher-priority safeguards remain binding. Request scope expansion before out-of-scope work.

## Execute the plan

1. Work in dependency order and perform authorized edits and operations; do not stop at planning or advice.
2. Make the smallest coherent changes that meet acceptance criteria. Preserve established patterns, use existing tooling, and avoid unrelated refactors or cleanup.
3. Inspect intermediate results and the final change set. Remove accidental artifacts before verification.

## Write status to plan files only when requested

1. Treat source plans as read-only unless the user explicitly requests status writeback.
2. When writeback is requested, add or update compact status blocks without deleting or rewriting plan prose. Obtain the actual current date from the environment; never reuse a copied example date.
3. Mark an item complete only after its verification passes. Record blocked items with the blocker and preserve failed evidence.
4. After a later gap fix, rerun the affected verification and refresh its status block so the plan reflects final evidence. If required writeback is unavailable, checkpoint and report the limitation while completing independent safe work when possible.

## Recover from failures safely

1. Capture the failed command or operation, output, scope, and relevant environment state.
2. Diagnose the cause and distinguish implementation failures from environment, dependency, permission, and pre-existing failures. Retry only after a relevant change or new evidence; never repeat an unchanged failing action.
3. Apply the smallest safe correction, rerun the focused failing check, and then rerun dependent verification.
4. Do not conceal failures, weaken assertions, bypass safeguards, discard user work, or disable meaningful checks to obtain a pass.
5. After bounded corrective attempts, or when recovery needs new authority, checkpoint with evidence, attempts, the blocker, and required user action.

## Verify against the acceptance criteria

1. Run proportionate checks: focused tests first, then relevant suites, builds, linting, type checks, static analysis, or targeted reviews.
2. If normal checks are unavailable, perform the strongest safe targeted review and explain the limitation.
3. Record each exact shell command or named non-shell operation, available status, and outcome. Include an exit code when provided.
4. Classify verification as passed, failed, blocked, or not run. Separate implementation failures from environment limitations and pre-existing failures.
5. Reinspect scope, completeness, artifacts, and instruction compliance. Claim success only when behavior and evidence satisfy acceptance criteria.

## Audit and close plan gaps

1. After implementation, map every work item and acceptance criterion to implementation evidence and verification evidence. Do not treat filenames, text matches, or code presence alone as proof of behavior.
2. List missing, contradictory, unverified, or out-of-scope requirements as gaps before fixing them.
3. Run at most three gap-closure passes per plan. In each pass, apply only safe in-scope fixes, rerun affected checks, refresh requested status writebacks, and rebuild the evidence map. Stop early when a full pass finds no gaps; record anything remaining after the third pass as outstanding.
4. When the project provides a specific auditor, run it only if the tool exists, its use is authorized, and its checks and fixes remain in scope. Review findings before changing code, verify accepted fixes, and report unavailable or rejected auditing without substituting an unrelated dependency.

## Hand off proportionately

End with completed behavior, changed files or systems, exact verification commands or operations and outcomes, material assumptions or plan deviations, remaining limitations or blockers, and required user action. For multiple plan files, preserve processing order and include per-plan tallies for total, completed, and blocked items plus gaps closed and outstanding. Omit empty ceremony for trivial tasks, but never hide failed, blocked, skipped, or unverified work. Leave every live-plan item completed or explicitly blocked.
