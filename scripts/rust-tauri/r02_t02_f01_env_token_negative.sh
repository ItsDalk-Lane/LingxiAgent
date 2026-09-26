#!/usr/bin/env bash
# R02-T02 / F01 closure (R02-T01 REVIEW_R1 finding) — machine-enforce
# "the domain never reads environment variables" via DEP-08
# forbidden_source_tokens (std::env:: / env::var).
#
# Proves with a REAL negative injection into lingxi-kernel source:
#   green baseline -> inject std::env::var usage -> checker FAILs with
#   [D3] DEP-08 naming the forbidden token and file -> restore (backup
#   copy, byte-verified) -> checker green again -> zero residue.
#
# Restore discipline (T01 A02 lesson, plus this task's own first-run
# incident): the injected marker is only cleared while DIRTY=1, and the
# EXIT trap fires BEFORE the flag is ever cleared; byte equality is
# verified against the pre-run shasum, not just the script's own copy.
#
# Usage: scripts/rust-tauri/r02_t02_f01_env_token_negative.sh [EVIDENCE_DIR]
# Exit 0 only if every phase holds (rejection + restoration + green).
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T02}"
mkdir -p "$EVIDENCE_DIR"

KERNEL=rust/crates/lingxi-kernel/src/lib.rs
CHECKER="python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py"

BACKUP="$(mktemp /tmp/r02t02-f01-kernel.XXXXXX.rs)"
cp "$KERNEL" "$BACKUP"
BASE_SHA="$(shasum -a 256 "$KERNEL" | awk '{print $1}')"
DIRTY=0

restore() {
  if [ "$DIRTY" = "1" ]; then
    cp "$BACKUP" "$KERNEL"
    echo "cleanup: restored injected kernel lib.rs from backup" >&2
  fi
  rm -f "$BACKUP"
}
trap restore EXIT

echo "== [1/4] green baseline"
$CHECKER > "$EVIDENCE_DIR/f01-0-baseline-green.log" 2>&1
tail -n 1 "$EVIDENCE_DIR/f01-0-baseline-green.log"

echo "== [2/4] inject std::env::var into lingxi-kernel source (non-comment)"
cat >> "$KERNEL" <<'EOF'

#[allow(dead_code)]
pub fn env_probe() -> Option<String> { std::env::var("LINGXI_PROBE").ok() }
EOF
DIRTY=1

set +e
$CHECKER > "$EVIDENCE_DIR/f01-1-injected-rejected.log" 2>&1
RC=$?
set -e
echo "checker exit with injection: $RC"
[ "$RC" -ne 0 ] || { echo "ERROR: checker accepted kernel env access" >&2; exit 1; }
grep -q "FAIL \[D3\] DEP-08: forbidden token 'std::env::' in rust/crates/lingxi-kernel/src/lib.rs" \
  "$EVIDENCE_DIR/f01-1-injected-rejected.log" || {
    echo "ERROR: rejection is not the expected D3/DEP-08 token violation:" >&2
    cat "$EVIDENCE_DIR/f01-1-injected-rejected.log" >&2
    exit 1
  }
grep -m1 "FAIL" "$EVIDENCE_DIR/f01-1-injected-rejected.log"

echo "== [3/4] restore and verify byte equality against pre-run sha256"
restore
DIRTY=0
NOW_SHA="$(shasum -a 256 "$KERNEL" | awk '{print $1}')"
[ "$NOW_SHA" = "$BASE_SHA" ] || {
  echo "ERROR: kernel lib.rs sha mismatch after restore ($NOW_SHA != $BASE_SHA)" >&2
  exit 1
}
echo "kernel lib.rs byte-identical to pre-run state: OK"

echo "== [4/4] checker green again"
$CHECKER > "$EVIDENCE_DIR/f01-2-restored-green.log" 2>&1
tail -n 1 "$EVIDENCE_DIR/f01-2-restored-green.log"

trap - EXIT
echo "F01 RESULT: PASS (env token in kernel is machine-rejected; restoration byte-verified; checker green)"
