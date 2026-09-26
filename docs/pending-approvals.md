# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

Approved so far; the record is in `docs/decisions.md`:
- items 1–8 and 11–20, on 2026-09-25;
- items 9, 10 and 21–28, on 2026-09-26 (item 28 as the owner settled it: builds need only meet what GitHub requires until there is a Developer ID).

## Phase 8

29. **Where updates are served from (D6).** The updater is built and checked against the Linen update key, but `Quentinbest/read-book` is private. An installed Linen asks for `…/releases/latest/download/latest.json` without logging in and gets 404, so it would never see an update. The app's address for updates is fixed at build time, so this needs deciding before the first release that people install.
    *Recommendation:* a public repository that holds only releases (for example `Quentinbest/linen-releases`), while the code stays private.
    - The release workflow would publish there, with a token that can write to it (a repository secret).
    - The app's update address changes in one line of `src-tauri/tauri.conf.json`.
    - The alternatives are making `read-book` public, or no automatic updates until later.

30. **Screen 08: the Notes tab's export footer (Phase 7).** Phase 7 added “Export as Markdown · via Markdown Export extension” at the foot of the Notes tab. It was not in the Phase 7 review, so Screen 08's baseline still lacks it. See `docs/visual/diff/08-navigator-notes.png` (the red text at the bottom left; the other marks are the inactive window on Desktop 2).
    *Recommendation:* approve, and take `docs/visual/app/08-navigator-notes.png` as the new baseline.

## Needs a person, not a decision

- **GitHub Actions is not running.** Since 2026-09-25 14:02 every run stops before it starts: “recent account payments have failed or your spending limit needs to be increased” (Billing & plans in the GitHub account settings). The code is fine: every CI step passes locally on 2026-09-26, and one clippy error that CI would have caught is fixed. The release workflow (`.github/workflows/release.yml`) needs Actions too.
