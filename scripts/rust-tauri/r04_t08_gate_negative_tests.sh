#!/usr/bin/env bash
# R04-T08 — the R04 stage gate's NEGATIVE tests.
#
# Proves on an ISOLATED /tmp copy of the repo (never by tampering the
# production gate in place) that the R04 stage gate fails CLOSED with the
# gap NAMED for every fake-green shape the registration must not allow:
#
#   N-unknown an unknown stage id            → verify-stage non-zero,
#        "unknown stage" (a stage without a map is a hard error);
#   N-empty  an EMPTY scenario set           → verify-stage non-zero,
#        "registers an EMPTY scenario set" (never a vacuous pass);
#   N1a delete one supplemental-leaf mapping → verify-stage non-zero,
#        the dropped R00 leaf id is named (pre-run cross-check abort);
#   N1b delete the r04_tool_matrix command   → verify-stage non-zero,
#        "references unknown command" names it (parse refusal);
#   C03a a non-empty (stale) evidence root   → verify-stage refuses to
#        run, "is not empty" (old results can never mask a new run);
#   N4  delete the R04-SUP01 scenario        → the xtask map-pinning
#        tests (part of the gate's own rust_test_workspace command) go
#        red naming R04-SUP01;
#   N2  a test filter that matches 0 tests   → the registered producer
#        fails naming the empty case set, and the full verify-stage R04
#        gate exits non-zero (no overall PASS);
#   N3  a declared evidence file never written → the command itself is
#        green but verify-stage fails on missingEvidence naming the path;
#   C03b an execution input changes mid-run  → the gate's candidate
#        binding marks the run unstable (stable=false, reason recorded)
#        and FORCES overall FAIL — the earlier PASS cannot be reused.
#
# Usage: scripts/rust-tauri/r04_t08_gate_negative_tests.sh [EVIDENCE_DIR]
# Every case's real exit code and the gap-naming text are archived under
# EVIDENCE_DIR; the script exits 0 only if EVERY case exited non-zero AND
# the expected gap text was found.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
# The evidence root must be ABSOLUTE and must NOT cross a symlink (the
# gate's candidate binding refuses a --evidence path that crosses one —
# macOS /tmp → /private/tmp would be refused; keeping it inside the repo
# mirrors the R03 negative battery and stays binding-compatible).
EV_ARG="${1:-$ROOT/artifacts/rust-tauri/R04/T08-E01/negative-tests}"
case "$EV_ARG" in
  /*) EV="$EV_ARG" ;;
  *)  EV="$ROOT/$EV_ARG" ;;
esac
mkdir -p "$EV"
: > "$EV/summary.txt"
: > "$EV/case-results.tsv"

note() { printf '%s\n' "$*" | tee -a "$EV/summary.txt"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

CARGO="$HOME/.cargo/bin/cargo"
[ -x "$CARGO" ] || fail "~/.cargo/bin/cargo (rustup 1.98.1) not found"
export CARGO_NET_OFFLINE=true
# The copy's build target MUST live outside /private/tmp (and outside the
# OS $TMPDIR): the incumbent sandbox contract legitimately allows sandboxed
# writes there, so a CARGO_TARGET_TMPDIR under /tmp would make the T06
# A12 "restricted" sentinel writable by contract and the suite would red
# for an environment reason (observed on the first battery run — recorded
# in the T08 report §5 row 10a). $HOME scratch keeps it outside.
NEG_TARGET="${R04_NEG_CARGO_TARGET:-$HOME/.cache/lingxi-r04-t08-neg-target}"

# ── isolated copy: git worktree at HEAD + the uncommitted R04-T08 files ─────
COPY="$(mktemp -d /tmp/lingxi-r04-t08-neg-XXXXXX)"
note "== preparing isolated copy $COPY (git worktree at HEAD $(git -C "$ROOT" rev-parse --short HEAD) + uncommitted R04-T08 files) =="
if ! git -C "$ROOT" worktree add --detach "$COPY" HEAD > "$EV/worktree-add.log" 2>&1; then
  cat "$EV/worktree-add.log"; fail "worktree add failed"
fi
cleanup() { git -C "$ROOT" worktree remove --force "$COPY" >/dev/null 2>&1 || true; }
trap cleanup EXIT

for f in \
  rust/crates/xtask/src/main.rs \
  rust/crates/xtask/src/runner_identity.rs \
  rust/crates/xtask/src/stage_map.rs \
  rust/crates/xtask/src/stage_maps/R04.json \
  rust/crates/lingxi-service/src/artifactverify.rs \
  rust/crates/lingxi-service/src/filetools.rs \
  rust/crates/lingxi-service/src/lib.rs \
  rust/crates/lingxi-service/src/toolgateway.rs \
  rust/crates/lingxi-service/src/workerrpc.rs \
  rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs \
  rust/crates/lingxi-service/tests/r04_t04_file_tools.rs \
  rust/crates/lingxi-service/tests/r04_t07_mcp_and_workers.rs \
  rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs \
  scripts/rust-tauri/r04_t08_matrix.sh \
  scripts/rust-tauri/r04_t08_generate_stage_map.py \
  scripts/rust-tauri/r04_t08_gate_negative_tests.sh \
  scripts/rust-tauri/r04_rr1_g05_repair_suites.sh; do
  mkdir -p "$COPY/$(dirname "$f")"
  cp "$ROOT/$f" "$COPY/$f"
done
chmod +x "$COPY/scripts/rust-tauri/r04_t08_matrix.sh"
# The nested R03 regression reuses node_modules from the repo root it runs
# in; give the copy a clonefile (CoW) copy so the /tmp gate run is
# representative instead of failing on a missing dependency tree.
if [ -d "$ROOT/node_modules" ]; then
  note "== cloning node_modules into the copy (cp -Rc, CoW) =="
  cp -Rc "$ROOT/node_modules" "$COPY/node_modules" 2>/dev/null || cp -R "$ROOT/node_modules" "$COPY/node_modules"
fi

# Pristine copies for resetting between cases.
# RR1 G05 increment (2026-10-01): the overlay also carries the RR1 repair
# producer — the map now registers `r04_rr1_repair_suites` and the xtask
# pin tests read that script, so a copy without it would be a broken tree
# (the battery must tamper exactly ONE thing per case, never accidentally
# red on a missing registered file).
mkdir -p "$EV/pristine"
cp "$COPY/rust/crates/xtask/src/stage_maps/R04.json" "$EV/pristine/R04.json"
cp "$COPY/scripts/rust-tauri/r04_t08_matrix.sh" "$EV/pristine/r04_t08_matrix.sh"
cp "$COPY/scripts/rust-tauri/r04_rr1_g05_repair_suites.sh" "$EV/pristine/r04_rr1_g05_repair_suites.sh"
reset_copy() {
  cp "$EV/pristine/R04.json" "$COPY/rust/crates/xtask/src/stage_maps/R04.json"
  cp "$EV/pristine/r04_t08_matrix.sh" "$COPY/scripts/rust-tauri/r04_t08_matrix.sh"
  cp "$EV/pristine/r04_rr1_g05_repair_suites.sh" "$COPY/scripts/rust-tauri/r04_rr1_g05_repair_suites.sh"
}

record_case() { # name exit_code named verdict
  note "case $1: exit=$2 gap-named=$3 verdict=$4"
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$EV/case-results.tsv"
}

# gate_run NAME [pattern...] — runs the REAL verify-stage R04 inside the
# copy, records exit code + output, and requires: non-zero exit AND every
# pattern present somewhere in the run output or the written result JSON.
gate_run() {
  local name="$1"; shift
  local dir="$EV/$name"
  mkdir -p "$dir"
  (cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" run --manifest-path rust/Cargo.toml \
     --locked -p xtask -- verify-stage R04 --evidence "$dir/evidence" \
     > "$dir/gate.stdout.log" 2> "$dir/gate.stderr.log")
  local exit_code=$?
  echo "$exit_code" > "$dir/exit-code.txt"
  # The gap text can legitimately live in the gate's own logs, the written
  # result JSON, the tampered producer's command stdout/stderr, or the
  # producer's evidence files under the case evidence root.
  local haystack="$dir/gate.stdout.log $dir/gate.stderr.log"
  local extra
  for extra in \
    "$dir/evidence/verify-stage-result.json" \
    "$dir/evidence/r04_tool_matrix/stdout.log" \
    "$dir/evidence/r04_tool_matrix/stderr.log" \
    "$dir/evidence/R04_MATRIX/summary.txt" \
    "$dir/evidence/R04_MATRIX/integration.log"; do
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

# ── N-unknown: an unregistered stage id is a hard error ─────────────────────
reset_copy
U_DIR="$EV/n-unknown-stage"
mkdir -p "$U_DIR"
(cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" run --manifest-path rust/Cargo.toml \
   --locked -p xtask -- verify-stage R99 --evidence "$U_DIR/evidence" \
   > "$U_DIR/gate.stdout.log" 2> "$U_DIR/gate.stderr.log")
U_EXIT=$?
echo "$U_EXIT" > "$U_DIR/exit-code.txt"
U_NAMED=OK
grep -qF "unknown stage" "$U_DIR/gate.stderr.log" || U_NAMED="MISSING:unknown-stage"
U_VERDICT=BAD; if [ "$U_EXIT" -ne 0 ] && [ "$U_NAMED" = "OK" ]; then U_VERDICT=OK; fi
note "== N-unknown: verify-stage R99 (no such map registered) =="
record_case "n-unknown-stage" "$U_EXIT" "$U_NAMED" "$U_VERDICT"

# ── N-empty: an EMPTY scenario set is never a vacuous pass ──────────────────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
m["scenarios"] = []
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== N-empty: R04 map with an EMPTY scenario set =="
# parse_stage_map refuses the empty set first ("scenarios must not be
# EMPTY…"); main.rs keeps a second, same-meaning guard.
gate_run n-empty-scenario-set "must not be EMPTY"

# ── N1a: delete one supplemental-leaf mapping from the FULL real map ────────
reset_copy
LEAF_ID="$(python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
leaf = m["supplementalLeafScenarios"][0]
del m["supplementalLeafScenarios"][0]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
print(leaf["id"])
PY
)"
note "== N1a: deleted supplemental leaf mapping $LEAF_ID from the copy's map =="
gate_run n1a-delete-leaf-mapping "drops 1 REQUIRED_SUPPLEMENTAL leaf scenario(s)" "$LEAF_ID"

# ── N1b: delete the r04_tool_matrix command mapping (scenario keeps it) ─────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
del m["commands"]["r04_tool_matrix"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== N1b: deleted the r04_tool_matrix command mapping from the copy's map =="
gate_run n1b-delete-command-mapping "references unknown command" "r04_tool_matrix"

# ── C03a: a non-empty (stale) evidence root is refused outright ─────────────
reset_copy
mkdir -p "$EV/c03a-stale-evidence-root/evidence"
echo "stale evidence from an earlier run" > "$EV/c03a-stale-evidence-root/evidence/old.txt"
note "== C03a: evidence root pre-filled with a stale file =="
gate_run c03a-stale-evidence-root "is not empty"

# ── N4: delete the R04-SUP01 scenario → xtask map-pinning tests red ─────────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
m["scenarios"] = [s for s in m["scenarios"] if s["id"] != "R04-SUP01"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
N4_DIR="$EV/n4-delete-sup-scenario"
mkdir -p "$N4_DIR"
(cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" test --manifest-path rust/Cargo.toml \
   --locked -p xtask --bin xtask r04_ \
   > "$N4_DIR/test.stdout.log" 2>&1)
N4_EXIT=$?
echo "$N4_EXIT" > "$N4_DIR/exit-code.txt"
N4_NAMED=OK
grep -qF "R04-SUP01" "$N4_DIR/test.stdout.log" || N4_NAMED="MISSING:R04-SUP01"
N4_VERDICT=BAD; if [ "$N4_EXIT" -ne 0 ] && [ "$N4_NAMED" = "OK" ]; then N4_VERDICT=OK; fi
note "== N4: deleted the R04-SUP01 scenario from the copy's map; xtask map-pinning tests: =="
record_case "n4-delete-sup-scenario" "$N4_EXIT" "$N4_NAMED" "$N4_VERDICT"

# ── N2: a filter that matches 0 tests must fail the gate (full run) ─────────
reset_copy
sed -i '' 's| -- --test-threads=2| -- r04t08_no_such_filter_xyz --test-threads=2|' \
  "$COPY/scripts/rust-tauri/r04_t08_matrix.sh"
note "== N2: producer tampered to a 0-match test filter; full verify-stage R04 =="
gate_run n2-zero-match-filter "no case fragments were produced"

# ── N3: a declared evidence file the producer never writes (full run) ───────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
m["commands"]["r04_tool_matrix"]["evidencePaths"].append(
    "{EVIDENCE}/R04_MATRIX/missing-evidence-demo.json")
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== N3: map declares an evidence file the producer never writes; full verify-stage R04 =="
gate_run n3-missing-evidence-file "missing-evidence-demo.json"

# ── C03b: an execution input changes MID-RUN → binding forces FAIL ──────────
reset_copy
note "== C03b: starting a full verify-stage R04, then changing an execution input mid-run =="
C3B_DIR="$EV/c03b-midrun-input-change"
mkdir -p "$C3B_DIR"
(cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" run --manifest-path rust/Cargo.toml \
   --locked -p xtask -- verify-stage R04 --evidence "$C3B_DIR/evidence" \
   > "$C3B_DIR/gate.stdout.log" 2> "$C3B_DIR/gate.stderr.log") &
GATE_PID=$!
# Wait until the first command is actually producing evidence, then mutate
# a tracked EXECUTION input inside the copy.
CHANGED=0
for _ in $(seq 1 7200); do
  if [ -f "$C3B_DIR/evidence/rust_test_workspace/stdout.log" ]; then
    sleep 2
    printf '\n// R04-T08 C03 controlled STALE demo: execution input changed mid-run.\n' \
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
    "schema": "lingxi.r04-t08-gate-negative-tests.v1",
    "isolatedCopy": "git worktree at HEAD + uncommitted R04-T08 files (removed after the run)",
    "gate": "cargo run -p xtask -- verify-stage R04 (real binary, rebuilt inside the copy)",
    "cases": rows,
    "allRefused": all(r["verdict"] == "OK" for r in rows),
}
(ev / "case-results.json").write_text(json.dumps(doc, indent=1) + "\n")
print(json.dumps(doc["cases"], indent=1))
if not doc["allRefused"]:
    raise SystemExit(1)
PY
note "RESULT: every negative case failed closed with the gap named (no total PASS was granted)"
