#!/usr/bin/env bash
# S1a：标准仓库内证据根 + 本运行 stdout/stderr 落 artifacts 内 .log（但不落
# 证据子树内）——RR1 INDEPENDENT-9 的失败形态在修复后必须通过。
# 期望：exit 0；e0-run-output-sinks.txt 记 DIR run-root 与 DIR 证据目录；
# e0-binding-exclusions.txt 同时含两者；镜像 cmp 通过。
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG=s1a-inrepo
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
RR="artifacts/rust-tauri/R05/RR2/B-R2/R2SIM/$TAG"
mkdir -p "$W/repo/$RR/run-root"
( cd "$W/repo" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
    bash "$GATE" "$RR/ev" > "$W/repo/$RR/run-root/stdout.log" 2> "$W/repo/$RR/run-root/stderr.log" )
RC=$?
echo "gate exit=$RC"
mkdir -p "$W/captured"
cp "$W/repo/$RR/run-root/stdout.log" "$W/captured/gate-stdout.log" 2>/dev/null || true
cp -Rc "$W/repo/$RR/ev" "$W/captured/ev" 2>/dev/null || true
echo "--- sinks ---"; cat "$W/captured/ev/legacy-entry/e0-run-output-sinks.txt" 2>/dev/null
echo "--- exclusions ---"; cat "$W/captured/ev/legacy-entry/e0-binding-exclusions.txt" 2>/dev/null
rm -rf "$W/repo"
[ "$RC" = 0 ] && grep -q '^DIR artifacts' "$W/captured/ev/legacy-entry/e0-run-output-sinks.txt" \
  && echo "S1a PASS" || { echo "S1a FAIL"; exit 1; }
