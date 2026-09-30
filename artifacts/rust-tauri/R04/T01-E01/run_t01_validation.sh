#!/usr/bin/env bash
# R04-T01 evidence reproduction script (EXECUTOR-R04-T01-E01, 2026-09-30).
#
# Re-runs every validation whose raw output is archived under
# artifacts/rust-tauri/R04/T01-E01/. Paths are absolute so the A15/A16
# producers write their fragments where this script's caller can see them
# (the matrix test resolves its R03_T08_EVIDENCE_DIR relative to the
# PACKAGE root, not the repo root — relative paths scatter evidence; the
# registered stage gate always passes absolute paths).
#
# Toolchain: rustup 1.98.1 via rust-toolchain.toml. ~/.cargo/bin MUST
# precede /opt/homebrew/bin in PATH (Homebrew cargo 1.93.0 ignores the
# lock). CARGO=/Users/study_superior/.cargo/bin/cargo below is the safest
# form. Exit codes are printed per step; the script exits non-zero if any
# step fails, EXCEPT step 6's r00_management_leaves environment failure is
# reported verbatim (see report §验证 for the firewall analysis).
set -uo pipefail
cd "$(dirname "$0")/../../.."   # repo root
CARGO=/Users/study_superior/.cargo/bin/cargo
EVIDENCE="$(pwd)/artifacts/rust-tauri/R04/T01-E01"

run() { echo "== $* =="; "$@"; echo "exit: $?"; }

run $CARGO fmt --manifest-path rust/Cargo.toml --all -- --check
run $CARGO clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
run $CARGO test --manifest-path rust/Cargo.toml --workspace --locked
run $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
run $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
run $CARGO test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t01_tool_catalog -- --nocapture
run node "$EVIDENCE/cross-lang-canon-nonbmp.mjs"
run bash scripts/rust-tauri/r03_g07_repair_suites.sh "$EVIDENCE/r03-repair-suites-regression-repro"
run bash scripts/rust-tauri/r03_t08_matrix.sh "$EVIDENCE/r03-regression-repro/A15"
run bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh "$EVIDENCE/r03-regression-repro/A16"
