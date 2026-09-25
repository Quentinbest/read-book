#!/bin/sh
# Run the in-app end-to-end suite (or any spike) on macOS Desktop 2, so the
# harness window never covers the desktop in use (owner's instruction, 2026-09-25).
# Usage: scripts/e2e.sh [spike, default r] [check-id regex]
#   LINEN_SPACE=3 scripts/e2e.sh     another desktop
#   scripts/e2e.sh r '^(A1|E6)'      only matching checks
# Build first: LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes \
#              --config src-tauri/tauri.spikes.conf.json
set -e
cd "$(dirname "$0")/.."
DATA=$(mktemp -d)
LINEN_SPACE="${LINEN_SPACE:-2}" LINEN_DATA_DIR="$DATA" LINEN_SPIKE="${1:-r}" \
  LINEN_E2E_ONLY="${2:-}" LINEN_SPIKE_TIMEOUT="${LINEN_SPIKE_TIMEOUT:-3000}" \
  ./src-tauri/target/release/linen
