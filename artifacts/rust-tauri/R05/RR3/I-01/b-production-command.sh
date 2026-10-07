set -uo pipefail
COPY="$1"; PRISTINE="$2"
fail() { echo "FAIL: $*" >&2; exit 1; }
snapshot_pristine() {
  # 快照只拍一次，不能把变异或漂移重新当作合法基线。
  [ ! -e "$PRISTINE/$1" ] || fail "pristine snapshot already exists: $1"
  mkdir -p "$PRISTINE/$(dirname "$1")" || fail "pristine directory failed: $1"
  cp "$COPY/$1" "$PRISTINE/$1" || fail "pristine copy failed: $1"
  cmp "$COPY/$1" "$PRISTINE/$1" || fail "pristine bytes differ: $1"
}
# 快照和恢复消费同一份清单，避免新增变异目标只登记到一侧。
MUTATED_FILES=(
  rust/crates/xtask/src/stage_maps/R05.json
  rust/crates/xtask/src/stage_maps/R04.json
  rust/crates/lingxi-service/src/lib.rs
  rust/crates/lingxi-kernel/src/lib.rs
  rust/crates/lingxi-adapters/src/models/tool_render.rs
  rust/crates/lingxi-adapters/src/models/openai_completions.rs
  rust/crates/lingxi-adapters/src/models/credentials.rs
  rust/crates/lingxi-service/src/runs.rs
  rust/crates/lingxi-service/src/credentials/mod.rs
  rust/crates/lingxi-service/tests/r05_t01_binary_wiring.rs
  docs/rust-tauri/R05/r05_stage_pins.tsv
  scripts/rust-tauri/r05_t08_stage_suites.sh
)
reset_copy() {
  local f
  for f in "${MUTATED_FILES[@]}"; do
    cp "$PRISTINE/$f" "$COPY/$f" || fail "restore copy failed: $f"
    cmp "$PRISTINE/$f" "$COPY/$f" || fail "restore bytes differ: $f"
  done
}

n06_start_signal() {
  # 使用现行场景首命令的权威 argv，匹配 verify.rs 已有的启动输出。
  python3 - "$COPY/rust/crates/xtask/src/stage_maps/R02.json" "$1" <<'PY'
import json, pathlib, sys
stage = json.loads(pathlib.Path(sys.argv[1]).read_text())
key = stage["scenarios"][0]["commandRefs"][0]
argv = stage["commands"][key]["argv"]
print(f'xtask: verify-stage {stage["stage"]} [{" ".join(argv)}] > {pathlib.Path(sys.argv[2]) / key}')
PY
}

wait_for_n06_start() {
  local pid="$1" log="$2" signal="$3" limit="${4:-240}" matched i
  for ((i=0; i<limit; i++)); do
    n06_gate_running "$pid" || return 1
    grep -Fx -- "$signal" "$log" >/dev/null
    matched=$?
    if [ "$matched" -eq 0 ]; then
      n06_gate_running "$pid" || return 1
      return 0
    fi
    [ "$matched" -eq 1 ] || { echo "N06 prerequisite failed: start log query failed" >&2; return 1; }
    sleep 1
  done
  echo "N06 prerequisite failed: authoritative command start not observed" >&2
  return 1
}

n06_gate_running() {
  # 未回收的已退出进程仍可能响应 kill -0，因此同时拒绝僵尸状态。
  local state status
  state="$(ps -p "$1" -o stat=)"
  status=$?
  if [ "$status" -gt 1 ]; then
    echo "N06 prerequisite failed: process query failed" >&2
    return 1
  fi
  case "$state" in
    ""|*Z*) echo "N06 prerequisite failed: gate exited" >&2; return 1 ;;
  esac
  [ "$status" -eq 0 ] && kill -0 "$1" 2>/dev/null || { echo "N06 prerequisite failed: gate exited" >&2; return 1; }
}

EV="$3"; CARGO="$4"; CASE_SCOPE=N03
note() { printf "%s\n" "$*" | tee -a "$EV/summary.txt"; }
record_case() { # name exit_code named verdict note
  note "case $1: exit=$2 gap-named=$3 verdict=$4 ($5)"
  printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" >> "$EV/case-results.tsv"
}

cargo_in_copy() { # args...
  (cd "$COPY" && "$CARGO" "$@" )
}
xtask_test() { # name filter [pattern...]
  local name="$1"; shift
  local filter="$1"; shift
  local dir="$EV/$name"
  mkdir -p "$dir"
  cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask "$filter" \
    > "$dir/test.stdout.log" 2>&1
  local exit_code=$?
  echo "$exit_code" > "$dir/exit-code.txt"
  local named=OK
  local pattern
  for pattern in "$@"; do
    if ! grep -qF -- "$pattern" "$dir/test.stdout.log"; then named="MISSING:$pattern"; break; fi
  done
  local verdict=BAD
  if [ "$exit_code" -ne 0 ] && [ "$named" = "OK" ]; then verdict=OK; fi
  RECORD_NAME="$name"; RECORD_EXIT="$exit_code"; RECORD_NAMED="$named"; RECORD_VERDICT="$verdict"
}

