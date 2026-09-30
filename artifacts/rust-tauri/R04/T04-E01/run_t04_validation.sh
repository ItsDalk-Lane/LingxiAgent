#!/usr/bin/env bash
# R04-T04-E01 validation battery (final candidate, code-frozen).
# Usage: bash artifacts/rust-tauri/R04/T04-E01/run_t04_validation.sh <fresh-evidence-dir>
# Every command's real exit code is recorded in <evidence>/exit-codes.txt.
set -u
EVIDENCE="${1:?usage: run_t04_validation.sh <evidence-dir>}"
CARGO=/Users/study_superior/.cargo/bin/cargo
MANIFEST=rust/Cargo.toml
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$REPO_ROOT" || exit 1
mkdir -p "$EVIDENCE/gates" "$EVIDENCE/regression"
: > "$EVIDENCE/exit-codes.txt"

run_gate() {
  local name="$1"; shift
  echo "== $name: $*" | tee -a "$EVIDENCE/exit-codes.txt"
  "$@" > "$EVIDENCE/gates/${name}.log" 2>&1
  local code=$?
  echo "exit $code — $name" >> "$EVIDENCE/exit-codes.txt"
  echo "== $name exit $code"
}

run_gate rust_fmt_check "$CARGO" fmt --manifest-path "$MANIFEST" --all -- --check
run_gate rust_clippy "$CARGO" clippy --manifest-path "$MANIFEST" --workspace --all-targets --locked -- -D warnings
run_gate workspace_test "$CARGO" test --manifest-path "$MANIFEST" --workspace --locked
run_gate check_contracts "$CARGO" run --manifest-path "$MANIFEST" --locked -p xtask -- check-contracts
run_gate check_boundaries "$CARGO" run --manifest-path "$MANIFEST" --locked -p xtask -- check-boundaries
run_gate r04_t04_acceptance_tests "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t04_file_tools -- --nocapture
run_gate r04_t03_acceptance_tests "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t03_approval_service -- --nocapture
run_gate r04_t02_acceptance_tests "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t02_tool_gateway -- --nocapture
run_gate r04_t01_acceptance_tests "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-service --locked --test r04_t01_tool_catalog -- --nocapture
run_gate kernel_lib_tests "$CARGO" test --manifest-path "$MANIFEST" -p lingxi-kernel --locked --lib

bash scripts/rust-tauri/r03_g07_repair_suites.sh "$EVIDENCE/regression/repair-suites" \
  > "$EVIDENCE/gates/r03_g07_repair_suites.log" 2>&1
echo "exit $? — r03_g07_repair_suites" >> "$EVIDENCE/exit-codes.txt"
bash scripts/rust-tauri/r03_t08_matrix.sh "$EVIDENCE/regression/t08-a15" \
  > "$EVIDENCE/gates/r03_t08_matrix_a15.log" 2>&1
echo "exit $? — r03_t08_matrix_a15" >> "$EVIDENCE/exit-codes.txt"
bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh "$EVIDENCE/regression/t08-a16" \
  > "$EVIDENCE/gates/r03_t08_a16_seed.log" 2>&1
echo "exit $? — r03_t08_a16_seed" >> "$EVIDENCE/exit-codes.txt"

echo "==== FINAL ===="
cat "$EVIDENCE/exit-codes.txt"
