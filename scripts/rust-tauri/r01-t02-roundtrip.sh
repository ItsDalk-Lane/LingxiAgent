#!/usr/bin/env bash
# R01-A03 — cross-language round-trip gate.
#   Rust encode (lingxi-protocol-gen golden) → TS decode/validate/re-encode
#   (tests/migration/r01-t02/roundtrip.mjs) → Rust read-back
#   (lingxi-protocol-verify). Plus cargo unit tests and the tsc type check of
#   the generated TS bindings.
set -euo pipefail
cd "$(dirname "$0")/../.."

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/lingxi-r01t02-target}"
TS_OUT="$(mktemp -d /tmp/lingxi-r01t02-ts-out.XXXXXX)"
trap 'rm -rf "$TS_OUT"' EXIT
CARGO_OFFLINE="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY CARGO_NET_OFFLINE=true"

echo "== [1/5] cargo test -p lingxi-protocol (incl. round-trip property tests)"
$CARGO_OFFLINE cargo test -p lingxi-protocol --manifest-path rust/Cargo.toml

echo "== [2/5] generated tree is drift-free (lingxi-protocol-gen --check)"
$CARGO_OFFLINE cargo run -q -p lingxi-protocol --manifest-path rust/Cargo.toml \
  --bin lingxi-protocol-gen -- --check

echo "== [3/5] TS consumer: decode + schema-validate + re-encode ($TS_OUT)"
node tests/migration/r01-t02/roundtrip.mjs \
  --generated contracts/generated --out "$TS_OUT"

echo "== [4/5] Rust read-back of the TS re-encoded copies"
$CARGO_OFFLINE cargo run -q -p lingxi-protocol --manifest-path rust/Cargo.toml \
  --bin lingxi-protocol-verify -- --ts "$TS_OUT"

echo "== [5/5] tsc --noEmit on generated TS bindings + type usage"
npx tsc -p tests/migration/r01-t02/tsconfig.json

echo "OK: R01-A03 round-trip gate passed (Rust→TS→Rust byte-identical)"
