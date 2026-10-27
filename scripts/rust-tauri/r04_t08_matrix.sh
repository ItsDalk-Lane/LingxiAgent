#!/usr/bin/env bash
# R04-T08 / acceptance R04-A15 + R04-A16 — the full tool matrix + leaf-case
# producer.
#
# Drives the REAL tool chain (registry → unified gateway → policy/approval →
# the native file/process executors, the rmcp stdio bridge and the worker
# RPC — through real run chains where a run is involved) and records:
#   - <DIR>/leaf-cases.json   — lingxi.leaf-case-results.v1 file the R04
#     stage gate's supplemental-leaf assertion contracts consume (every
#     case was ASSERTED in-process first; this only packages the
#     fragments);
#   - <DIR>/matrix-counts.json — the matrix cell counters (tool families,
#     permission/entry/lifecycle consistency, A15/A16 scenario outcomes);
#   - <DIR>/integration.log   — the raw test stdout;
#   - <DIR>/summary.txt       — the gate lines.
#
# The doubles only supply external responses (the StepsProvider plays the
# R05 model, the fixture children play external workers/MCP servers, the
# inline probe executors play external tools whose DELIVERED references
# the gateway's registration audit judges); the gateway, the policy, the
# registry, the journal and the driver are the real implementations.
#
# Usage: scripts/rust-tauri/r04_t08_matrix.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R04/T08/R04_MATRIX}"
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"

TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r04-t08-matrix}"
TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
if [ -z "$TOOLCHAIN" ]; then
  echo "ERROR: cannot parse toolchain channel from rust-toolchain.toml" >&2
  exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/rustup" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "ERROR: rustup not found; this gate requires the locked toolchain ($TOOLCHAIN)" >&2
    exit 1
  fi
fi

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

note "== building the tool-matrix test (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
  rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-service --test r04_t08_tool_matrix --no-run \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
note "PASS build (locked, offline)"

note "== running the full tool matrix through the REAL chain (A15/A16 + matrix cells) =="
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
  R04_T08_EVIDENCE_DIR="$EVIDENCE_DIR" \
  rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-service --test r04_t08_tool_matrix -- --test-threads=2 \
  > "$EVIDENCE_DIR/integration.log" 2>&1 \
  || { tail -60 "$EVIDENCE_DIR/integration.log"; fail "matrix run failed"; }
grep -q "test result: ok" "$EVIDENCE_DIR/integration.log" || fail "matrix run must be green"
note "PASS tool matrix green (real chain; doubles = external responses only)"

# Assemble the machine-consumable case files from the fragments the test
# wrote (each fragment was ASSERTED in-process; this only packages them).
python3 - "$EVIDENCE_DIR" << 'PYEOF' || fail "case assembly failed"
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
cases_dir = ev / "cases"
fragments = sorted(cases_dir.glob("*.json"))
if not fragments:
    raise SystemExit("no case fragments were produced")
cases = []
for frag in fragments:
    doc = json.loads(frag.read_text())
    cases.append({
        "case": doc["case"],
        "expect": doc["expect"],
        "actual": doc["actual"],
        "ok": doc["ok"],
    })
leaf = {
    "schema": "lingxi.leaf-case-results.v1",
    "producedBy": "cargo test -p lingxi-service --test r04_t08_tool_matrix (real chain)",
    "cases": cases,
}
(ev / "leaf-cases.json").write_text(json.dumps(leaf, indent=1, ensure_ascii=False) + "\n")
matrix = {
    "schema": "lingxi.r04-t08-tool-matrix.v1",
    "caseCount": len(cases),
    "allCasesOk": all(c["ok"] for c in cases),
    "caseNames": [c["case"] for c in cases],
}
(ev / "matrix-counts.json").write_text(json.dumps(matrix, indent=1, ensure_ascii=False) + "\n")
print(f"assembled {len(cases)} leaf cases")
PYEOF
note "PASS case files assembled (leaf-cases.json + matrix-counts.json)"
note "RESULT: R04-T08 matrix evidence ALL GREEN"
