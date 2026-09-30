#!/usr/bin/env bash
# R04-T05 validation replay (EXECUTOR-R04-T05-E01).
# Absolute-path convention (same as T01-T04 replay scripts). Records each
# command's real exit code into exit-codes.txt; the final-candidate run's
# outputs live in gates/.
set -u
REPO=/Users/study_superior/Desktop/Code/LingxiAgent
CARGO=/Users/study_superior/.cargo/bin/cargo
OUT="$REPO/artifacts/rust-tauri/R04/T05-E01"
G="$OUT/gates"
mkdir -p "$G"
CODES="$OUT/exit-codes.txt"
: > "$CODES"

run() {
  local name="$1"; shift
  echo "== $name: $*" | tee -a "$CODES"
  "$@" > "$G/${name}.log" 2>&1
  local code=$?
  echo "exit $code" | tee -a "$CODES" >> "$G/${name}.log" 2>&1
  echo "$name exit $code" >> "$CODES"
}

cd "$REPO" || exit 1
run rust_fmt_check      "$CARGO" fmt --manifest-path rust/Cargo.toml --all -- --check
run rust_clippy         "$CARGO" clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
# NOTE: the workspace run's expected shape on this machine is
# "77 binaries ok, 823 passed, 1 environment failure (r00_management_leaves,
#  macOS firewall vs the newly-built unsigned test binary)" — the same
# intermittent registered item from T01/T02. --no-fail-fast is used so every
# binary still runs; the isolated r00 rerun log sits next to this script.
run workspace_test      "$CARGO" test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast
run check_contracts     "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
run check_boundaries    "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
run check_boundaries_selftest "$CARGO" run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries --self-test
run r04_t05_acceptance_tests "$CARGO" test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t05_process_tools
run r04_t05_unit_tests  "$CARGO" test --manifest-path rust/Cargo.toml -p lingxi-service --locked --lib procsupervisor
run r04_t01_regression  "$CARGO" test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t01_tool_catalog
run r04_t02_regression  "$CARGO" test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t02_tool_gateway
run r04_t03_regression  "$CARGO" test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t03_approval_service
run r04_t04_regression  "$CARGO" test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t04_file_tools
run kernel_lib_tests    "$CARGO" test --manifest-path rust/Cargo.toml -p lingxi-kernel --locked --lib

# R03 regression scripts (script exit codes appended to their logs).
R3A=/tmp/r04t05-replay-suites; R3B=/tmp/r04t05-replay-a15; R3C=/tmp/r04t05-replay-a16
rm -rf "$R3A" "$R3B" "$R3C"; mkdir -p "$R3A" "$R3B" "$R3C"
bash scripts/rust-tauri/r03_g07_repair_suites.sh "$R3A" > "$G/r03_g07_repair_suites.log" 2>&1
echo "exit $?" >> "$G/r03_g07_repair_suites.log"; echo "r03_g07_repair_suites exit $?" >> "$CODES"
bash scripts/rust-tauri/r03_t08_matrix.sh "$R3B" > "$G/r03_t08_matrix_a15.log" 2>&1
echo "exit $?" >> "$G/r03_t08_matrix_a15.log"; echo "r03_t08_matrix_a15 exit $?" >> "$CODES"
bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh "$R3C" > "$G/r03_t08_a16_seed.log" 2>&1
echo "exit $?" >> "$G/r03_t08_a16_seed.log"; echo "r03_t08_a16_seed exit $?" >> "$CODES"

echo "---- summary ----"
cat "$CODES"
