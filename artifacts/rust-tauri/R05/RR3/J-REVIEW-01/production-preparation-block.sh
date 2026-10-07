SOURCE_HEAD="$(git -C "$ROOT" rev-parse --short HEAD)" || fail "source HEAD query failed"
note "== preparing the isolated copy at $COPY (local clone of HEAD $SOURCE_HEAD + uncommitted working-tree overlay) =="
rm -rf "$COPY"
if ! git -C "$ROOT" clone --shared --quiet --branch codex/rust-tauri-migration "$ROOT" "$COPY" > "$EV/clone.log" 2>&1; then
  cat "$EV/clone.log"; fail "local clone failed"
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
