#!/usr/bin/env bash
# R03 repair round G07 / F08-C02 — adversarial-repair suite producer.
#
# Runs the ten integration suites added by repair workorders G01–G06
# (F01–F07) plus the RR2 fixed repair (R03-RR2-F05-01) through the REAL
# service chain and pins each suite's executed test count EXACTLY. This is the registered producer behind the R03 stage
# map's `repair_suites` command / `R03-RP01` scenario: the gate observes
# this script's real exit code, and this script refuses every fake-green
# shape on its own:
#   - a suite whose filter matches 0 tests ("running 0 tests") is a GAP,
#     never a pass (cargo itself exits 0 there — the classic hole);
#   - a suite that ran fewer/more tests than pinned (filtered subset,
#     renamed/deleted tests, duplicated suites) is a GAP;
#   - any failing/ignored test is a failure.
#
# Outputs (declared evidence of the stage map — must be FRESH per run,
# the gate's F04 freshness check enforces that):
#   <DIR>/repair-cases.json — lingxi.r03-repair-suite-results.v1, one
#     machine record per pinned suite (expect == pinned count, actual ==
#     observed passed count);
#   <DIR>/summary.txt       — per-suite PASS lines + totals;
#   <DIR>/<suite>.log       — the raw cargo test stdout per suite.
#
# The pin table below is mirrored by the xtask unit test
# (stage_map.rs `r03_...` map-pinning tests) — deleting a mapping here,
# in the stage map, or lowering a pinned count turns the workspace test
# suite (a gate command itself) red.
#
# Usage: scripts/rust-tauri/r03_g07_repair_suites.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R03/repair-current/G07-E01/repair-suites}"
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"

TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r03-g07-repair}"
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

# ── pin table: suite <pinned-count> <F-ID> ───────────────────────────────────
# pin <suite> <count> <F-ID>   (machine-checked by xtask stage_map tests)
# RR2 increment (R03-RR2-F05-01, 2026-09-30): added the tenth suite
# `request_id_canonicalization` (F-ID RR2-F05, count 7) — the canonical
# requestId chain cases C01-C05 of the RR2 fixed repair. No existing suite
# or count was removed or lowered.
PIN_LINES="
pin cancel_link_inheritance 7 F01
pin subagent_closeout 8 F02
pin cancel_terminal_race 13 F03
pin tool_receipt_unknown 6 F04
pin admission_dedup_consistency 5 F05
pin admission_dedup_adversarial 5 F05
pin input_payload_fidelity 5 F06
pin input_budget_refusal 2 F06
pin background_steering 8 F07
pin request_id_canonicalization 7 RR2-F05
"

note "== building the ten repair suites (rustup $TOOLCHAIN, $TARGET_DIR, --locked, offline) =="
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
  rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-service \
  --test cancel_link_inheritance \
  --test subagent_closeout \
  --test cancel_terminal_race \
  --test tool_receipt_unknown \
  --test admission_dedup_consistency \
  --test admission_dedup_adversarial \
  --test input_payload_fidelity \
  --test input_budget_refusal \
  --test background_steering \
  --test request_id_canonicalization \
  --no-run \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
note "PASS build (locked, offline)"

GAPS_FILE="$EVIDENCE_DIR/gaps.txt"
: > "$GAPS_FILE"
CASES_FILE="$EVIDENCE_DIR/cases.jsonl"
: > "$CASES_FILE"

while read -r _ suite pinned fid; do
  [ -n "$suite" ] || continue
  LOG="$EVIDENCE_DIR/$suite.log"
  note "== running repair suite $suite (F-ID $fid, pinned $pinned) =="
  set +e
  env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
    CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
    rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
    -p lingxi-service --test "$suite" -- --test-threads=4 \
    > "$LOG" 2>&1
  RUN_EXIT=$?
  set -e
  RAN="$(sed -n 's/^running \([0-9][0-9]*\) tests$/\1/p' "$LOG" | head -n 1)"
  PASSED="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$LOG" | head -n 1)"
  if [ -z "$PASSED" ]; then
    PASSED="$(sed -n 's/^test result: FAILED\. [0-9][0-9]* failed; \([0-9][0-9]*\) passed.*/\1/p' "$LOG" | head -n 1)"
  fi
  FAILED="$(sed -n 's/^test result: FAILED\. \([0-9][0-9]*\) failed.*/\1/p' "$LOG" | head -n 1)"
  if [ "$RUN_EXIT" -ne 0 ]; then
    echo "suite $suite: cargo test exit $RUN_EXIT" >> "$GAPS_FILE"
  fi
  if [ -z "$RAN" ] || [ -z "$PASSED" ]; then
    echo "suite $suite: no parseable libtest summary (running='${RAN:-}' passed='${PASSED:-}') — the filter matched nothing or the harness output drifted" >> "$GAPS_FILE"
  else
    if [ "$RAN" -eq 0 ]; then
      echo "suite $suite: filter matched 0 tests (running=0, pinned=$pinned) — empty test collections must not pass" >> "$GAPS_FILE"
    fi
    if [ "$RAN" -ne "$pinned" ]; then
      echo "suite $suite: executed $RAN tests but the stage pin is $pinned (filtered subset / renamed or deleted tests)" >> "$GAPS_FILE"
    fi
    if [ "$PASSED" -ne "$pinned" ]; then
      echo "suite $suite: passed $PASSED but the stage pin is $pinned" >> "$GAPS_FILE"
    fi
  fi
  if [ -n "$FAILED" ] && [ "$FAILED" -ne 0 ]; then
    echo "suite $suite: $FAILED failing tests" >> "$GAPS_FILE"
  fi
  ACTUAL="${PASSED:-0}"
  OK="false"
  if [ -z "$(grep "^suite $suite:" "$GAPS_FILE")" ]; then OK="true"; fi
  printf '{"suite":"%s","issue":"%s","expect":%s,"actual":%s,"ok":%s}\n' \
    "$suite" "$fid" "$pinned" "$ACTUAL" "$OK" >> "$CASES_FILE"
  if [ "$OK" = "true" ]; then
    note "PASS $suite ($fid): $PASSED/$pinned tests green"
  else
    note "FAIL $suite ($fid): see gaps.txt"
  fi
done <<EOF
$PIN_LINES
EOF

# Assemble the machine-consumable case file from the per-suite records.
python3 - "$EVIDENCE_DIR" << 'PYEOF' || fail "case assembly failed"
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
records = [json.loads(line) for line in (ev / "cases.jsonl").read_text().splitlines() if line.strip()]
if not records:
    raise SystemExit("no per-suite records were produced")
doc = {
    "schema": "lingxi.r03-repair-suite-results.v1",
    "producedBy": "scripts/rust-tauri/r03_g07_repair_suites.sh (cargo test -p lingxi-service --test <suite>, real chain)",
    "suites": [
        {"suite": r["suite"], "issue": r["issue"], "expect": r["expect"],
         "actual": r["actual"], "ok": r["ok"]}
        for r in records
    ],
    "allSuitesOk": all(r["ok"] for r in records),
}
(ev / "repair-cases.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
print(f"assembled {len(records)} repair-suite records")
PYEOF

if grep -q . "$GAPS_FILE"; then
  note "GAPS (each must be named, none may pass silently):"
  sed 's/^/  /' "$GAPS_FILE" | tee -a "$EVIDENCE_DIR/summary.txt"
  fail "repair-suite coverage has gaps ($(wc -l < "$GAPS_FILE" | tr -d ' ') lines)"
fi
note "RESULT: all ten repair suites green with exact pinned counts (G01-G06 nine + RR2-F05-01 one)"
