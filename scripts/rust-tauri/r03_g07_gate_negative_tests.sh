#!/usr/bin/env bash
# R03 repair round G07 / F08-C02 + F08-C03 — gate NEGATIVE tests.
#
# Proves on an ISOLATED /tmp copy of the repo (never by tampering the
# production gate in place) that the R03 stage gate fails CLOSED with the
# gap NAMED for every fake-green shape the repair mappings must not allow:
#
#   N1a delete one supplemental-leaf mapping      → verify-stage non-zero,
#        the dropped R00 leaf id is named (pre-run cross-check abort);
#   N1b delete the repair_suites command mapping  → verify-stage non-zero,
#        "references unknown command" names it (parse refusal);
#   N4  delete the R03-RP01 repair scenario       → the xtask map-pinning
#        tests (part of the gate's own rust_test_workspace command) go
#        red naming R03-RP01;
#   N2  a test filter that matches 0 tests        → the registered
#        producer fails naming the suite + counts, and the full
#        verify-stage R03 gate exits non-zero (no overall PASS);
#   N3  a declared evidence file never written    → the command itself is
#        green but verify-stage fails on missingEvidence naming the path;
#   C03a a non-empty (stale) evidence root        → verify-stage refuses to
#        run, "not empty" (old results can never mask a new run);
#   C03b an execution input changes mid-run       → the gate's candidate
#        binding marks the run unstable (stable=false, reason recorded)
#        and FORCES overall FAIL — the earlier PASS cannot be reused.
#
# Usage: scripts/rust-tauri/r03_g07_gate_negative_tests.sh [EVIDENCE_DIR]
# Every case's real exit code and the gap-naming text are archived under
# EVIDENCE_DIR; the script exits 0 only if EVERY case exited non-zero AND
# the expected gap text was found.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
EV="${1:-$ROOT/artifacts/rust-tauri/R03/repair-current/G07-E01/negative-tests}"
mkdir -p "$EV"
: > "$EV/summary.txt"
: > "$EV/case-results.tsv"

