#!/usr/bin/env bash
# R04 RR1 repair round G05 (CLOSE-C01 adversarial) — the RR1 registration's
# NEGATIVE tests.
#
# Proves on an ISOLATED /tmp copy of the repo (never by tampering the
# production gate in place) that the newly registered RR1 scenarios are not
# an empty shell — every fake-green shape fails CLOSED with the gap NAMED:
#
#   RR1-N1 delete one R04-RR1-F03 scenario from the map
#        → the xtask map-pinning tests (part of the gate's own
#          rust_test_workspace command) go red naming R04-RR1-F03;
#   RR1-N2 tamper a pinned case count in the producer's pin table
#        (9 → 8) → the xtask pin-table mirror test goes red naming the
#        drift (a lowered pin could otherwise green a filtered subset);
#   RR1-N3 drop a `cid` C-ID ownership line from the producer
#        → the xtask C-ID coverage test goes red naming R04-RR1-F05-C05
#        (a case the gate no longer owns must never pass silently);
#   RR1-N4 the same scenario deletion, judged by the REAL full gate:
#        verify-stage R04 exits non-zero (rust_test_workspace FAIL → the
#          scenario and overall verdict can never be PASS).
#
# Usage: scripts/rust-tauri/r04_rr1_g05_negative_tests.sh [EVIDENCE_DIR]
# Every case's real exit code and the gap-naming text are archived under
# EVIDENCE_DIR; the script exits 0 only if EVERY case exited non-zero AND
# the expected gap text was found.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
EV_ARG="${1:-$ROOT/artifacts/rust-tauri/R04/RR1-G05-E01/negative}"
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
NEG_TARGET="${R04_NEG_CARGO_TARGET:-$HOME/.cache/lingxi-r04-rr1-neg-target}"

# ── isolated copy: git worktree at HEAD + the uncommitted G05-E01 files ─────
COPY="$(mktemp -d /tmp/lingxi-r04-rr1-neg-XXXXXX)"
note "== preparing isolated copy $COPY (git worktree at HEAD $(git -C "$ROOT" rev-parse --short HEAD) + uncommitted G05-E01 files) =="
if ! git -C "$ROOT" worktree add --detach "$COPY" HEAD > "$EV/worktree-add.log" 2>&1; then
  cat "$EV/worktree-add.log"; fail "worktree add failed"
fi
cleanup() { git -C "$ROOT" worktree remove --force "$COPY" >/dev/null 2>&1 || true; }
trap cleanup EXIT

for f in \
  rust/crates/xtask/src/stage_map.rs \
  rust/crates/xtask/src/stage_maps/R04.json \
  scripts/rust-tauri/r04_rr1_g05_repair_suites.sh; do
  mkdir -p "$COPY/$(dirname "$f")"
  cp "$ROOT/$f" "$COPY/$f"
done
chmod +x "$COPY/scripts/rust-tauri/r04_rr1_g05_repair_suites.sh"

# Pristine copies for resetting between cases.
mkdir -p "$EV/pristine"
cp "$COPY/rust/crates/xtask/src/stage_maps/R04.json" "$EV/pristine/R04.json"
cp "$COPY/scripts/rust-tauri/r04_rr1_g05_repair_suites.sh" "$EV/pristine/r04_rr1_g05_repair_suites.sh"
reset_copy() {
  cp "$EV/pristine/R04.json" "$COPY/rust/crates/xtask/src/stage_maps/R04.json"
  cp "$EV/pristine/r04_rr1_g05_repair_suites.sh" "$COPY/scripts/rust-tauri/r04_rr1_g05_repair_suites.sh"
}

record_case() { # name exit_code named verdict
  note "case $1: exit=$2 gap-named=$3 verdict=$4"
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$EV/case-results.tsv"
}

