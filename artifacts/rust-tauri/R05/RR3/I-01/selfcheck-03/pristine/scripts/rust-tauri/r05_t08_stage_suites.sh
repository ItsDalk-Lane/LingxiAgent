#!/usr/bin/env bash
# R05-T08 stage-suite producer — the registered producer behind the R05 stage
# map's `r05_stage_suites` command (scripts/rust-tauri/r05_t08_stage_suites.sh).
#
# Runs every R05 integration suite (27 suites across lingxi-service and
# lingxi-adapters after R05 RR1 — the original 18 plus the RR1 repair
# batteries: r05_t05_network, r05_t06_rr1_media_resource,
# r05_t06_rr1_system_speech, r05_t07_rr1_usage_ledger,
# r05_t08_production_tools, r05_t08_resources, r05_t03_rr1_replay,
# r05_t04_rr1_batch_terminal, r05_t07_rr1_usage_strict) plus the 64 pinned
# lib unit tests the acceptance ledger cites, through the REAL cargo test
# chain, and pins each run's executed test count EXACTLY. The gate observes
# this script's real exit code; the script refuses every fake-green shape
# on its own:
#   - a run whose filter matches 0 tests ("running 0 tests") is a GAP,
#     never a pass (cargo itself exits 0 there — the classic hole);
#   - a run that executed fewer/more tests than pinned (filtered subset,
#     renamed/deleted tests, duplicated runs) is a GAP;
#   - any failing/ignored test is a failure;
#   - every executed test must additionally be OWNED by exactly one R05
#     C-ID of the acceptance checklist (the cid TSV): a test that ran but
#     belongs to no case, or a pinned case whose test never ran green
#     (renamed/deleted/moved module), is a named GAP — 漏 ID fails closed;
#   - R05 RR1 F26: the cid table's C-ID set must equal EXACTLY the
#     `cid`-bound entries of the AUTHORITATIVE registry
#     (r05_required_cids.tsv — the 100 original C-IDs + the 3 appended).
#     A fabricated ID (R05-T99-C99), a renamed ID, or a dropped ID is a
#     GAP — the count-only hole is closed;
#   - R05 RR1 F25: exclusive-leaf cases are TEST-LEVEL (the leafcase line
#     names the test); actual=1 requires that test's `... ok` line, never
#     suite-level green alone.
#
# Outputs (declared evidence of the stage map — FRESH per run; the gate's
# freshness check enforces that):
#   <DIR>/r05-cases.json  — lingxi.r05-stage-suite-results.v1 (per-run
#     records + per-C-ID case records with expect/actual);
#   <DIR>/leaf-cases.json — lingxi.leaf-case-results.v1 (per-leaf cases);
#   <DIR>/f27-resource-series.json — the F27 raw resource time series
#     (copied from the path the resources test prints);
#   <DIR>/summary.txt     — per-run PASS lines + totals;
#   <DIR>/<run>.log       — raw cargo test stdout per run;
#   <DIR>/pin-table.txt / cid-table.txt / required-cids.txt — the exact
#     tables verified.
#
# The pin/cid/registry TSVs are mirrored by the xtask unit tests
# (stage_map.rs `r05_*` map-pinning tests) — deleting a mapping, lowering a
# pinned count, dropping a C-ID line, or editing the registry turns the
# workspace test suite (a gate command itself) red.
#
# Usage: scripts/rust-tauri/r05_t08_stage_suites.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R05/T08-E01/R05_SUITES}"
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"

TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r05-t08}"
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

