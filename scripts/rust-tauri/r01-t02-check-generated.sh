#!/usr/bin/env bash
# R01-T02 — "regenerate → no diff" gate for every generated artifact.
# Exit 0 only if both generated trees match regeneration exactly.
set -euo pipefail
cd "$(dirname "$0")/../.."

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/lingxi-r01t02-target}"

echo "== [1/2] contracts/generated (lingxi-protocol-gen --check)"
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true \
  cargo run -q -p lingxi-protocol --manifest-path rust/Cargo.toml \
  --bin lingxi-protocol-gen -- --check

echo "== [2/2] API_COMPAT_MATRIX.json (extract --check)"
node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --check

echo "OK: all generated artifacts are drift-free"
