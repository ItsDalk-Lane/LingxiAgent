set -uo pipefail; ROOT="$1"; COPY="$2"; EV="$3"; note(){ echo "$*"; }; fail(){ echo "FAIL: $*" >&2; exit 1; };
SOURCE_HEAD="$(git -C "$ROOT" rev-parse --short HEAD)" || fail "source HEAD query failed"
note "== preparing the isolated copy at $COPY (local clone of HEAD $SOURCE_HEAD + uncommitted working-tree overlay) =="
rmdir "$COPY" || fail "isolated copy is no longer empty"
if ! python3 "$ROOT/scripts/rust-tauri/prepare_git_copy.py" \
  --source "$ROOT" --copy "$COPY" --revision HEAD --branch codex/rust-tauri-migration \
  --evidence "$EV/git-preparation" > "$EV/clone.log" 2>&1; then
  cat "$EV/clone.log"; fail "complete Git copy preparation failed"
fi
# Overlay the uncommitted candidate: rust/, scripts/, docs/rust-tauri/
# (the clone already carries the committed R00–R04 docs; rsync --delete makes
# the overlaid subtrees byte-equal to the main working tree). rust/target is
# excluded — it is a gitignored build cache, not candidate source, and the
# copy builds into its own NEG_TARGET cache anyway.
for sub in rust scripts docs/rust-tauri; do
  rsync -a --delete --exclude 'target/' "$ROOT/$sub/" "$COPY/$sub/" || fail "rsync overlay failed for $sub"
done
# 先准备当前候选的Node输入及独立依赖，再拍任何快照或启动门禁。
# 失败直接停止；完整依赖清单、真实工具及读取探针保存在本轮证据。
python3 "$ROOT/scripts/rust-tauri/r05_t08_prepare_node.py" \
  --source "$ROOT" --copy "$COPY" --evidence "$EV/node-preparation" \
  || fail "isolated Node dependency preparation failed"
# 已提交的历史证据完整保留；本轮新增输出不从来源额外带入。
# 副本绑定覆盖其全部候选输入，规则与原先相同。
COPY_BRANCH="$(git -C "$COPY" rev-parse --abbrev-ref HEAD)" || fail "copy branch query failed"
COPY_HEAD="$(git -C "$COPY" rev-parse --short HEAD)" || fail "copy HEAD query failed"
COPY_STATUS="$(git -C "$COPY" status --porcelain)" || fail "copy status query failed"
COPY_DIRTY_COUNT="$(printf '%s\n' "$COPY_STATUS" | sed '/^$/d' | wc -l | tr -d ' ')" || fail "copy dirty count failed"
note "== copy ready (branch $COPY_BRANCH, HEAD $COPY_HEAD, worktree dirty=$COPY_DIRTY_COUNT files) =="
