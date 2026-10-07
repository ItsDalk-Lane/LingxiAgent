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
    kill -0 "$pid" 2>/dev/null || { echo "N06 prerequisite failed: gate exited" >&2; return 1; }
    grep -Fx -- "$signal" "$log" >/dev/null
    matched=$?
    if [ "$matched" -eq 0 ]; then
      kill -0 "$pid" 2>/dev/null || { echo "N06 prerequisite failed: gate exited after signal" >&2; return 1; }
      return 0
    fi
    [ "$matched" -eq 1 ] || { echo "N06 prerequisite failed: start log query failed" >&2; return 1; }
    sleep 1
  done
  echo "N06 prerequisite failed: authoritative command start not observed" >&2
  return 1
}

eval "$3"
