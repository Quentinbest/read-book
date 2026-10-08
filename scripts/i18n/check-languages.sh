#!/bin/sh
# Stage 6 (docs/i18n-plan.md §6): the per-language smoke check. Runs the visual
# pass in English, then in each language, into i18n-out/visual/<tag>/, and judges
# them (scripts/i18n/check-visual.mjs): every screen captured, no UI text that
# overflows where English does not, nothing left in English, no missing key.
# About 4 minutes a language.
#
#   scripts/i18n/check-languages.sh              English, then every draft
#   scripts/i18n/check-languages.sh ja es        English, then these
#
# Build first (CLAUDE.md): LINEN_SPIKES=1 pnpm tauri build --no-bundle \
#   --features spikes --config src-tauri/tauri.spikes.conf.json
set -e
cd "$(dirname "$0")/../.."
# A test build only (scripts/e2e.sh refuses others too, but this loop would go on).
strings src-tauri/target/release/linen 2>/dev/null | grep -q spike_capture || {
  echo "src-tauri/target/release/linen is not a test build; build it first." >&2
  exit 1
}
LOCALES="${*:-zh-Hans zh-Hant ja es}"
OUT=i18n-out/visual

# English: the visual pass writes the baseline candidates; keep a copy, then put
# the tracked files back as they were.
rm -rf "$OUT/en"
scripts/e2e.sh v > /dev/null 2>&1
mkdir -p "$OUT/en"
cp docs/spikes/raw/visual-candidates.json docs/visual/app/*.png "$OUT/en/"
git checkout -- docs/visual/app docs/spikes/raw/visual-candidates.json

for l in $LOCALES; do
  rm -rf "${OUT:?}/$l"
  LINEN_LOCALE="$l" scripts/e2e.sh v > /dev/null 2>&1 || true
done
node scripts/i18n/check-visual.mjs $LOCALES
