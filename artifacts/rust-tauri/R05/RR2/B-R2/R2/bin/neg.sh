#!/usr/bin/env bash
# 负例驱动：一次性副本内跑 directed 门禁（/tmp 证据根 + /tmp 捕获 → 排除单元为
# 空、绑定穷举），主绑定完成后立刻变异副本树，期望 E0 镜像 cmp 失败并带
# "does not mirror the invoking worktree" 签名。usage: neg.sh <tag> <mutation-cmd...>
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG="$1"; shift
W="$(new_work "neg-$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
C="$W/repo"
( cd "$C" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
    bash "$GATE" "$W/ev" > "$W/gate-stdout.log" 2> "$W/gate-stderr.log" ) &
GATE_PID=$!
# 主绑定后立即出现 dirty/clean NOTE；等它出现再变异（变异落在主绑定与副本
# 绑定之间 → cmp 必须红）
if ! wait_gate_line "$W/gate-stdout.log" 'NOTE candidate-worktree-dirty|candidate worktree clean' 300 "$GATE_PID"; then
  echo "NEG-$TAG FAIL (binding line never appeared / gate exited early)"; wait "$GATE_PID"; exit 1
fi
sleep 3
( cd "$C" && "$@" ) > "$W/mutation.log" 2>&1
echo "mutation: $(cat "$W/mutation.log")"
wait "$GATE_PID"; RC=$?
echo "NEG-$TAG gate exit=$RC"
tail -2 "$W/gate-stderr.log"
rm -rf "$C"
if [ "$RC" -ne 0 ] && grep -q 'does not mirror the invoking worktree' "$W/gate-stderr.log"; then
  echo "NEG-$TAG PASS"
else
  echo "NEG-$TAG FAIL"; exit 1
fi