# ── pin/cid/registry tables: the SINGLE source of truth lives in the repo ────
# docs/rust-tauri/R05/r05_stage_pins.tsv       (pin <run> <count> <tag>)
# docs/rust-tauri/R05/r05_stage_cids.tsv       (cid <C-ID> <run> <test>+…)
# docs/rust-tauri/R05/r05_required_cids.tsv    (reqcid <C-ID> <binding>)
# run keys: svc:<suite> / adp:<suite> = integration suites of that package;
# lib-<pkg>/<full::path> = one EXACT lib unit test. The same TSVs are
# machine-mirrored by the xtask stage_map tests — deleting a mapping,
# lowering a pinned count, dropping a C-ID line, or editing the registry
# turns the workspace test suite (a gate command itself) red. Re-registered
# 2026-10-05 by R05 RR1 F26 (27 suites = 362 integration tests + 64 lib
# pins; 93 cid-owned + 10 command-bound = the authoritative 103).
PIN_TABLE="docs/rust-tauri/R05/r05_stage_pins.tsv"
CID_TABLE="docs/rust-tauri/R05/r05_stage_cids.tsv"
REGISTRY="docs/rust-tauri/R05/r05_required_cids.tsv"
[ -f "$PIN_TABLE" ] || fail "pin table missing: $PIN_TABLE"
[ -f "$CID_TABLE" ] || fail "cid table missing: $CID_TABLE"
[ -f "$REGISTRY" ] || fail "required-cid registry missing: $REGISTRY"
PIN_LINES="$(grep -E '^pin ' "$PIN_TABLE")"
CID_LINES="$(grep -E '^cid ' "$CID_TABLE")"
REGISTRY_LINES="$(grep -E '^reqcid ' "$REGISTRY")"
[ -n "$PIN_LINES" ] || fail "pin table $PIN_TABLE has no pin lines"
[ -n "$CID_LINES" ] || fail "cid table $CID_TABLE has no cid lines"
[ -n "$REGISTRY_LINES" ] || fail "registry $REGISTRY has no reqcid lines"

printf '%s\n' "$PIN_LINES" > "$EVIDENCE_DIR/pin-table.txt"
printf '%s\n' "$CID_LINES" > "$EVIDENCE_DIR/cid-table.txt"
printf '%s\n' "$REGISTRY_LINES" > "$EVIDENCE_DIR/required-cids.txt"

# R05 RR1 F26 (pre-run): the authoritative identity check. The cid table's
# C-ID set must equal EXACTLY the registry's `cid`-bound set — a fabricated
# ID, a rename, or a dropped line fails HERE, before any test runs.
python3 - "$EVIDENCE_DIR" <<'PYREG' || fail "required-C-ID identity check failed"
import pathlib, sys
ev = pathlib.Path(sys.argv[1])
registry = {}
for line in (ev / "required-cids.txt").read_text().splitlines():
    fields = line.split()
    if len(fields) != 3:
        raise SystemExit(f"malformed registry line: {line!r}")
    _, cid, binding = fields
    if binding != "cid" and not binding.startswith("command:"):
        raise SystemExit(f"registry entry {cid} has an unknown binding {binding!r}")
    if cid in registry:
        raise SystemExit(f"duplicate registry entry {cid}")
    registry[cid] = binding
if len(registry) != 103:
    raise SystemExit(f"the registry must carry exactly 103 required C-IDs, got {len(registry)}")
owned = set()
for line in (ev / "cid-table.txt").read_text().splitlines():
    fields = line.split()
    if len(fields) < 4:
        raise SystemExit(f"malformed cid line: {line!r}")
    owned.add(fields[1])
expected = {cid for cid, binding in registry.items() if binding == "cid"}
if owned != expected:
    raise SystemExit(
        "the cid table does not match the authoritative registry — "
        f"fabricated/unregistered: {sorted(owned - expected)}; "
        f"missing: {sorted(expected - owned)}"
    )
print(f"registry identity OK: {len(owned)} cid-owned + "
      f"{sum(1 for b in registry.values() if b.startswith('command:'))} command-bound = "
      f"{len(registry)} required C-IDs")
PYREG

log_name() { printf '%s' "$1" | tr ':/' '__'; }

note "== building the R05 stage suites (rustup $TOOLCHAIN, $TARGET_DIR, --locked, offline) =="
env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
  rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
  -p lingxi-service -p lingxi-adapters \
  --tests \
  --no-run \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
note "PASS build (locked, offline)"

GAPS_FILE="$EVIDENCE_DIR/gaps.txt"
: > "$GAPS_FILE"
CASES_FILE="$EVIDENCE_DIR/cases.jsonl"
: > "$CASES_FILE"

