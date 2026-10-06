#!/usr/bin/env bash
# N4b：旧静态 untracked 证据（上一轮运行留下的、未被任何排除单元覆盖的 .log）
# 在绑定中途被改动 → 镜像 cmp 必须红。与 N4a（tracked 旧证据）互补。
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG=n4b-old-static-untracked
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
C="$W/repo"
OLD="artifacts/rust-tauri/R05/RR2/B-R2/old-round-probe"
mkdir -p "$C/$OLD"
printf 'old static untracked evidence from a previous round\n' > "$C/$OLD/old.log"
( cd "$C" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
    bash "$GATE" "$W/ev" > "$W/gate-stdout.log" 2> "$W/gate-stderr.log" ) &
GATE_PID=$!
if ! wait_gate_line "$W/gate-stdout.log" 'NOTE candidate-worktree-dirty|candidate worktree clean' 300 "$GATE_PID"; then
  echo "NEG-$TAG FAIL (binding line never appeared)"; kill "$GATE_PID" 2>/dev/null; exit 1
fi
sleep 3
printf 'MUTATED mid-binding: old static UNTRACKED evidence change\n' >> "$C/$OLD/old.log"
echo "mutation applied to $OLD/old.log"
wait "$GATE_PID"; RC=$?
echo "NEG-$TAG gate exit=$RC"
tail -2 "$W/gate-stderr.log"
rm -rf "$C"
if [ "$RC" -ne 0 ] && grep -q 'does not mirror the invoking worktree' "$W/gate-stderr.log"; then
  echo "NEG-$TAG PASS"
else
  echo "NEG-$TAG FAIL"; exit 1
fi
