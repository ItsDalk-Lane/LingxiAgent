#!/usr/bin/env bash
# S5a：仓库根 standalone 完整跑（full 模式，E0–E5 全链）。真实工作树当前含合法
# tracked+untracked 改动（脏候选，即 S3b）；门禁 stdout/stderr 重定向到仓库内
# 专用 run-root 的 .log（RR1 失败形态本身）——修复后必须 exit 0 且最终 GREEN。
set -u
MAIN_REPO="/Users/study_superior/Desktop/Code/LingxiAgent"
GATE="scripts/rust-tauri/r02_t08_legacy_entry_regression.sh"
RR="$MAIN_REPO/artifacts/rust-tauri/R05/RR2/B-R2/R2/s5-full-2/run-root"
EV="$MAIN_REPO/artifacts/rust-tauri/R05/RR2/B-R2/R2/s5-full-2/ev"
mkdir -p "$RR"
cd "$MAIN_REPO" || exit 90
bash "$GATE" "$EV" > "$RR/stdout.log" 2> "$RR/stderr.log"
RC=$?
echo "S5a full gate exit=$RC"
tail -3 "$RR/stderr.log" 2>/dev/null
exit "$RC"
