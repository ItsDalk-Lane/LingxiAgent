#!/usr/bin/env bash
# S3a：干净候选。一次性副本内先恢复 tracked 改动并移除 untracked-unignored
# 文件（仅Fixture 准备，门禁自身不含任何破坏性 git 操作），使
# `git status --porcelain --untracked-files=all` 为空，然后跑 directed 门禁。
# 期望 exit 0 且 summary 记录 "candidate worktree clean"。
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG=s3-clean
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
C="$W/repo"
[ "$(cd "$C" && pwd -P)" != "$MAIN_REPO" ] || { echo "S3a FAIL (copy path == real repo, refusing)"; exit 1; }
git -C "$C" checkout -- . 2>/dev/null || git -C "$C" restore --source=HEAD --worktree -- .  # fixture 准备：丢弃副本内 tracked 改动
( cd "$C" && git ls-files --others --exclude-standard -z | xargs -0 rm -rf )               # fixture 准备：移除副本内 untracked-unignored 路径（-rf：嵌套 git 仓库条目是目录；仅在副本内执行，rm 的 cwd 必须是副本，绝不能是真实仓库根）
LEFT="$(git -C "$C" status --porcelain --untracked-files=all | wc -l | tr -d ' ')"
echo "clean-candidate residue entries: $LEFT"
[ "$LEFT" = 0 ] || { echo "S3a FAIL (copy not clean)"; exit 1; }
( cd "$C" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
    bash "$GATE" "$W/ev" > "$W/gate-stdout.log" 2> "$W/gate-stderr.log" )
RC=$?
echo "gate exit=$RC"
rm -rf "$C"
grep -q "candidate worktree clean" "$W/ev/legacy-entry/summary.txt" 2>/dev/null && echo "clean-binding note OK"
[ "$RC" = 0 ] && echo "S3a PASS" || { echo "S3a FAIL"; tail -5 "$W/gate-stderr.log" 2>/dev/null; exit 1; }
