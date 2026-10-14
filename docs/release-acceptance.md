# Release acceptance

Every release is tried as the release it will be, with real input, before it is published (owner, 2026-10-14; `docs/decisions.md`). The release checks in `docs/phase8-status.md` prove the package: signatures, versions, `latest.json`. This proves the behaviour of what changed. Until now the owner was the first tester of each release, after it was published; four of six fix releases (0.2.1, 0.4.1, 0.4.2, 0.5.1) fixed what that testing found.

## Steps

| # | Who | Step |
| --- | --- | --- |
| 1 | Claude | Writes the release's record, `docs/releases/vX.Y.Z.md`, from the template below, with 3–8 scenarios taken from the version's `CHANGELOG.md` entry. Each names the rule it checks, what to do with the mouse, trackpad and keyboard, and what should happen. It is committed with the version bump. |
| 2 | Claude | Runs the full in-app suite on a test build of the release commit (`scripts/e2e.sh`; the README's “Test in the real app”) and records the count, `D1-no-uncaught-errors` included. A subset is not enough here: checks depend on earlier ones. |
| 3 | — | The tag is pushed and the release workflow makes the draft. |
| 4 | Owner | Quits Linen and runs `scripts/release-candidate.sh vX.Y.Z`. It downloads the draft's DMG and updater archive for this Mac and checks the archive against the update key, as the updater does (`src-tauri/examples/verify_update.rs`). The DMG's app must be that archive's app, byte for byte. It then backs up the library folder and the installed app, installs the candidate and opens it. It refuses while any Linen has the library open. Release builds always use the real library, so the backup comes first. |
| 5 | Owner | Runs each scenario by hand, with the real mouse, trackpad and keyboard, and says what happened in each. |
| 6 | Claude | Records each result with the setup: the build, the Mac, macOS, and for a lookup the provider and model. "All worked" is not a record; each scenario gets its own line. The record goes in with the "Record the vX.Y.Z release" PR. |
| 7 | Owner | Publishes the draft when every scenario passes, or when the record says why a failure is accepted. Otherwise `scripts/release-candidate.sh --restore <backup>` puts the previous app and library back, and the fix becomes a new candidate. |

The script never deletes anything: the backup stays in `~/Library/Application Support/app.linen.reader.before-vX.Y.Z-<time>`, and `--restore` moves the candidate's app and library aside rather than over-writing them. Delete these folders by hand once the release has settled.

## Template

```markdown
# Linen vX.Y.Z — release acceptance

- **Release commit:** <sha> · **Draft:** <link>
- **In-app suite:** <n>/<n> on <sha>, <date>, `D1-no-uncaught-errors` included
- **Setup:** <Mac model>, macOS <version>, the <aarch64|x64> DMG; for lookups, <provider> and <model>
- **Backup:** `app.linen.reader.before-vX.Y.Z-<time>`

| #   | Scenario (rule) | Steps, with real input | Expected | Result |
| --- | --------------- | ---------------------- | -------- | ------ |
| 1   |                 |                        |          |        |

**Outcome:** published <date> · or withdrawn, and why
```

## How it is measured

The review that introduced this (2026-10-13) set the check: the next five releases each have a record with a scenario table and a setup, and at most one of them needs a fix release for something in a feature it changed.
