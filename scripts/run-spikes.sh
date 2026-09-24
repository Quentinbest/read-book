#!/bin/sh
# Build the spike harness and run spikes (default: the interactive ones, B and C).
# Usage: scripts/run-spikes.sh [a,b,c,d,dt,e,f]
# Results: docs/spikes/raw/<spike>.json. See docs/spikes/*.md for each spike.
set -e
cd "$(dirname "$0")/.."
LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes --config src-tauri/tauri.spikes.conf.json
LINEN_SPIKE="${1:-b,c}" LINEN_SPIKE_TIMEOUT=3600 ./src-tauri/target/release/linen
