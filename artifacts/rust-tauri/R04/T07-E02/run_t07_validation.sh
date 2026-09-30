#!/usr/bin/env bash
# R04-T07 (E02) validation re-run script — absolute-path form.
# Reproduces every gate from docs/rust-tauri/R04/R04-T07_REPORT.md §5.
# Environment: rustup cargo at ~/.cargo/bin MUST take precedence over
# /opt/homebrew/bin (rust-toolchain.toml 1.98.1 is only read by rustup).
set -uo pipefail

REPO="/Users/study_superior/Desktop/Code/LingxiAgent"
CARGO="/Users/study_superior/.cargo/bin/cargo"
EVIDENCE="${REPO}/artifacts/rust-tauri/R04/T07-E02"
FRESH_DIR="${1:-/tmp/r04t07-e02-validation-$$}"

mkdir -p "${EVIDENCE}/gates" "${FRESH_DIR}"

run() {
  local name="$1"; shift
  echo "=== ${name}"
  "$@" > "${EVIDENCE}/gates/${name}.log" 2>&1
  local code=$?
  echo "exit=${code}" >> "${EVIDENCE}/gates/${name}.log"
  echo "${name}: exit ${code}"
}

cd "${REPO}"

run rust_fmt_check ${CARGO} fmt --manifest-path rust/Cargo.toml --all -- --check
run rust_clippy ${CARGO} clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
run workspace_test_nff ${CARGO} test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast
run r04_t07_acceptance_tests ${CARGO} test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t07_mcp_and_workers -- --test-threads=4
run r04_t07_unit_mcpbridge ${CARGO} test --manifest-path rust/Cargo.toml -p lingxi-service --locked --lib mcpbridge
run r04_t07_unit_workerrpc ${CARGO} test --manifest-path rust/Cargo.toml -p lingxi-service --locked --lib workerrpc
run check_contracts ${CARGO} run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
run check_boundaries ${CARGO} run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
run check_boundaries_selftest ${CARGO} run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries -- --self-test

# T01-T06 suite regression + kernel
for t in r04_t01_tool_catalog r04_t02_tool_gateway r04_t03_approval_service r04_t04_file_tools r04_t05_process_tools r04_t06_sandbox; do
  run "regression_${t}" ${CARGO} test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test "${t}"
done
run regression_kernel_lib ${CARGO} test --manifest-path rust/Cargo.toml -p lingxi-kernel --locked --lib

# R03 regression battery
run r03_g07_repair_suites bash scripts/rust-tauri/r03_g07_repair_suites.sh "${FRESH_DIR}/g07"
run r03_t08_matrix_a15 bash scripts/rust-tauri/r03_t08_matrix.sh "${FRESH_DIR}/a15"
run r03_t08_a16_seed bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh "${FRESH_DIR}/a16"

echo "done. logs in ${EVIDENCE}/gates/"