while read -r _ run pinned tag; do
  [ -n "$run" ] || continue
  LOG="$EVIDENCE_DIR/$(log_name "$run").log"
  case "$run" in
    svc:*) SUITE="${run#svc:}"; RUN_ARGS=(-p lingxi-service --test "$SUITE" -- --test-threads=2)
           # R05 RR1 F27: the resources suite prints the raw-series path on
           # stdout (--nocapture lets it reach the run log the producer reads).
           [ "$run" = "svc:r05_t08_resources" ] && RUN_ARGS+=("--nocapture") ;;
    adp:*) SUITE="${run#adp:}"; RUN_ARGS=(-p lingxi-adapters --test "$SUITE" -- --test-threads=2) ;;
    lib-*) REST="${run#lib-}"; PKG_NAME="${REST%%/*}"; TEST_PATH="${REST#*/}"
           case "$PKG_NAME" in
             adapters) CRATE=lingxi-adapters ;;
             kernel)   CRATE=lingxi-kernel ;;
             service)  CRATE=lingxi-service ;;
             *) fail "unknown lib pin package: $run" ;;
           esac
           RUN_ARGS=(-p "$CRATE" --lib "$TEST_PATH" -- --exact) ;;
    *) fail "unknown pin run key: $run" ;;
  esac
  note "== running $run (pinned $pinned) =="
  set +e
  env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
    CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR \
    rustup run "$TOOLCHAIN" cargo test --manifest-path rust/Cargo.toml --locked --offline \
    "${RUN_ARGS[@]}" \
    > "$LOG" 2>&1
  RUN_EXIT=$?
  set -e
  RAN="$(sed -n 's/^running \([0-9][0-9]*\) test[s]*$/\1/p' "$LOG" | head -n 1)"
  PASSED="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$LOG" | head -n 1)"
  if [ -z "$PASSED" ]; then
    # libtest prints the failed summary in EITHER field order
    # ("1 failed; 0 passed" or "0 passed; 1 failed"); accept both.
    PASSED="$(sed -n 's/^test result: FAILED\. [0-9][0-9]* failed; \([0-9][0-9]*\) passed.*/\1/p' "$LOG" | head -n 1)"
  fi
  if [ -z "$PASSED" ]; then
    PASSED="$(sed -n 's/^test result: FAILED\. \([0-9][0-9]*\) passed; [0-9][0-9]* failed.*/\1/p' "$LOG" | head -n 1)"
  fi
  FAILED="$(sed -n 's/^test result: FAILED\. \([0-9][0-9]*\) failed.*/\1/p' "$LOG" | head -n 1)"
  if [ -z "$FAILED" ]; then
    FAILED="$(sed -n 's/^test result: FAILED\. [0-9][0-9]* passed; \([0-9][0-9]*\) failed.*/\1/p' "$LOG" | head -n 1)"
  fi
  if [ "$RUN_EXIT" -ne 0 ]; then
    echo "run $run: cargo test exit $RUN_EXIT" >> "$GAPS_FILE"
  fi
  if [ -z "$RAN" ] || [ -z "$PASSED" ]; then
    echo "run $run: no parseable libtest summary (running='${RAN:-}' passed='${PASSED:-}') — the filter matched nothing or the harness output drifted" >> "$GAPS_FILE"
  else
    if [ "$RAN" -eq 0 ]; then
      echo "run $run: filter matched 0 tests (running=0, pinned=$pinned) — empty test collections must not pass" >> "$GAPS_FILE"
    fi
    if [ "$RAN" -ne "$pinned" ]; then
      echo "run $run: executed $RAN tests but the stage pin is $pinned (filtered subset / renamed or deleted tests)" >> "$GAPS_FILE"
    fi
    if [ "$PASSED" -ne "$pinned" ]; then
      echo "run $run: passed $PASSED but the stage pin is $pinned" >> "$GAPS_FILE"
    fi
  fi
  if [ -n "$FAILED" ] && [ "$FAILED" -ne 0 ]; then
    echo "run $run: $FAILED failing tests" >> "$GAPS_FILE"
  fi
  ACTUAL="${PASSED:-0}"
  OK="false"
  if [ -z "$(grep "^run $run:" "$GAPS_FILE")" ]; then OK="true"; fi
  python3 - "$CASES_FILE" "$run" "$tag" "$pinned" "$ACTUAL" "$OK" <<'PY'
import json, sys
path, run, tag, expect, actual, ok = sys.argv[1:7]
record = {"run": run, "issue": tag, "expect": int(expect),
          "actual": int(actual), "ok": ok == "true"}
with open(path, "a", encoding="utf-8") as fh:
    fh.write(json.dumps(record, ensure_ascii=False) + "\n")
