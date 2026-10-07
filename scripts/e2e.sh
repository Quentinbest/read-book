#!/bin/sh
# Run the in-app end-to-end suite (or any spike), on the current desktop.
# Usage: scripts/e2e.sh [spike, default r] [check-id regex] [app arguments]
#   LINEN_SPACE=2 scripts/e2e.sh     on Desktop 2, off the screen in use (optional
#                                    since the owner's 2026-09-29 instruction)
#   scripts/e2e.sh r '^(A1|E6)'      only matching checks
#   scripts/e2e.sh r '^scrollbar-survey$' -AppleShowScrollBars WhenScrolling
#                                    arguments after the regex go to the app; AppKit
#                                    reads these as defaults for this run only
# Build first: LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes \
#              --config src-tauri/tauri.spikes.conf.json
set -e
# caffeinate -d: a sleeping display stops WebKit's display link, and with it
# requestAnimationFrame, so a run longer than the display-sleep delay stalls when
# nobody is at the Mac (seen 2026-09-26: a 100 s stall). Only while the run lasts.
cd "$(dirname "$0")/.."
DATA=$(mktemp -d)
SPIKE="${1:-r}"
ONLY="${2:-}"
if [ $# -gt 2 ]; then shift 2; else set --; fi
LINEN_SPACE="${LINEN_SPACE:-}" LINEN_DATA_DIR="$DATA" LINEN_SPIKE="$SPIKE" \
  LINEN_E2E_ONLY="$ONLY" LINEN_SPIKE_TIMEOUT="${LINEN_SPIKE_TIMEOUT:-3000}" \
  caffeinate -di ./src-tauri/target/release/linen "$@"
