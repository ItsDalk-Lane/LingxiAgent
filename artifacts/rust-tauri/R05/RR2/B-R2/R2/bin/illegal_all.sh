#!/usr/bin/env bash
# 非法排除根负例（共享一个副本，三次调用，各自全新 /tmp 证据根）：rust/（源码
# 目录）、artifacts（整个证据树）、artifacts/rust-tauri（整个证据分发树）。
# 每次都必须在绑定前被拒绝：退出非 0 且报错点名 declared run-output root。
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
. "$BIN/lib.sh"
TAG=illegal-roots
W="$(new_work "$TAG")"; echo "WORK=$W"
make_copy "$W" || { echo "FAIL copy"; exit 90; }
C="$W/repo"
declare -a ROOTS=(rust artifacts artifacts/rust-tauri)
declare -a OK=(0 0 0)
for i in 0 1 2; do
  EV="$W/ev-$i"
  ( cd "$C" && R02_LEGACY_REGRESSION_MODE=directed-no-seal-family \
      R02_A16_RUN_OUTPUT_ROOTS="${ROOTS[$i]}" \
      bash "$GATE" "$EV" > "$W/gate-stdout-$i.log" 2> "$W/gate-stderr-$i.log" )
  RC=$?
  MSG="$(tail -1 "$W/gate-stderr-$i.log" 2>/dev/null)"
  echo "root=[${ROOTS[$i]}] exit=$RC msg=$MSG"
  if [ "$RC" -ne 0 ] && printf '%s' "$MSG" | grep -q "declared run-output root"; then
    OK[$i]=1
  fi
done
rm -rf "$C"
if [ "${OK[0]}" = 1 ] && [ "${OK[1]}" = 1 ] && [ "${OK[2]}" = 1 ]; then
  echo "ILLEGAL-ALL PASS (rust/ + artifacts + artifacts/rust-tauri all rejected)"
else
  echo "ILLEGAL-ALL FAIL"; exit 1
fi