PY
  if [ "$OK" = "true" ]; then
    note "PASS $run: $PASSED/$pinned tests green"
  else
    note "FAIL $run: see gaps.txt"
  fi
done <<EOF
$PIN_LINES
EOF

# Assemble the machine-consumable case file and verify per-C-ID test-name
# ownership against the real run logs.
python3 - "$EVIDENCE_DIR" << 'PYEOF' || fail "case assembly failed"
import json, pathlib, re, sys
ev = pathlib.Path(sys.argv[1])
records = [json.loads(line) for line in (ev / "cases.jsonl").read_text().splitlines() if line.strip()]
if not records:
    raise SystemExit("no per-run records were produced")
pins = {}
for line in (ev / "pin-table.txt").read_text().splitlines():
    _, run, count, tag = line.split()
    pins[run] = (int(count), tag)

def log_of(run):
    return ev / (run.replace(":", "_").replace("/", "_") + ".log")

cid_lines = [line.split() for line in (ev / "cid-table.txt").read_text().splitlines() if line.strip()]
gaps = []
cases = {}
ownership = {}
for fields in cid_lines:
    if len(fields) != 4:
        gaps.append(f"malformed cid line: {' '.join(fields)!r}")
        continue
    _, cid, run, names_field = fields
    if run not in pins:
        gaps.append(f"cid {cid}: run {run!r} is not in the pin table")
        continue
    if not cid.startswith("R05-T"):
        gaps.append(f"cid {cid}: not an R05 case id")
    log = log_of(run).read_text() if log_of(run).exists() else ""
    for name in names_field.split("+"):
        key = (run, name)
        if key in ownership:
            gaps.append(f"test {name!r} in run {run!r} is claimed by both "
                        f"{ownership[key]} and {cid}")
            continue
        ownership[key] = cid
        ok_line = re.search(r"^test " + re.escape(name) + r" \.\.\. ok$", log, re.M)
        entry = cases.setdefault(cid, {"expect": 0, "actual": 0})
        entry["expect"] += 1
        if ok_line:
            entry["actual"] += 1
        else:
            gaps.append(f"cid {cid}: test {name!r} has no `... ok` line in {log_of(run).name} "
                        f"(renamed, deleted, moved module, or not green)")

# Every executed test of every PINNED run must be owned: for lib pins the
# executed set is the single pinned test; for suite runs it is the whole
# suite (the pin count equals the suite size by construction).
for run, (count, _tag) in pins.items():
    owned = sum(1 for (r, _n) in ownership if r == run)
    if owned != count:
        gaps.append(f"run {run!r} pins {count} executed tests but the cid table owns {owned} "
                    f"— every executed test must belong to exactly one C-ID")
if not cases:
    gaps.append("no C-ID case records were produced from the cid table")

case_records = []
for cid in sorted(cases):
    entry = cases[cid]
    case_records.append({"case": cid, "expect": entry["expect"],
                         "actual": entry["actual"], "ok": entry["expect"] == entry["actual"]})
