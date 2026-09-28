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
# incident; R9-F07 rework): the restore obligation is registered BEFORE
# the first byte that can modify the real kernel file, and restore()
# decides BY BYTES (compare against the pre-run sha256), never by a flag
# — the old `DIRTY=1` was only set AFTER `cat >>` completed, so a partial
# append failure (set -e → EXIT trap → DIRTY still 0) skipped the restore
# AND deleted the only backup, leaving a half-injected source. Restore
# now: unmodified bytes → no-op; modified bytes → write the backup back
# and VERIFY byte equality; only verified equality retires the
# obligation. A failed restore KEEPS the backup (the only copy of the
# original bytes) and exits nonzero.
#
# Usage: scripts/rust-tauri/r02_t02_f01_env_token_negative.sh [EVIDENCE_DIR]
# Exit 0 only if every phase holds (rejection + restoration + green).
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T02}"
mkdir -p "$EVIDENCE_DIR"

KERNEL=rust/crates/lingxi-kernel/src/lib.rs
CHECKER="python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py"

BACKUP=$(mktemp "${TMPDIR:-/tmp}/r02t02-f01-kernel.XXXXXX.rs")
cp "$KERNEL" "$BACKUP"
BASE_SHA="$(shasum -a 256 "$KERNEL" | awk '{print $1}')"

sha_of_kernel() {
  [ -f "$KERNEL" ] && shasum -a 256 "$KERNEL" | awk '{print $1}' || printf ''
}

restore() {
  local now_sha restored_sha
  now_sha="$(sha_of_kernel)"
  if [ "$now_sha" = "$BASE_SHA" ]; then
    # The file is byte-identical to the pre-run state — either never
    # actually modified (the obligation was registered speculatively
    # before the write) or already restored and verified. Retire the
    # obligation without writing anything.
    rm -f "$BACKUP"
    return 0
  fi
  # The kernel differs from its pre-run bytes: the obligation is LIVE.
  # Write the backup back, then VERIFY; only verified equality may retire
  # it. On any failure the backup (the only copy of the original bytes)
  # is KEPT and the failure is loud.
  if ! cp "$BACKUP" "$KERNEL"; then
    echo "ERROR: restore FAILED — original bytes preserved in $BACKUP (DO NOT DELETE); kernel left modified" >&2
    exit 1
  fi
  restored_sha="$(sha_of_kernel)"
  if [ "$restored_sha" != "$BASE_SHA" ]; then
    echo "ERROR: restore byte-verification FAILED ($restored_sha != $BASE_SHA) — original bytes preserved in $BACKUP (DO NOT DELETE)" >&2
    exit 1
  fi
  echo "cleanup: restored injected kernel lib.rs from backup (byte-verified)" >&2
  rm -f "$BACKUP"
}
trap restore EXIT

echo "== [1/4] green baseline"
$CHECKER > "$EVIDENCE_DIR/f01-0-baseline-green.log" 2>&1
tail -n 1 "$EVIDENCE_DIR/f01-0-baseline-green.log"

echo "== [2/4] inject std::env::var into lingxi-kernel source (non-comment)"
# R9-F07: the restore obligation goes live BEFORE the append — a partial
# write failure now finds an active trap that restores by BYTES (see
# restore above), never a DIRTY flag that was not yet set.
cat >> "$KERNEL" <<'EOF'

#[allow(dead_code)]
pub fn env_probe() -> Option<String> { std::env::var("LINGXI_PROBE").ok() }
EOF

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
echo "F01 RESULT: PASS (env token in kernel is machine-rejected; restoration byte-verified from a pre-registered obligation; checker green)"
