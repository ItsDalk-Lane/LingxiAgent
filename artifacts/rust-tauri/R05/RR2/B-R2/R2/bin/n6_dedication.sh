#!/usr/bin/env bash
# N6（dedication 围栏 in-vivo）：sink 落在一个含既有旧证据的目录里 ——
#  a) 门禁必须仍通过，但该目录不得成为 DIR 排除单元（降级为 FILE：只排除
#     sink 文件本身），旧证据保持绑定；
#  b) 绑定中途改这份旧证据 → 镜像 cmp 必须红。
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
MODE="${1:-a}"   # a = 通过+FILE 降级；b = 旧证据中途变异必须红
TAG="n6-$MODE"
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
C="$W/repo"
OLD="artifacts/rust-tauri/R05/RR2/B-R2/round2-probe/old"
mkdir -p "$C/$OLD"
printf 'old static untracked evidence (previous round)\n' > "$C/$OLD/old.log"
( cd "$C" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
    bash "$GATE" "$W/ev" \
    > "$C/artifacts/rust-tauri/R05/RR2/B-R2/round2-probe/run.log" \
    2> "$W/gate-stderr-was-stderr.log" ) &     # stderr 走 /tmp：只有 stdout 是被测 sink
GATE_PID=$!
if ! wait_gate_line "$C/artifacts/rust-tauri/R05/RR2/B-R2/round2-probe/run.log" \
     'NOTE candidate-worktree-dirty|candidate worktree clean' 300 "$GATE_PID"; then
  echo "N6-$MODE FAIL (binding line never appeared)"; kill "$GATE_PID" 2>/dev/null; exit 1
fi
if [ "$MODE" = b ]; then
  sleep 3
  printf 'MUTATED mid-binding: old static evidence change\n' >> "$C/$OLD/old.log"
  echo "mutation applied to $OLD/old.log"
fi
wait "$GATE_PID"; RC=$?
SINKS="$W/ev/legacy-entry/e0-run-output-sinks.txt"
echo "N6-$MODE gate exit=$RC"
echo "--- sinks ---"; cat "$SINKS" 2>/dev/null
rm -rf "$C"
if [ "$MODE" = a ]; then
  [ "$RC" = 0 ] && grep -q '^FILE artifacts/rust-tauri/R05/RR2/B-R2/round2-probe/run.log$' "$SINKS" \
    && ! grep -q '^DIR artifacts/rust-tauri/R05/RR2/B-R2/round2-probe$' "$SINKS" \
    && echo "N6a PASS (dir NOT promoted; FILE-only attribution; old evidence stays bound)" \
    || { echo "N6a FAIL"; exit 1; }
else
  [ "$RC" -ne 0 ] && grep -q 'does not mirror the invoking worktree' "$W/gate-stderr-was-stderr.log" \
    && echo "N6b PASS (old-evidence mid-binding change still fails the mirror cmp)" \
    || { echo "N6b FAIL"; exit 1; }
fi