xtask_run() { # name filter [pattern...]
  local name="$1"; shift
  local filter="$1"; shift
  local dir="$EV/$name"
  mkdir -p "$dir"
  (cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" test --manifest-path rust/Cargo.toml \
     --locked -p xtask --bin xtask "$filter" \
     > "$dir/test.stdout.log" 2>&1)
  local exit_code=$?
  echo "$exit_code" > "$dir/exit-code.txt"
  local named=OK
  local pattern
  for pattern in "$@"; do
    if ! grep -qF -- "$pattern" "$dir/test.stdout.log"; then named="MISSING:$pattern"; break; fi
  done
  local verdict=BAD
  if [ "$exit_code" -ne 0 ] && [ "$named" = "OK" ]; then verdict=OK; fi
  record_case "$name" "$exit_code" "$named" "$verdict"
}

# ── RR1-N1: delete the R04-RR1-F03 scenario → xtask pin tests red ───────────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
m["scenarios"] = [s for s in m["scenarios"] if s["id"] != "R04-RR1-F03"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== RR1-N1: deleted the R04-RR1-F03 scenario from the copy's map; xtask map-pinning tests: =="
xtask_run rr1-n1-delete-rr1-scenario r04_ "R04-RR1-F03"

# ── RR1-N2: tamper a pinned count (9 → 8) → pin-table mirror test red ───────
reset_copy
sed -i '' 's/^pin r04_t05_registry_capacity 9 RR1-F01$/pin r04_t05_registry_capacity 8 RR1-F01/' \
  "$COPY/scripts/rust-tauri/r04_rr1_g05_repair_suites.sh"
note "== RR1-N2: producer pin count tampered 9 -> 8 in the copy; xtask pin-table mirror test: =="
xtask_run rr1-n2-tamper-pin-count r04_rr1_producer_pin_table \
  "pin table drifted" "r04_t05_registry_capacity"

# ── RR1-N3: drop the R04-RR1-F05-C05 cid line → C-ID coverage test red ──────
reset_copy
sed -i '' '/^cid R04-RR1-F05-C05 /d' "$COPY/scripts/rust-tauri/r04_rr1_g05_repair_suites.sh"
note "== RR1-N3: dropped the R04-RR1-F05-C05 cid ownership line in the copy; xtask C-ID coverage test: =="
xtask_run rr1-n3-drop-cid-line r04_rr1_producer_case_table \
  "R04-RR1-F05-C05" "cid table drifted"

# ── RR1-N4: the same scenario deletion judged by the REAL full gate ─────────
reset_copy
python3 - "$COPY" <<'PY'
import json, sys
p = sys.argv[1] + "/rust/crates/xtask/src/stage_maps/R04.json"
m = json.load(open(p))
m["scenarios"] = [s for s in m["scenarios"] if s["id"] != "R04-RR1-F03"]
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
note "== RR1-N4: deleted the R04-RR1-F03 scenario; REAL full verify-stage R04 in the copy =="
N4_DIR="$EV/rr1-n4-full-gate"
mkdir -p "$N4_DIR"
(cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" run --manifest-path rust/Cargo.toml \
   --locked -p xtask -- verify-stage R04 --evidence "$N4_DIR/evidence" \
   > "$N4_DIR/gate.stdout.log" 2> "$N4_DIR/gate.stderr.log")
N4_EXIT=$?
echo "$N4_EXIT" > "$N4_DIR/exit-code.txt"
N4_RESULT="$N4_DIR/evidence/verify-stage-result.json"
N4_NAMED=OK
python3 - "$N4_RESULT" "$N4_DIR/gate.stdout.log" "$N4_DIR/gate.stderr.log" \
  "$N4_DIR/evidence/rust_test_workspace/stdout.log" <<'PY' > "$N4_DIR/named-check.txt"
import json, sys
found_scenario_fail = False
found_workspace_fail = False
try:
    d = json.load(open(sys.argv[1]))
    for s in d.get("scenarios", []):
        if s.get("id") == "R04-RR1-F03":
            # The scenario is DELETED from the tampered map; what must be
            # red instead is the rust_test_workspace command (the xtask
            # pin tests naming R04-RR1-F03) and the overall verdict.
            found_scenario_fail = True
    for c in d.get("commands", []):
        if c.get("key") == "rust_test_workspace" and c.get("status") == "FAIL":
            found_workspace_fail = True
    print("overall=" + str(d.get("overall")))
    print("rust_test_workspace_FAIL=" + str(found_workspace_fail))
    print("deleted_scenario_absent=" + str(not found_scenario_fail))
except Exception as exc:
    print("result-unreadable:", exc)
haystack = ""
for path in sys.argv[2:]:
    try:
        haystack += open(path, errors="replace").read()
    except OSError:
        pass
print("gap_text_in_gate_logs=" + str("R04-RR1-F03" in haystack))
PY
grep -qF "overall=FAIL" "$N4_DIR/named-check.txt" || N4_NAMED="MISSING:overall=FAIL"
grep -qF "rust_test_workspace_FAIL=True" "$N4_DIR/named-check.txt" || N4_NAMED="MISSING:rust_test_workspace_FAIL"
grep -qF "gap_text_in_gate_logs=True" "$N4_DIR/named-check.txt" || N4_NAMED="MISSING:R04-RR1-F03-named"
N4_VERDICT=BAD; if [ "$N4_EXIT" -ne 0 ] && [ "$N4_NAMED" = "OK" ]; then N4_VERDICT=OK; fi
record_case "rr1-n4-full-gate" "$N4_EXIT" "$N4_NAMED" "$N4_VERDICT"

# ── verdict ─────────────────────────────────────────────────────────────────
python3 - "$EV" <<'PY' || fail "at least one RR1 negative case did NOT fail closed with the gap named"
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
    "schema": "lingxi.r04-rr1-g05-gate-negative-tests.v1",
    "isolatedCopy": "git worktree at HEAD + uncommitted G05-E01 files (removed after the run)",
    "gate": "cargo test -p xtask (RR1-N1..N3) + real verify-stage R04 (RR1-N4)",
    "cases": rows,
    "allRefused": all(r["verdict"] == "OK" for r in rows),
}
(ev / "case-results.json").write_text(json.dumps(doc, indent=1) + "\n")
print(json.dumps(doc["cases"], indent=1))
if not doc["allRefused"]:
    raise SystemExit(1)
PY
note "RESULT: every RR1 negative case failed closed with the gap named (the new registration is not an empty shell)"