doc = {
    "schema": "lingxi.r05-stage-suite-results.v1",
    "producedBy": "scripts/rust-tauri/r05_t08_stage_suites.sh "
                  "(cargo test, real chain: service+adapters integration + pinned lib tests)",
    "suites": records,
    "cases": case_records,
    "allSuitesOk": all(r["ok"] for r in records),
    "allCasesOk": all(c["ok"] for c in case_records) if case_records else False,
}
(ev / "r05-cases.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
print(f"assembled {len(records)} run records / {len(case_records)} C-ID case records")
if gaps:
    (ev / "cid-gaps.txt").write_text("\n".join(gaps) + "\n")
    raise SystemExit("cid ownership gaps:\n" + "\n".join(gaps))

# R00 supplemental leaves bound to R05 (130 leaves / 137 cases after R05 RR1
# F25): share leaves pin ONE suite-level case (green iff the mapped run
# executed its exact pin green); R05-EXCLUSIVE leaves (the six OAuth leaves)
# pin one TEST-LEVEL case per original R00 assertion — the leafcase line
# names the test, and actual=1 requires THAT test's `... ok` line in the
# run's log plus the run's own green (never suite-green alone).
# Schema: lingxi.leaf-case-results.v1 (what the stage map's assertion
# contracts consume).
LEAF_MAP = pathlib.Path("docs/rust-tauri/R05/r05_leaf_case_map.tsv")
leaf_lines = [l.split() for l in LEAF_MAP.read_text().splitlines() if l.startswith("leafcase ")]
by_run_ok = {r["run"]: r["ok"] and r["actual"] == r["expect"] for r in records}
leaf_records = []
for fields in leaf_lines:
    if len(fields) == 3:
        _, case, run = fields
        if run not in by_run_ok:
            gaps.append(f"leaf case {case}: run {run!r} is not in the pin table")
            continue
        green = bool(by_run_ok[run])
        leaf_records.append({"case": case, "expect": 1,
                             "actual": 1 if green else 0, "ok": green})
    elif len(fields) == 4:
        _, case, run, test = fields
        if run not in by_run_ok:
            gaps.append(f"leaf case {case}: run {run!r} is not in the pin table")
            continue
        log = log_of(run)
        log_text = log.read_text() if log.exists() else ""
        import re as _re
        test_ok = _re.search(r"^test " + _re.escape(test) + r" \.\.\. ok$", log_text, _re.M)
        if not test_ok:
            gaps.append(f"leaf case {case}: test {test!r} has no `... ok` line in "
                        f"{log.name} (renamed, deleted, moved module, or not green)")
        green = bool(by_run_ok[run]) and bool(test_ok)
        leaf_records.append({"case": case, "expect": 1,
                             "actual": 1 if green else 0, "ok": green,
                             "evidence": f"{run}:{test}"})
    else:
        gaps.append(f"malformed leafcase line: {' '.join(fields)!r}")
leaf_doc = {
    "schema": "lingxi.leaf-case-results.v1",
    "producedBy": "scripts/rust-tauri/r05_t08_stage_suites.sh "
                  "(share leaves = mapped suite's exact-pin green run; "
                  "exclusive leaves = the NAMED test's ok line in that run)",
    "cases": leaf_records,
}
(ev / "leaf-cases.json").write_text(json.dumps(leaf_doc, indent=1, ensure_ascii=False) + "\n")
if len(leaf_records) != 137:
    gaps.append(f"expected 137 R00 R05-bound leaf cases (124 share + 13 test-level), "
                f"produced {len(leaf_records)}")
# R05 RR1 F25: the leaf-section gaps are FATAL too (a test-level case whose
# named test has no ok line, a run not in the pin table, a malformed line,
# or a drifted count must fail the producer — never a silent actual=0).
if gaps:
    (ev / "assembly-gaps.txt").write_text("\n".join(gaps) + "\n")
    raise SystemExit("case-assembly gaps:\n" + "\n".join(gaps))
PYEOF
note "PASS case files assembled (r05-cases.json + leaf-cases.json; every executed test owned by exactly one C-ID)"

if grep -q . "$GAPS_FILE"; then
  note "GAPS (each must be named, none may pass silently):"
  sed 's/^/  /' "$GAPS_FILE" | tee -a "$EVIDENCE_DIR/summary.txt"
  fail "R05 stage-suite coverage has gaps ($(wc -l < "$GAPS_FILE" | tr -d ' ') lines)"
fi
note "RESULT: all R05 stage runs green with exact pinned counts (27 suites + 64 lib pins; 93 cid-owned + 10 command-bound = 103 required C-IDs; 130 leaves / 137 leaf cases)"

# R05 RR1 F27: preserve the RAW resource time series — the resources suite
# prints the path of the series JSON it wrote into its own temp root; copy
# that file into THIS run's evidence directory (the Rust side never writes
# outside its temp root).
SERIES_SRC="$(sed -n 's/^F27 raw resource series: //p' "$EVIDENCE_DIR/$(log_name svc:r05_t08_resources).log" | tail -n 1)"
if [ -n "$SERIES_SRC" ] && [ -f "$SERIES_SRC" ]; then
  cp "$SERIES_SRC" "$EVIDENCE_DIR/f27-resource-series.json" \
    || fail "cannot copy the F27 raw resource series into the evidence dir"
  note "PASS F27 raw resource series preserved ($EVIDENCE_DIR/f27-resource-series.json)"
else
  fail "the F27 raw resource series was not produced or found (src='${SERIES_SRC:-none}') — raw samples are mandatory evidence, never optional"
fi
