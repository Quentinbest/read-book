#!/usr/bin/env bash
# Mirrors .github/workflows/ci.yml (minus the Tauri build and Playwright; pass --full for Playwright).
# Rust comes from rust-toolchain.toml, as on CI.
set -euo pipefail
cd "$(dirname "$0")/.."
M=src-tauri/Cargo.toml
step() { echo "==> $*"; "$@"; }
step pnpm format:check
step pnpm lint
step pnpm check
step pnpm test
step pnpm design:check
step cargo fmt --manifest-path $M --check
step cargo clippy --manifest-path $M --all-targets -- -D warnings
# As on CI, the corpus tests must run, not skip (fetch.py skips verified copies).
step python3 scripts/corpus/fetch.py
step python3 scripts/corpus/generate.py
LINEN_REQUIRE_CORPUS=1 step cargo test --manifest-path $M
if [[ "${1:-}" == "--full" ]]; then step pnpm test:integration; fi
echo "ci-local: all green"
