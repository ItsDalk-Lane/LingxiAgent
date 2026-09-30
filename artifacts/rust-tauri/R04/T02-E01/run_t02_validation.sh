#!/usr/bin/env bash
# R04-T02 evidence reproduction script (EXECUTOR-R04-T02-E01, 2026-09-30).
#
# Re-runs every validation whose raw output is archived under
# artifacts/rust-tauri/R04/T02-E01/. Fresh evidence directories are used
# (the R03 producers refuse non-empty dirs); the FIRST-run fragments under
# r03-*-regression-final/ are the archived ones.
#
# Toolchain: rustup 1.98.1 via rust-toolchain.toml; ~/.cargo/bin MUST
# precede /opt/homebrew/bin in PATH. Exit codes are printed per step; the
# script exits non-zero if any step fails, EXCEPT the workspace test's
# r00_management_leaves environment failure (documented macOS-firewall
# form; verify the isolated rerun text instead).
set -uo pipefail
cd "$(dirname "$0")/../../.."   # repo root
CARGO=/Users/study_superior/.cargo/bin/cargo
EVIDENCE="$(pwd)/artifacts/rust-tauri/R04/T02-E01"

run() { echo "== $* =="; "$@"; echo "exit: $?"; }

run $CARGO fmt --manifest-path rust/Cargo.toml --all -- --check
run $CARGO clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
run $CARGO test --manifest-path rust/Cargo.toml --workspace --locked
run $CARGO test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r00_management_leaves
run $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
run $CARGO run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
run $CARGO test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t02_tool_gateway -- --nocapture
run $CARGO test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t01_tool_catalog
run $CARGO test --manifest-path rust/Cargo.toml -p lingxi-service --locked --lib toolgateway
run bash scripts/rust-tauri/r03_g07_repair_suites.sh "$EVIDENCE/r03-repair-suites-regression-repro"
run bash scripts/rust-tauri/r03_t08_matrix.sh "$EVIDENCE/r03-regression-repro/A15"
run bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh "$EVIDENCE/r03-regression-repro/A16"
