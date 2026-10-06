#!/usr/bin/env bash
# 非法排除根负例：R02_A16_RUN_OUTPUT_ROOTS=<root> 必须在绑定前被拒绝（快速
# 失败、退出非 0、报错点名 declared run-output root 非法）。usage: illegal.sh <root>
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG="illegal-$(printf '%s' "$1" | tr '/' '_')"
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
( cd "$W/repo" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
    R02_A16_RUN_OUTPUT_ROOTS="$1" \
    bash "$GATE" "$W/ev" > "$W/gate-stdout.log" 2> "$W/gate-stderr.log" )
RC=$?
echo "ILLEGAL root=[$1] gate exit=$RC"
cat "$W/gate-stderr.log"
rm -rf "$W/repo"
if [ "$RC" -ne 0 ] && grep -q "declared run-output root" "$W/gate-stderr.log"; then
  echo "ILLEGAL PASS (rejected: $1)"
else
  echo "ILLEGAL FAIL (root was NOT rejected: $1)"; exit 1
fi
