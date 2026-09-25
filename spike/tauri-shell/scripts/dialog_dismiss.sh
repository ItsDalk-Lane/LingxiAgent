#!/bin/zsh
# R01-T06 文件对话框自动取消器。实测结论（见 TAURI_SPIKE_REPORT §7-R1）：
# - 合成 Escape（cghidEventTap 或 postToPid）会让 NSOpenPanel 走 performClose: 关闭，
#   面板消失但 rfd 完成回调**不触发**（callback 永不兑现）；
# - 只有 Cmd+period（cancel: 的键盘等价物）postToPid 能确定性触发取消回调。
# 激活应用（System Events，偶发 -600 竞态故重试）仅作真实性辅助；功能部分为 postToPid。
# 合成事件注入需要注入进程持有辅助功能权限；失败如实非零退出。
# 用法：dialog_dismiss.sh <pid> <wait_s> <post_cmd_period_bin>
set -u
PID="$1"
WAIT_S="${2:-6}"
POST_CP="${3:-/tmp/lingxi-t06-bin/post_cmd_period}"
sleep "$WAIT_S"
/usr/bin/osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true" 2>/dev/null
ACT=$?
sleep 1
"$POST_CP" "$PID"
CP=$?
echo "activate_exit=$ACT(best-effort) post_cmd_period_exit=$CP"
[ $CP -eq 0 ]
