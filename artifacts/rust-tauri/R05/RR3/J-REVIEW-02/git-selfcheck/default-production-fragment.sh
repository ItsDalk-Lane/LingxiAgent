set -euo pipefail; ROOT="$1"; COPY="$2"; EV="$3"; fail(){ echo "$*" >&2; exit 1; };
rmdir "$COPY" || fail "isolated copy is no longer empty"
if ! python3 "$ROOT/scripts/rust-tauri/prepare_git_copy.py" \
  --source "$ROOT" --copy "$COPY" --revision HEAD --branch codex/rust-tauri-migration \
  --evidence "$EV/git-preparation" > "$EV/clone.log" 2>&1; then
  cat "$EV/clone.log"; fail "complete Git copy preparation failed"
fi
