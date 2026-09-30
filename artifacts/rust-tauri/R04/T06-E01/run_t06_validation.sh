#!/bin/bash
# R04-T06 validation rerun — the full gate set with real exit codes.
# Absolute-path form (repo root fixed); every command's exit code lands in
# exit-codes.txt and each log in gates/.
set -u
REPO_ROOT="/Users/study_superior/Desktop/Code/LingxiAgent"
CARGO="/Users/study_superior/.cargo/bin/cargo"
MANIFEST="$REPO_ROOT/rust/Cargo.toml"
OUT_DIR="$REPO_ROOT/artifacts/rust-tauri/R04/T06-E01"
GATES="$OUT_DIR/gates"
CODES="$OUT_DIR/exit-codes.txt"

cd "$REPO_ROOT" || exit 1
: > "$CODES"

run() {
  local name="$1"; shift
  echo "=== $name: $*" | tee -a "$CODES"
  "$@" > "$GATES/$name.log" 2>&1
  local rc=$?
  echo "$name exit=$rc" >> "$CODES"
  tail -2 "$GATES/$name.log" | head -1
  return 0
}

run rust_fmt_check      "$CARGO" fmt --manifest-path "$MANIFEST" --all -- --check
run rust_clippy         "$CARGO" clippy --manifest-path "$MANIFEST" --workspace --all-targets --locked -- -D warnings
run workspace_test      "$CARGO" test --manifest-path "$MANIFEST" --workspace --locked --no-fail-fast
run check_contracts     "$CARGO" run --manifest-path "$MANIFEST" --locked -p xtask -- check-contracts
run check_boundaries    "$CARGO" run --manifest-path "$MANIFEST" --locked -p xtask -- check-boundaries
run r04_t06_acceptance  "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t06_sandbox
run r04_t06_unit        "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --lib sandbox::
run r04_t05_process     "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t05_process_tools
run r04_t04_files       "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t04_file_tools
run r04_t03_approval    "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t03_approval_service
run r04_t02_gateway     "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t02_tool_gateway
run r04_t01_catalog     "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t01_tool_catalog
run kernel_lib_tests    "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-kernel --locked --lib

# R03 regressions (fresh isolated dirs)
R03_DIR="$OUT_DIR/regression"
mkdir -p "$R03_DIR"
run r03_g07_repair_suites bash "$REPO_ROOT/scripts/rust-tauri/r03_g07_repair_suites.sh" "$R03_DIR/g07-$(date +%s)"
run r03_t08_matrix_a15   bash "$REPO_ROOT/scripts/rust-tauri/r03_t08_matrix.sh" "$R03_DIR/a15-$(date +%s)"
run r03_t08_a16_seed     bash "$REPO_ROOT/scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh" "$R03_DIR/a16-$(date +%s)"

echo "=== summary ==="
cat "$CODES"
