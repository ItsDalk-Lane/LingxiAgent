#!/usr/bin/env bash
# R05 RR2 WP-B 修复轮 1（B-R2/R2）自检公共库。所有场景都在一次性 CoW 副本里
# 跑门禁（真实工作树只读：仅在场景启动时被 cp -Rc 快照一次），场景产物先落在
# /tmp WORK，再由 archive() 归档到真实证据树。用法：source 本文件。
set -u
MAIN_REPO="/Users/study_superior/Desktop/Code/LingxiAgent"
EVBASE="$MAIN_REPO/artifacts/rust-tauri/R05/RR2/B-R2/R2"
GATE="scripts/rust-tauri/r02_t08_legacy_entry_regression.sh"

new_work() { mktemp -d "${TMPDIR:-/tmp}/b-r2r2-${1:-sc}.XXXXXX"; }

make_copy() { # <work> -> $work/repo
  # 性能捷径（已在证据 README 说明）：跳过 rust/target —— 它是 gitignored
  # 产物目录，从不进入绑定（bind 只收未忽略路径），门禁的 npm 步骤也不使用
  # 它；省去对 ~19 万+ 目录项的克隆遍历。其余内容（.git/node_modules/
  # artifacts/源码）全部保留。等价性由 s1b（全量副本）与上一轮
  # standalone-full-1（真实全树）交叉印证。
  local dst="$1/repo"
  mkdir -p "$dst/rust"
  ( cd "$MAIN_REPO" || exit 1
    for f in * .[!.]*; do
      [ -e "$f" ] || continue
      [ "$f" = "rust" ] && continue
      cp -Rc "$f" "$dst/" || exit 1
    done
    for f in rust/* rust/.[!.]*; do
      [ -e "$f" ] || continue
      [ "$f" = "rust/target" ] && continue
      cp -Rc "$f" "$dst/rust/" || exit 1
    done
  )
}

gate_directed() { # <copy> <evidence-root> [extra env as VAR=VAL ...]
  local copy="$1" ev="$2"; shift 2
  ( cd "$copy" && env R02_LEGACY_REGRESSION_MODE=directed-no-seal-family "$@" \
      bash "$GATE" "$ev" )
}

archive() { # <tag> <work>  — 把 WORK 里的驱动记录与（若存在）证据子树归档
  local tag="$1" work="$2" dest="$EVBASE/$tag"
  mkdir -p "$dest"
  cp -Rc "$work" "$dest/work" 2>/dev/null || true
  echo "archived $work -> $dest/work"
}

wait_gate_line() { # <gate-log> <pattern(ERE)> <timeout-sec> <pid> -> 0 seen / 1 gone / 2 timeout
  local log="$1" pat="$2" tmo="$3" pid="$4" i
  for ((i=0; i<tmo; i++)); do
    grep -qE "$pat" "$log" 2>/dev/null && return 0
    kill -0 "$pid" 2>/dev/null || return 1
    sleep 1
  done
  return 2
}
