#!/usr/bin/env bash
# R04-T03 validation replay (final candidate). Absolute paths, run from
# anywhere. Every command's exit code is recorded by the caller.
set -uo pipefail
CARGO=/Users/study_superior/.cargo/bin/cargo
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
MANIFEST="$REPO/rust/Cargo.toml"

echo "== 1. cargo fmt --all -- --check =="
"$CARGO" fmt --manifest-path "$MANIFEST" --all -- --check
echo "fmt exit: $?"

echo "== 2. cargo clippy --workspace --all-targets --locked -- -D warnings =="
"$CARGO" clippy --manifest-path "$MANIFEST" --workspace --all-targets --locked -- -D warnings
echo "clippy exit: $?"

echo "== 3. cargo test --workspace --locked =="
"$CARGO" test --manifest-path "$MANIFEST" --workspace --locked
echo "workspace test exit: $?"

echo "== 4. xtask check-contracts =="
"$CARGO" run --manifest-path "$MANIFEST" --locked -p xtask -- check-contracts
echo "check-contracts exit: $?"

echo "== 5. xtask check-boundaries (+ self-test battery) =="
"$CARGO" run --manifest-path "$MANIFEST" --locked -p xtask -- check-boundaries
echo "check-boundaries exit: $?"
python3 -B "$REPO/docs/rust-tauri/R01/r01_t01_check_ownership.py" --self-test
echo "boundaries self-test exit: $?"

echo "== 6. T03 acceptance suite + lib units =="
"$CARGO" test --manifest-path "$MANIFEST" --locked -p lingxi-service --test r04_t03_approval_service -- --nocapture
echo "t03 suite exit: $?"
"$CARGO" test --manifest-path "$MANIFEST" --locked -p lingxi-service --lib approval_service
echo "t03 lib exit: $?"

echo "== 7. T01/T02 regression suites =="
"$CARGO" test --manifest-path "$MANIFEST" --locked -p lingxi-service --test r04_t02_tool_gateway
echo "t02 suite exit: $?"
"$CARGO" test --manifest-path "$MANIFEST" --locked -p lingxi-service --test r04_t01_tool_catalog
echo "t01 suite exit: $?"
"$CARGO" test --manifest-path "$MANIFEST" --locked -p lingxi-kernel --lib
echo "kernel lib exit: $?"

echo "== 8. R03 regression (SUP-05): ten repair suites + A15 + A16 =="
bash "$REPO/scripts/rust-tauri/r03_g07_repair_suites.sh" /tmp/r04t03-replay/repair-suites
echo "r03 suites exit: $?"
bash "$REPO/scripts/rust-tauri/r03_t08_matrix.sh" /tmp/r04t03-replay/t08-a15
echo "a15 exit: $?"
bash "$REPO/scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh" /tmp/r04t03-replay/t08-a16
echo "a16 exit: $?"