note() { printf '%s\n' "$*" | tee -a "$EV/summary.txt"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

CARGO="$HOME/.cargo/bin/cargo"
[ -x "$CARGO" ] || fail "~/.cargo/bin/cargo (rustup 1.98.1) not found"
export CARGO_NET_OFFLINE=true
NEG_TARGET="/tmp/rust-target-r03-g07-neg"

# ── isolated copy: git worktree at HEAD + the uncommitted gate files ────────
COPY="$(mktemp -d /tmp/lingxi-r03-g07-neg-XXXXXX)"
note "== preparing isolated copy $COPY (git worktree at HEAD $(git -C "$ROOT" rev-parse --short HEAD) + uncommitted gate files) =="
if ! git -C "$ROOT" worktree add --detach "$COPY" HEAD > "$EV/worktree-add.log" 2>&1; then
  cat "$EV/worktree-add.log"; fail "worktree add failed"
fi
cleanup() { git -C "$ROOT" worktree remove --force "$COPY" >/dev/null 2>&1 || true; }
trap cleanup EXIT

for f in \
  rust/crates/xtask/src/stage_maps/R03.json \
  rust/crates/xtask/src/stage_map.rs \
  rust/crates/xtask/src/verify/runner_tests.rs \
  scripts/rust-tauri/r03_t08_generate_stage_map.py \
  scripts/rust-tauri/r03_g07_repair_suites.sh \
  scripts/rust-tauri/r03_g07_gate_negative_tests.sh; do
  mkdir -p "$COPY/$(dirname "$f")"
  cp "$ROOT/$f" "$COPY/$f"
done
chmod +x "$COPY/scripts/rust-tauri/r03_g07_repair_suites.sh"
# The directed legacy-regression chain reuses node_modules from the repo
# root it runs in; give the copy a clonefile (CoW) copy so the /tmp gate
# run is representative instead of failing on a missing dependency tree.
if [ -d "$ROOT/node_modules" ]; then
  note "== cloning node_modules into the copy (cp -Rc, CoW) =="
  cp -Rc "$ROOT/node_modules" "$COPY/node_modules" 2>/dev/null || cp -R "$ROOT/node_modules" "$COPY/node_modules"
fi

# Pristine copies for resetting between cases.
mkdir -p "$EV/pristine"
cp "$COPY/rust/crates/xtask/src/stage_maps/R03.json" "$EV/pristine/R03.json"
cp "$COPY/scripts/rust-tauri/r03_g07_repair_suites.sh" "$EV/pristine/r03_g07_repair_suites.sh"
reset_copy() {
  cp "$EV/pristine/R03.json" "$COPY/rust/crates/xtask/src/stage_maps/R03.json"
  cp "$EV/pristine/r03_g07_repair_suites.sh" "$COPY/scripts/rust-tauri/r03_g07_repair_suites.sh"
}

record_case() { # name exit_code named verdict
  echo "$3" > /dev/null
  note "case $1: exit=$2 gap-named=$3 verdict=$4"
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$EV/case-results.tsv"
}

# gate_run NAME [pattern...] — runs the REAL verify-stage R03 inside the
# copy, records exit code + output, and requires: non-zero exit AND every
# pattern present somewhere in the run output or the written result JSON.
gate_run() {
  local name="$1"; shift
  local dir="$EV/$name"
  mkdir -p "$dir"
  (cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" run --manifest-path rust/Cargo.toml \
     --locked -p xtask -- verify-stage R03 --evidence "$dir/evidence" \
     > "$dir/gate.stdout.log" 2> "$dir/gate.stderr.log")
  local exit_code=$?
  echo "$exit_code" > "$dir/exit-code.txt"
  # The gap text can legitimately live in the gate's own logs, the written
  # result JSON, the tampered producer's command stdout/stderr, or the
  # producer's evidence files under the case evidence root — all of them
  # are first-run artifacts of THIS case (the driver assertion was widened
  # after the first N2 run proved the gate refuses but the naming text
  # lived in gaps.txt/summary.txt, which the original haystack missed).
  local haystack="$dir/gate.stdout.log $dir/gate.stderr.log"
  local extra
  for extra in \
    "$dir/evidence/verify-stage-result.json" \
    "$dir/evidence/repair_suites/stdout.log" \
    "$dir/evidence/repair_suites/stderr.log" \
    "$dir/evidence/G07_REPAIR/summary.txt" \
    "$dir/evidence/G07_REPAIR/gaps.txt"; do
    [ -f "$extra" ] && haystack="$haystack $extra"
  done
  local named=OK
  local pattern
  for pattern in "$@"; do
    if ! grep -qF -- "$pattern" $haystack 2>/dev/null; then named="MISSING:$pattern"; break; fi
  done
  local verdict=BAD
  if [ "$exit_code" -ne 0 ] && [ "$named" = "OK" ]; then verdict=OK; fi
  record_case "$name" "$exit_code" "$named" "$verdict"
}

# ── N1a: delete one supplemental-leaf mapping from the FULL real map ────────
reset_copy
LEAF_ID="$(python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R03.json"
m = json.load(open(p))
leaf = m["supplementalLeafScenarios"][0]
del m["supplementalLeafScenarios"][0]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
print(leaf["id"])
PY
)"
note "== N1a: deleted supplemental leaf mapping $LEAF_ID from the copy's map =="
gate_run n1a-delete-leaf-mapping "drops 1 REQUIRED_SUPPLEMENTAL leaf scenario(s)" "$LEAF_ID"

# ── N1b: delete the repair_suites command mapping (scenario keeps it) ──────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R03.json"
m = json.load(open(p))
del m["commands"]["repair_suites"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== N1b: deleted the repair_suites command mapping from the copy's map =="
gate_run n1b-delete-command-mapping "references unknown command" "repair_suites"

# ── N4: delete the R03-RP01 repair scenario → xtask map-pinning tests red ──
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R03.json"
m = json.load(open(p))
m["scenarios"] = [s for s in m["scenarios"] if s["id"] != "R03-RP01"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
N4_DIR="$EV/n4-delete-repair-scenario"
mkdir -p "$N4_DIR"
(cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" test --manifest-path rust/Cargo.toml \
   --locked -p xtask --bin xtask r03_ \
   > "$N4_DIR/test.stdout.log" 2>&1)
N4_EXIT=$?
echo "$N4_EXIT" > "$N4_DIR/exit-code.txt"
N4_NAMED=OK
grep -qF "R03-RP01" "$N4_DIR/test.stdout.log" || N4_NAMED="MISSING:R03-RP01"
N4_VERDICT=BAD; if [ "$N4_EXIT" -ne 0 ] && [ "$N4_NAMED" = "OK" ]; then N4_VERDICT=OK; fi
note "== N4: deleted the R03-RP01 scenario from the copy's map; xtask map-pinning tests: =="
record_case "n4-delete-repair-scenario" "$N4_EXIT" "$N4_NAMED" "$N4_VERDICT"

# ── C03a: a non-empty (stale) evidence root is refused outright ────────────
reset_copy
mkdir -p "$EV/c03a-stale-evidence-root/evidence"
echo "stale evidence from an earlier run" > "$EV/c03a-stale-evidence-root/evidence/old.txt"
note "== C03a: evidence root pre-filled with a stale file =="
gate_run c03a-stale-evidence-root "is not empty"

# ── N2: a filter that matches 0 tests must fail the gate (full run) ────────
reset_copy
sed -i '' 's| -- --test-threads=4| -- g07_no_such_filter_xyz --test-threads=4|' \
  "$COPY/scripts/rust-tauri/r03_g07_repair_suites.sh"
note "== N2: producer tampered to a 0-match test filter; full verify-stage R03 =="
gate_run n2-zero-match-filter "filter matched 0 tests" "cancel_link_inheritance"

# ── N3: a declared evidence file the producer never writes (full run) ──────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R03.json"
m = json.load(open(p))
m["commands"]["repair_suites"]["evidencePaths"].append(
    "{EVIDENCE}/G07_REPAIR/missing-evidence-demo.json")
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== N3: map declares an evidence file the producer never writes; full verify-stage R03 =="
gate_run n3-missing-evidence-file "missing-evidence-demo.json"

# ── C03b: an execution input changes MID-RUN → binding forces FAIL ─────────
reset_copy
note "== C03b: starting a full verify-stage R03, then changing an execution input mid-run =="
C3B_DIR="$EV/c03b-midrun-input-change"
mkdir -p "$C3B_DIR"
(cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" run --manifest-path rust/Cargo.toml \
   --locked -p xtask -- verify-stage R03 --evidence "$C3B_DIR/evidence" \
   > "$C3B_DIR/gate.stdout.log" 2> "$C3B_DIR/gate.stderr.log") &
GATE_PID=$!
# Wait until the repair producer is actually running (its first suite log
# appears), then mutate a tracked EXECUTION input inside the copy.
CHANGED=0
for _ in $(seq 1 7200); do
  if [ -f "$C3B_DIR/evidence/G07_REPAIR/cancel_link_inheritance.log" ]; then
    sleep 2
    printf '\n// G07/F08-C03 controlled STALE demo: execution input changed mid-run.\n' \
      >> "$COPY/rust/crates/lingxi-service/src/lib.rs"
    CHANGED=1
    break
  fi
  kill -0 "$GATE_PID" 2>/dev/null || break
  sleep 1
done
wait "$GATE_PID"
C3B_EXIT=$?
echo "$C3B_EXIT" > "$C3B_DIR/exit-code.txt"
echo "$CHANGED" > "$C3B_DIR/midrun-change-applied.txt"
C3B_RESULT="$C3B_DIR/evidence/verify-stage-result.json"
C3B_STABLE="$(python3 - "$C3B_RESULT" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
    print(str(d.get("candidateSourceBinding", {}).get("stable")).lower())
except Exception:
    print("unreadable")
PY
)"
C3B_NAMED=OK
grep -qF "Candidate file bytes or HEAD changed" "$C3B_RESULT" 2>/dev/null \
  || C3B_NAMED="MISSING:stable-reason"
[ "$C3B_STABLE" = "false" ] || C3B_NAMED="MISSING:stable=false(got:$C3B_STABLE)"
[ "$CHANGED" -eq 1 ] || C3B_NAMED="MISSING:midrun-window"
C3B_VERDICT=BAD; if [ "$C3B_EXIT" -ne 0 ] && [ "$C3B_NAMED" = "OK" ]; then C3B_VERDICT=OK; fi
record_case "c03b-midrun-input-change" "$C3B_EXIT" "$C3B_NAMED" "$C3B_VERDICT"

# ── verdict ─────────────────────────────────────────────────────────────────
python3 - "$EV" <<'PY' || fail "at least one negative case did NOT fail closed with the gap named"
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
rows = []
for line in (ev / "case-results.tsv").read_text().splitlines():
    if not line.strip():
        continue
    name, exit_code, named, verdict = line.split("\t")
    rows.append({"case": name, "exitCode": int(exit_code), "gapNamed": named,
                 "verdict": verdict})
doc = {
    "schema": "lingxi.r03-g07-gate-negative-tests.v1",
    "isolatedCopy": "git worktree at HEAD + uncommitted gate files (removed after the run)",
    "gate": "cargo run -p xtask -- verify-stage R03 (real binary, rebuilt inside the copy)",
    "cases": rows,
    "allRefused": all(r["verdict"] == "OK" for r in rows),
}
(ev / "case-results.json").write_text(json.dumps(doc, indent=1) + "\n")
print(json.dumps(doc["cases"], indent=1))
if not doc["allRefused"]:
    raise SystemExit(1)
PY
note "RESULT: every negative case failed closed with the gap named (no total PASS was granted)"
