#!/usr/bin/env bash
# One-off re-run of the c03b (mid-run candidate drift) negative case after
# the battery wrapper was killed by the environment mid-case. Same
# isolation methodology as r04_t08_gate_negative_tests.sh (worktree at HEAD
# + uncommitted R04-T08 files; home-dir cargo target).
set -uo pipefail
ROOT="/Users/study_superior/Desktop/Code/LingxiAgent"
EV="$ROOT/artifacts/rust-tauri/R04/T08-E01/negative-tests"
CARGO="$HOME/.cargo/bin/cargo"
NEG_TARGET="${R04_NEG_CARGO_TARGET:-$HOME/.cache/lingxi-r04-t08-neg-target}"
export CARGO_NET_OFFLINE=true

COPY="$(mktemp -d /tmp/lingxi-r04-t08-neg-XXXXXX)"
echo "copy=$COPY"
git -C "$ROOT" worktree add --detach "$COPY" HEAD > "$EV/c03b-worktree-add.log" 2>&1 || { cat "$EV/c03b-worktree-add.log"; exit 1; }
cleanup() { git -C "$ROOT" worktree remove --force "$COPY" >/dev/null 2>&1 || true; }
trap cleanup EXIT
for f in \
  rust/crates/xtask/src/main.rs \
  rust/crates/xtask/src/runner_identity.rs \
  rust/crates/xtask/src/stage_map.rs \
  rust/crates/xtask/src/stage_maps/R04.json \
  rust/crates/lingxi-service/src/artifactverify.rs \
  rust/crates/lingxi-service/src/filetools.rs \
  rust/crates/lingxi-service/src/lib.rs \
  rust/crates/lingxi-service/src/toolgateway.rs \
  rust/crates/lingxi-service/src/workerrpc.rs \
  rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs \
  rust/crates/lingxi-service/tests/r04_t04_file_tools.rs \
  rust/crates/lingxi-service/tests/r04_t07_mcp_and_workers.rs \
  rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs \
  scripts/rust-tauri/r04_t08_matrix.sh \
  scripts/rust-tauri/r04_t08_generate_stage_map.py \
  scripts/rust-tauri/r04_t08_gate_negative_tests.sh; do
  mkdir -p "$COPY/$(dirname "$f")"
  cp "$ROOT/$f" "$COPY/$f"
done
chmod +x "$COPY/scripts/rust-tauri/r04_t08_matrix.sh"
[ -d "$ROOT/node_modules" ] && cp -Rc "$ROOT/node_modules" "$COPY/node_modules" 2>/dev/null || cp -R "$ROOT/node_modules" "$COPY/node_modules"

C3B_DIR="$EV/c03b-midrun-input-change"
rm -rf "$C3B_DIR"; mkdir -p "$C3B_DIR"
(cd "$COPY" && CARGO_TARGET_DIR="$NEG_TARGET" "$CARGO" run --manifest-path rust/Cargo.toml \
   --locked -p xtask -- verify-stage R04 --evidence "$C3B_DIR/evidence" \
   > "$C3B_DIR/gate.stdout.log" 2> "$C3B_DIR/gate.stderr.log") &
GATE_PID=$!
CHANGED=0
for _ in $(seq 1 7200); do
  if [ -f "$C3B_DIR/evidence/rust_test_workspace/stdout.log" ]; then
    sleep 2
    printf '\n// R04-T08 C03 controlled STALE demo: execution input changed mid-run.\n' \
      >> "$COPY/rust/crates/lingxi-service/src/lib.rs"
    CHANGED=1
    break
  fi
  kill -0 "$GATE_PID" 2>/dev/null || break
  sleep 1
done
wait "$GATE_PID"
C3B_EXIT=$?
echo "$C3B_EXIT" > "$C3B_DIR/exit-code.txt"
echo "$CHANGED" > "$C3B_DIR/midrun-change-applied.txt"
C3B_RESULT="$C3B_DIR/evidence/verify-stage-result.json"
C3B_STABLE="$(python3 - "$C3B_RESULT" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
    print(str(d.get("candidateSourceBinding", {}).get("stable")).lower())
except Exception:
    print("unreadable")
PY
)"
C3B_NAMED=OK
grep -qF "Candidate file bytes or HEAD changed" "$C3B_RESULT" 2>/dev/null \
  || C3B_NAMED="MISSING:stable-reason"
[ "$C3B_STABLE" = "false" ] || C3B_NAMED="MISSING:stable=false(got:$C3B_STABLE)"
[ "$CHANGED" -eq 1 ] || C3B_NAMED="MISSING:midrun-window"
C3B_VERDICT=BAD; if [ "$C3B_EXIT" -ne 0 ] && [ "$C3B_NAMED" = "OK" ]; then C3B_VERDICT=OK; fi
printf 'c03b-midrun-input-change\t%s\t%s\t%s\n' "$C3B_EXIT" "$C3B_NAMED" "$C3B_VERDICT" > "$C3B_DIR/case-row.tsv"
echo "c03b done: exit=$C3B_EXIT named=$C3B_NAMED verdict=$C3B_VERDICT"
