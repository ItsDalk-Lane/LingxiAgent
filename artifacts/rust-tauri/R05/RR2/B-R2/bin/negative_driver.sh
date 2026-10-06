#!/usr/bin/env bash
# N-series negative driver: runs the gate INSIDE a throwaway CoW copy of the
# repo (the invoking worktree is never touched), waits until the main
# binding is taken (the candidate-worktree-dirty NOTE appears in the gate's
# out-of-repo stdout capture), then mutates the copy's tree MID-BINDING and
# expects the E0 mirror cmp to FAIL. usage: negative_driver.sh <tag> <mutation-script>
set -u
TAG="$1"; MUT="$2"
MAIN="/Users/study_superior/Desktop/Code/LingxiAgent"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/b2-neg-${TAG}.XXXXXX")"
COPY="$WORK/repo"
EV="$WORK/ev"
cp -Rc "$MAIN" "$COPY" || exit 90
mkdir -p "$EV"
cd "$COPY" || exit 90
bash scripts/rust-tauri/r02_t08_legacy_entry_regression.sh "$EV/legacy" \
  > "$WORK/gate-stdout.log" 2> "$WORK/gate-stderr.log" &
GATE=$!
# wait for the main binding: the dirty/clean NOTE is printed right after it
BOUND=0
for _ in $(seq 1 600); do
  if grep -q 'NOTE candidate-worktree-dirty\|candidate worktree clean' "$WORK/gate-stdout.log" 2>/dev/null; then BOUND=1; break; fi
  kill -0 "$GATE" 2>/dev/null || break
  sleep 1
done
if [ "$BOUND" = "1" ]; then
  sleep 3                       # let the copy's cp start rolling
  bash "$MUT" "$COPY" > "$WORK/mutation.log" 2>&1
  echo "mutation applied: $(cat "$WORK/mutation.log")" >> "$WORK/gate-stdout.log.note"
fi
wait "$GATE"; RC=$?
echo "NEG-$TAG exit=$RC bound=$BOUND"
tail -2 "$WORK/gate-stderr.log"
echo "WORKDIR=$WORK"
[ "$RC" -ne 0 ] && grep -q 'does not mirror the invoking worktree' "$WORK/gate-stderr.log" && exit 0
exit 1
