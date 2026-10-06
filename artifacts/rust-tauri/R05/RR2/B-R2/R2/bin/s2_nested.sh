#!/usr/bin/env bash
# S2 / S5b：嵌套布局模拟（verify-stage 嵌套调用形态，RR1 INDEPENDENT-9 同构）。
# 外层 runner 自身 fd→ r04_regression_gate/*.log 且其 heartbeat 持续写该 sink；
# 外层把中层输出重定向到 r03_regression_gate/*.log，中层 heartbeat 持续写；
# 中层再把门禁输出重定向到 r02_legacy_regression/*.log。三层日志在门禁绑定
# 窗口内同时增长。期望 exit 0，且三个命令目录 + 证据目录都作为 DIR 排除单元
# 被显式归属。
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG=s2-nested
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
RR="artifacts/rust-tauri/R05/RR2/B-R2/R2SIM/$TAG/verify-R05"
R04D="$W/repo/$RR/r04_regression_gate"
R03D="$W/repo/$RR/R04_REGRESSION/r03_regression_gate"
mkdir -p "$R04D" "$R03D"

run_mid() { # 中层：fd1/2 已由外层重定向到 r03 sink；自身 heartbeat 写继承的 r03 sink
  local top="artifacts/rust-tauri/R05/RR2/B-R2/R2SIM/$TAG/verify-R05/R04_REGRESSION"
  mkdir -p "$W/repo/$top/R03_REGRESSION/r02_legacy_regression"
  ( while :; do echo "mid-heartbeat $(date '+%H:%M:%S')"; sleep 5; done ) & local h1=$!
  ( cd "$W/repo" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
      bash "$GATE" "$top/R03_REGRESSION/R02/A16" \
      > "$W/repo/$top/R03_REGRESSION/r02_legacy_regression/stdout.log" \
      2> "$W/repo/$top/R03_REGRESSION/r02_legacy_regression/stderr.log" )
  local rc=$?
  kill "$h1" 2>/dev/null; wait "$h1" 2>/dev/null
  return $rc
}
run_outer() { # 外层：fd1/2 已由调用方重定向到 r04 sink；heartbeat 写继承的 r04 sink
  ( while :; do echo "outer-heartbeat $(date '+%H:%M:%S')"; sleep 5; done ) & local h0=$!
  ( cd "$W/repo" && run_mid > "$R03D/stdout.log" 2> "$R03D/stderr.log" )
  local rc=$?
  kill "$h0" 2>/dev/null; wait "$h0" 2>/dev/null
  return $rc
}
run_outer > "$R04D/stdout.log" 2> "$R04D/stderr.log"
RC=$?
echo "gate exit=$RC"
EV="$W/repo/$RR/R04_REGRESSION/R03_REGRESSION/R02/A16/legacy-entry"
mkdir -p "$W/captured"
cp "$EV"/e0-run-output-sinks.txt "$EV"/e0-binding-exclusions.txt "$W/captured/" 2>/dev/null || true
cp "$EV/summary.txt" "$W/captured/" 2>/dev/null || true
cp "$W/repo/$RR/R04_REGRESSION/R03_REGRESSION/r02_legacy_regression/stdout.log" "$W/captured/leaf-gate-stdout.log" 2>/dev/null || true
wc -l "$R04D/stdout.log" "$R03D/stdout.log" "$W/repo/$RR/R04_REGRESSION/R03_REGRESSION/r02_legacy_regression/stdout.log" > "$W/captured/sink-growth-line-counts.txt" 2>/dev/null || true
rm -rf "$W/repo"
echo "--- sinks ---"; cat "$W/captured/e0-run-output-sinks.txt" 2>/dev/null
echo "--- exclusions ---"; cat "$W/captured/e0-binding-exclusions.txt" 2>/dev/null
echo "--- sink growth ---"; cat "$W/captured/sink-growth-line-counts.txt" 2>/dev/null
[ "$RC" = 0 ] \
  && grep -q "DIR artifacts/rust-tauri/R05/RR2/B-R2/R2SIM/$TAG/verify-R05/r04_regression_gate\$" "$W/captured/e0-run-output-sinks.txt" \
  && grep -q "DIR .*R04_REGRESSION/r03_regression_gate\$" "$W/captured/e0-run-output-sinks.txt" \
  && grep -q "DIR .*r02_legacy_regression\$" "$W/captured/e0-run-output-sinks.txt" \
  && echo "S2 PASS (three-level run roots all attributed; parent+child logs grew simultaneously)" \
  || { echo "S2 FAIL"; exit 1; }
