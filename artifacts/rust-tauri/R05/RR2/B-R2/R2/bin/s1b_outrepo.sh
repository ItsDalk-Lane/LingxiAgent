#!/usr/bin/env bash
# S1b：仓库外证据根（/tmp）+ stdout/stderr 也在仓库外 —— 历史穷举绑定形态必须
# 继续通过（排除单元为空，绑定保持穷举）。
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG=s1b-outrepo
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
( cd "$W/repo" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
    bash "$GATE" "$W/ev" > "$W/gate-stdout.log" 2> "$W/gate-stderr.log" )
RC=$?
echo "gate exit=$RC"
echo "--- sinks ---"; cat "$W/ev/legacy-entry/e0-run-output-sinks.txt" 2>/dev/null || echo "(no sinks file)"
echo "--- exclusions ---"; cat "$W/ev/legacy-entry/e0-binding-exclusions.txt" 2>/dev/null || true
rm -rf "$W/repo"
[ "$RC" = 0 ] && [ ! -s "$W/ev/legacy-entry/e0-binding-exclusions.txt" ] \
  && echo "S1b PASS (exhaustive binding, no exclusions)" || { echo "S1b FAIL"; exit 1; }