run_n03() {
  reset_copy
  local dir="$EV/n03-tamper-pin-count"
  mkdir -p "$dir"
  python3 "$COPY/scripts/rust-tauri/r05_t08_mutate_pin.py" \
    "$COPY/docs/rust-tauri/R05/r05_stage_pins.tsv" "$dir/mutation.json" \
    > "$dir/mutation.log" 2>&1
  local mutation_exit=$?
  echo "$mutation_exit" > "$dir/mutation-exit-code.txt"
  [ "$mutation_exit" -eq 0 ] || { cat "$dir/mutation.log"; fail "N03 injection refused: unique authoritative anchor required"; }
  note "== N03: exactly one authoritative pin count lowered; old/new saved in mutation.json =="
  xtask_test n03-tamper-pin-count r05_stage_pin_table_matches \
    "svc:r05_t01_model_plane" "drifted" \
    "test stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites ... FAILED" \
    "0 passed; 1 failed; 0 ignored"
  record_case "R05-GATE-N03" "$RECORD_EXIT" "$RECORD_NAMED" "$RECORD_VERDICT" "权威表唯一目标降钉恰一次 → 镜像单测红点名漂移"
  reset_copy
  mkdir -p "$EV/n03-restored"
  cmp "$COPY/docs/rust-tauri/R05/r05_stage_pins.tsv" \
    "$PRISTINE/docs/rust-tauri/R05/r05_stage_pins.tsv" || fail "N03 restore differs"
  cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask \
    stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites -- --exact \
    > "$EV/n03-restored/test.stdout.log" 2>&1
  local restored_exit=$?
  echo "$restored_exit" > "$EV/n03-restored/exit-code.txt"
  [ "$restored_exit" -eq 0 ] && \
    grep -qF "1 passed; 0 failed; 0 ignored" "$EV/n03-restored/test.stdout.log" \
    || fail "N03 restored mirror was not run or not green"
  note "PASS N03 restore: authoritative table byte-equal, exact mirror 1/1 green"
}

write_results() {
# ── verdict ─────────────────────────────────────────────────────────────────
python3 - "$EV" "$CASE_SCOPE" "$COPY" <<'PY' || fail "at least one R05 negative case did NOT fail closed with the gap named"
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
scope = sys.argv[2]
expected = ["R05-GATE-N03"] if scope == "N03" else [f"R05-GATE-N{i:02d}" for i in range(1, 17)]
rows = []
for line in (ev / "case-results.tsv").read_text().splitlines():
    if not line.strip():
        continue
    name, exit_code, named, verdict, note = line.split("\t")
    rows.append({"case": name, "exitCode": int(exit_code), "gapNamed": named,
                 "verdict": verdict, "note": note})
controls = [{"name": "xtask-r05-mirror", "exitCode": int((ev / "control-xtask" / "exit-code.txt").read_text()), "expected": 0},
            {"name": "n03-restored-mirror", "exitCode": int((ev / "n03-restored" / "exit-code.txt").read_text()), "expected": 0}]
if scope == "ALL":
    controls.append({"name": "binary-wiring-suite", "exitCode": int((ev / "control-binwiring" / "exit-code.txt").read_text()), "expected": 0})
doc = {
    "schema": "lingxi.r05-negative-gate.v1",
    "isolatedCopy": "local git clone (read-only on the source) + uncommitted working-tree overlay under $HOME; the main working tree carries ZERO injections (reset_copy restores every mutated file from the pristine snapshot)",
    "scope": scope,
    "isolatedCopyPath": sys.argv[3],
    "expectedCases": expected,
    "unexecutedCases": [f"R05-GATE-N{i:02d}" for i in range(1, 17) if f"R05-GATE-N{i:02d}" not in expected],
    "controls": controls,
    "cases": rows,
    "allRefused": [r["case"] for r in rows] == expected and all(r["verdict"] == "OK" for r in rows),
    "controlsGreen": all(c["exitCode"] == c["expected"] for c in controls),
}
(ev / "case-results.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
print(json.dumps({"cases": len(rows), "allRefused": doc["allRefused"], "controlsGreen": doc["controlsGreen"]}))
if not (doc["allRefused"] and doc["controlsGreen"]):
    raise SystemExit(1)
PY
}

: > "$EV/case-results.tsv"
mkdir -p "$EV/control-xtask"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask r05_ > "$EV/control-xtask/test.stdout.log" 2>&1
code=$?
echo "$code" > "$EV/control-xtask/exit-code.txt"
[ "$code" -eq 0 ] || fail "normal mirror failed"
run_n03
write_results
