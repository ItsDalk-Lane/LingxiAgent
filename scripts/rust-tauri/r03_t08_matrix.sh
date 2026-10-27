#!/usr/bin/env bash
# R03-T08 / acceptance R03-A15 — combination-matrix + leaf-case producer.
#
# Drives the REAL service chain (SessionStore admission → busy gate →
# RunSupervisor → kernel state machine/fence → real RunDatabase → real
# EventService) with deterministic external-response doubles and records:
#   - <DIR>/combo-counts.json   — task/call/terminal counters per combo
#     (normal, multi-turn model, multi-tool, timeout, cancel stream-read,
#     cancel approval-wait, duplicate, out-of-order, cross-session,
#     crash recovery, steering, subagent family, background, reconnect);
#   - <DIR>/leaf-cases.json     — lingxi.leaf-case-results.v1 file the
#     R03 stage gate's supplemental-leaf assertion contracts consume;
#   - <DIR>/integration.log     — the raw test stdout (the A15
#     invocation-observation + integration log).
#
# The doubles only produce external responses; every state/event/terminal
# comes from the real supervisor and storage (A15 boundary).
#
# Usage: scripts/rust-tauri/r03_t08_matrix.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R03/T08/A15}"
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"

TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r03-t08-matrix}"
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

note "== building the acceptance matrix test (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
  rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-service --test r03_t08_acceptance_matrix --no-run \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
note "PASS build (locked, offline)"

note "== running the combination matrix through the REAL chain (A15) =="
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
  R03_T08_EVIDENCE_DIR="$EVIDENCE_DIR" \
  rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-service --test r03_t08_acceptance_matrix -- --nocapture --test-threads=4 \
  > "$EVIDENCE_DIR/integration.log" 2>&1 \
  || { tail -60 "$EVIDENCE_DIR/integration.log"; fail "matrix run failed"; }
grep -q "test result: ok" "$EVIDENCE_DIR/integration.log" || fail "matrix run must be green"
note "PASS combination matrix green (real chain, doubles = external responses only)"

# Assemble the machine-consumable case files from the fragments the test
# wrote (each fragment was ASSERTED in-process; this only packages them).
python3 - "$EVIDENCE_DIR" << 'PYEOF' || fail "case assembly failed"
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
cases_dir = ev / "cases"
combos_dir = ev / "combos"
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
    "producedBy": "cargo test -p lingxi-service --test r03_t08_acceptance_matrix (real chain)",
    "cases": cases,
}
(ev / "leaf-cases.json").write_text(json.dumps(leaf, indent=1, ensure_ascii=False) + "\n")
combos = []
for frag in sorted(combos_dir.glob("*.json")):
    doc = json.loads(frag.read_text())
    combos.append({"combo": doc["combo"], "counts": doc["counts"]})
summary = {
    "schema": "lingxi.r03-t08-combo-matrix.v1",
    "combos": combos,
    "caseCount": len(cases),
    "allCasesOk": all(c["ok"] for c in cases),
}
(ev / "combo-counts.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False) + "\n")
print(f"assembled {len(cases)} leaf cases and {len(combos)} combo counters")
PYEOF
note "PASS case files assembled (leaf-cases.json + combo-counts.json)"
note "RESULT: R03-T08 matrix evidence ALL GREEN"
