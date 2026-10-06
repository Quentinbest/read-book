#!/bin/sh
# Append each release's download counts to docs/release-stats.md (plan Phase 9).
# DMGs are new installs; .app.tar.gz archives are existing installs updating.
# latest.json is fetched by every install daily and .sig files ride along with
# the archives, so neither is counted. Counts are cumulative per release.
set -e
cd "$(dirname "$0")/.."
OUT=docs/release-stats.md
[ -f "$OUT" ] || printf '# Release downloads\n\nWritten by `scripts/release-stats.sh`. Cumulative counts per release, from the public GitHub API. DMG = new installs; updater = existing installs updating.\n' >"$OUT"
{
  printf '\n## %s\n\n| Release | DMG Apple silicon | DMG Intel | Updater Apple silicon | Updater Intel |\n|---|---|---|---|---|\n' "$(date +%Y-%m-%d)"
  gh api repos/Quentinbest/read-book/releases --jq '.[] | select(.draft|not) |
    def n(p): ([.assets[] | select(.name|test(p)) | .download_count] | add // 0);
    "| \(.tag_name) | \(n("aarch64\\.dmg$")) | \(n("x64\\.dmg$")) | \(n("aarch64\\.app\\.tar\\.gz$")) | \(n("x64\\.app\\.tar\\.gz$")) |"'
} >>"$OUT"
echo "appended to $OUT"
