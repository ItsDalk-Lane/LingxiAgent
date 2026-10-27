#!/bin/bash
# R06-T02 候选摘要计算（R3-F-02 修复——口径固化，声明口径与本脚本逐字一致）。
#
# 范围 = 已跟踪改动的完整 diff（git diff HEAD，含暂存与未暂存）
#      + 全部未跟踪文件的逐文件 sha256（LC_ALL=C 排序后逐一计算）。
#
# 排除（两根因对策）：
#   (a) artifacts/rust-tauri/R06/ 下全部产物目录（审查/根因/修复证据——
#       含 digest 载体文件自身；载体自引用在数学上无不动点，必须排除）；
#   (b) 任何位置的候选 digest 值文件（candidate_digest*.txt——双保险；
#       本脚本自身不含 digest 值，留在口径内）。
#
# 报告 docs/rust-tauri/R06/R06-T02_REPORT.md 在口径内——故报告正文不得
# 含 digest 值；值在报告定稿之后由本脚本计算，只落证据文件与交付消息。
#
# 用法：scripts/rust-tauri/r06_candidate_digest.sh
# 输出：单行 sha256（候选摘要）。
set -euo pipefail
cd "$(dirname "$0")/../.."
{
  git diff HEAD
  git ls-files --others --exclude-standard \
    | LC_ALL=C sort \
    | { grep -v '^artifacts/rust-tauri/R06/' || true; } \
    | { grep -v 'candidate_digest.*\.txt$' || true; } \
    | while IFS= read -r f; do shasum -a 256 -- "$f"; done
} | shasum -a 256 | awk '{print $1}'
