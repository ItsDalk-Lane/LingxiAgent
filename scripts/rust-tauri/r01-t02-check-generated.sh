#!/usr/bin/env bash
# R01-T02 — "regenerate → no diff" gate for every generated artifact.
# Exit 0 only if both generated trees match regeneration exactly.
#
# Toolchain (fix-headsha-r1 / F2): cargo is invoked EXPLICITLY through the
# rustup-pinned toolchain parsed from rust-toolchain.toml, never a bare
# PATH-resolved cargo — the gate must not depend on PATH ordering (e.g. a
# Homebrew cargo earlier in PATH).
#
# RR-T08-F1 hardening (R02-T08): the default CARGO_TARGET_DIR is DERIVED
# FROM THIS CHECKOUT'S PATH, so two checkouts never share build artifacts.
# The former fixed shared default (/tmp/lingxi-r01t02-target) let a
# leftover binary compiled in another checkout validate that checkout's
# contracts/generated tree instead of this one (wrong-tree green,
# 2026-09-25). A lingxi-protocol-gen compiled for THIS checkout also
# refuses at runtime to validate a foreign checkout (devgate::bound_repo_root).
# An explicitly provided CARGO_TARGET_DIR is still honored.
set -euo pipefail
cd "$(dirname "$0")/../.."

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/lingxi-r01t02-target-$(printf '%s' "$PWD" | shasum -a 256 | cut -c1-16)}"

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
if [ -z "$TOOLCHAIN" ]; then
  echo "ERROR: cannot parse toolchain channel from rust-toolchain.toml" >&2
  exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
  # rustup's standard install location; still explicit, never a bare PATH cargo.
  if [ -x "$HOME/.cargo/bin/rustup" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "ERROR: rustup not found; this gate requires the locked toolchain ($TOOLCHAIN) via rustup" >&2
    exit 1
  fi
fi
echo "== toolchain: rustup run $TOOLCHAIN ($(rustup run "$TOOLCHAIN" rustc --version))"

echo "== [1/2] contracts/generated (lingxi-protocol-gen --check)"
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true \
  rustup run "$TOOLCHAIN" cargo run -q -p lingxi-protocol --manifest-path rust/Cargo.toml \
  --bin lingxi-protocol-gen -- --check

echo "== [2/2] API_COMPAT_MATRIX.json (extract --check)"
node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --check

echo "OK: all generated artifacts are drift-free"
