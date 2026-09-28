#!/usr/bin/env bash
# R02-T06 / acceptance R02-A12 + PROD-DEFECT-1 4-variant drill — the REAL
# lingxi-service binary against deliberately corrupted data homes.
#
# The four drill variants (mirroring the incumbent Node probe shapes of
# ADR-004 §2 / RR-T07-PROD-DEFECT-1; R02 new-stack equivalents):
#   v1  torn stamp + readable barrier_raised transition journal 1→2 (Node R7:
#       fail-open, wrote 55 entries, lied "no higher-epoch evidence was found")
#   v2  valid stamp + torn transition journal (Node R9: fail-open, wrote 55)
#   v3  MISSING stamp + readable barrier_raised journal 1→2 (Node R10: THE
#       fail-open shape — wrote 55 entries with an untruthful warning)
#   v4  healthy control (fresh home: stamps epoch 1 and starts normally)
# Expected in the new stack: v1–v3 REFUSE startup (exit 2) with truthful
# evidence (the readable journal's epochs are NAMED), every pre-existing
# file byte-identical, no stamp invented, no auth state written (the gate
# precedes the auth bootstrap); v4 starts and stops cleanly.
#
# Additional corruption forms for A12 (多形态损坏库):
#   v5  garbage main db file      -> storage open refuses NOTADB (exit 2),
#                                    corrupt file byte-identical, no empty
#                                    db created in its place
#   v6  torn WAL tail (INFO row)  -> SQLite recovers BY DESIGN (incomplete
#                                    trailing frame = transaction never
#                                    committed); the drill records the actual
#                                    outcome and asserts the recovered view
#                                    is transaction-consistent (no half run)
#   v7  tampered migration receipt-> SchemaTampered refusal (exit 2), file
#                                    byte-identical
#   v8  garbage WAL header (INFO) -> observed recovery-by-design row: the
#                                    SQLite WAL recovery discards invalid
#                                    frames; recorded with actual outcome
#
# Environment guards: rustup-locked toolchain 1.98.1, task-dedicated target
# dir, offline locked build, proxy vars stripped, synthetic homes only.
#
# R02 stage-repair R5 / R5-F03: every temp path this run creates lives
# under ONE run-unique subtree of the caller-designated root
# ($R02_DRILL_ROOT, else $TMPDIR, else /tmp) — never a bare /tmp prefix.
# The run records every home it created and every PID it spawned, and the
# EXIT trap plus the end-of-run residue check touch ONLY those owned paths
# and PIDs. The old shapes (`mktemp -d /tmp/lingxi-r02t06-drill-v*-…`,
# `pgrep -f lingxi-service --home /tmp/lingxi-r02t06-drill`,
# `rm -rf /tmp/lingxi-r02t06-drill-v*-*`) matched ANY run's directories —
# a completing run could delete a concurrently running sibling's homes or
# a previous run's evidence; they are gone.
#
# R02 stage-repair R6 / R6-F02: the R5 OWN_PIDS ledger was append-only —
# wait/reap did not remove numbers, the EXIT trap swept ALL historical
# ids with kill -9, and the final check kill -0-probed the same history.
# A reaped PID can be RECYCLED into someone else's process; "we once
# created it" is not "we own it NOW". The ledger is now an ACTIVE set:
# every spawn registers (record_pid) and every reap path MUST retire
# (retire_pid) — wait/kill-collected numbers leave the set immediately.
# cleanup() signals ONLY active-set numbers that STILL prove CURRENT
# ownership (the process exists AND its ppid is THIS shell); a recycled
# number fails that proof, is skipped loudly, and is never signalled.
# TERM/INT take the same guarded path. The end-of-run residue check
# asserts the ACTIVE set is empty (every spawn reaped on its own path),
# never probe-recycled history.
#
# Usage: scripts/rust-tauri/r02_t06_recovery_drill.sh [EVIDENCE_DIR]
# Env:   R02_DRILL_ROOT  dedicated temp root for this run (gate harnesses
#                      set it — or TMPDIR — inside their own isolation
#                      root); CARGO_TARGET_DIR as before.
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T06}"
EVIDENCE_DIR="$EVIDENCE_DIR/recovery-drill"
# 每轮证据目录必须全新，防止独立直跑时旧日志覆盖或冒充本轮结果。
if [ -L "$EVIDENCE_DIR" ] || { [ -e "$EVIDENCE_DIR" ] && [ ! -d "$EVIDENCE_DIR" ]; }; then
  echo "ERROR: evidence path is not a regular directory: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -d "$EVIDENCE_DIR" ]; then
  FIRST_ENTRY="$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit)" || exit 1
  [ -z "$FIRST_ENTRY" ] || { echo "ERROR: evidence directory is not empty: $EVIDENCE_DIR" >&2; exit 1; }
fi
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r02-t06}"
DRILL_ROOT="${R02_DRILL_ROOT:-${TMPDIR:-/tmp}}"
case "$DRILL_ROOT" in
  /*) ;;
  *) echo "ERROR: R02_DRILL_ROOT/TMPDIR must be absolute: $DRILL_ROOT" >&2; exit 1 ;;
esac
RUN_ROOT=$(mktemp -d "$DRILL_ROOT/lingxi-r02t06-drill.XXXXXX")

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
if [ -z "$TOOLCHAIN" ]; then
  echo "ERROR: cannot parse toolchain channel from rust-toolchain.toml" >&2
  exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/rustup" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "ERROR: rustup not found; this gate requires the locked toolchain ($TOOLCHAIN)" >&2
    exit 1
  fi
fi
CARGO="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR rustup run $TOOLCHAIN cargo"

SERVICE_PID=""
# R5-F03/R6-F02 own-scope tracking: OWN_HOMES lists every home dir this
# run created; OWN_PIDS is the CURRENT ACTIVE set of this run's unreaped
# child processes (spawn → record_pid, reap → retire_pid). Cleanup and
# the residue check consult ONLY these structures — never a global
# pattern match over /tmp or pgrep prefixes, and never reaped history.
OWN_HOMES=()
OWN_PIDS=()
SPAWN_TOTAL=0

record_home() { # $1=dir created by THIS run
  OWN_HOMES+=("$1")
  printf '%s\n' "$1" >> "$EVIDENCE_DIR/own-created-homes.txt"
}
record_pid() { # $1=pid spawned by THIS run, not yet reaped
  OWN_PIDS+=("$1")
  SPAWN_TOTAL=$((SPAWN_TOTAL+1))
}
retire_pid() { # $1=pid THIS run has reaped (waited/killed) — leaves the active set NOW
  local i
  for i in "${!OWN_PIDS[@]}"; do
    if [ "${OWN_PIDS[$i]}" = "$1" ]; then
      unset "OWN_PIDS[$i]"
    fi
  done
}
# child_state <pid>: one of exited | owned | foreign | unobservable —
# the only objects this run may signal are `owned` (the pid exists AND
# its ppid is THIS shell). A recycled number now belonging to another
# process reads `foreign` (R6-F02); a number whose ppid cannot be read
# reads `unobservable` and is treated as UNKNOWN — never signalled,
# never waited unboundedly (R12-F01: the old boolean probe conflated
# these three into one false value, and its owned branch did
# `kill -9; wait` with NO deadline — an unreapable direct child in
# D-state would hang the trap forever even though KILL cannot be
# ignored).
child_state() {
  local ppid
  if ! kill -0 "$1" 2>/dev/null; then
    printf 'exited\n'
    return 0
  fi
  ppid="$(ps -o ppid= -p "$1" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$ppid" ]; then
    printf 'unobservable\n'
  elif [ "$ppid" = "$$" ]; then
    printf 'owned\n'
  else
    printf 'foreign\n'
  fi
}
# R12-F01: bounded crash-stop for ONE provably-owned active-set member:
# KILL-9 (this script's contractual first signal — the drills crash the
# service deliberately; no TERM stage is inserted) → ≤5 s re-check poll.
# Prints the final state; an unreapable survivor is reported as residue
# by the caller, never waited on unboundedly.
bounded_kill_stop_owned() {
  local pid="$1" state="" i
  # KILL 前重新确认进程仍属本脚本，未知或已复用的 PID 不得被信号触碰。
  state="$(child_state "$pid")"
  if [ "$state" != "owned" ]; then printf '%s\n' "$state"; return 0; fi
  kill -9 "$pid" 2>/dev/null || true
  for i in $(seq 1 100); do
    state="$(child_state "$pid")"
    case "$state" in
      exited|foreign) break ;;
      owned|unobservable) : ;;
    esac
    sleep 0.05
  done
  printf '%s\n' "${state:-unobservable}"
}
# 健康服务与同轮辅助进程先 TERM，只有仍能证明归属时才升级 KILL。
bounded_term_stop_owned() {
  local pid="$1" state="" i
  state="$(child_state "$pid")"
  if [ "$state" != "owned" ]; then printf '%s\n' "$state"; return 0; fi
  kill -TERM "$pid" 2>/dev/null || true
  for i in $(seq 1 100); do
    state="$(child_state "$pid")"
    case "$state" in exited|foreign) break ;; owned|unobservable) : ;; esac
    sleep 0.05
  done
  if [ "${state:-owned}" = "owned" ]; then state="$(bounded_kill_stop_owned "$pid")"; fi
  printf '%s\n' "${state:-unobservable}"
}
crash_active_pid() {
  local pid="$1" state rc
  [ "$(child_state "$pid")" = "owned" ] || fail "crash target $pid is not an owned live child"
  state="$(bounded_kill_stop_owned "$pid")"
  case "$state" in exited) ;; *) fail "crash target $pid remained or ownership changed after KILL budget (state=$state)" ;; esac
  if wait "$pid" 2>/dev/null; then rc=0; else rc=$?; fi
  retire_pid "$pid"
  [ "$rc" -eq 137 ] || fail "crash target $pid exit=$rc instead of 137"
}

CLEANED=0
cleanup() {
  [ "$CLEANED" = "1" ] && return 0
  CLEANED=1
  # Signal ONLY active-set PIDs that STILL prove current ownership: the
  # number exists AND is still a direct child of this shell. Reaped
  # numbers are already out of the set; a recycled number that now
  # belongs to someone else is skipped loudly — never signalled blind
  # (R6-F02: having created a pid once is not owning it now). R12-F01:
  # each owned member now goes through the BOUNDED KILL re-check above
  # and is reaped (exited) or reported as residue (owned/foreign/
  # unobservable) — the trap can no longer hang on an unreapable child.
  local pid cleanup_residue=0
  for pid in "${OWN_PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    case "$(child_state "$pid")" in
      owned)
        case "$(bounded_kill_stop_owned "$pid")" in
          exited)
            wait "$pid" 2>/dev/null || true
            ;;
          foreign)
            printf 'cleanup: pid %s 已不属于本脚本，不等待或发信号\n' "$pid" >&2 || true
            cleanup_residue=1
            ;;
          owned)
            printf 'cleanup: pid %s still OWNED after the KILL budget — RESIDUE left behind (unreapable?), no unbounded wait\n' "$pid" >&2 || true
            cleanup_residue=1
            ;;
          unobservable)
            printf 'cleanup: pid %s state UNOBSERVABLE after the KILL budget — not signalled further, no unbounded wait; possible residue\n' "$pid" >&2 || true
            cleanup_residue=1
            ;;
        esac
        ;;
      exited)
        wait "$pid" 2>/dev/null || true
        ;;
      foreign)
        printf 'cleanup: pid %s exists but is NOT currently owned by this shell — NOT signalled\n' "$pid" >&2 || true
        cleanup_residue=1
        ;;
      unobservable)
        printf 'cleanup: pid %s ownership UNOBSERVABLE (ps unreadable) — NOT signalled\n' "$pid" >&2 || true
        cleanup_residue=1
        ;;
    esac
  done
  OWN_PIDS=()
  # Only the directories THIS run created — a sibling run's subtree under
  # the same root is never matched, let alone deleted.
  if [ "$cleanup_residue" -eq 0 ]; then
    for home in "${OWN_HOMES[@]:-}"; do
      [ -n "$home" ] && [ -d "$home" ] && rm -rf -- "$home"
    done
    [ -d "$RUN_ROOT" ] && rm -rf -- "$RUN_ROOT"
  else
    echo "cleanup: 进程仍存活或归属不明，保留本轮 RUN_ROOT=$RUN_ROOT 供核查" >&2
    exit 1
  fi
  return 0
}
trap cleanup EXIT
trap 'cleanup; exit 143' TERM
trap 'cleanup; exit 130' INT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

note "== building lingxi-service + lingxi-storage-inspect (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
SERVICE_BIN="$TARGET_DIR/debug/lingxi-service"
INSPECT_BIN="$TARGET_DIR/debug/lingxi-storage-inspect"
[ -x "$SERVICE_BIN" ] || fail "service binary missing"
[ -x "$INSPECT_BIN" ] || fail "inspect binary missing"
note "PASS build (locked, offline)"

# run_probe <HOME> <TAG> — starts the binary, waits for READY or exit,
# records exit code / ready-line presence. Refusals exit 2 before binding.
run_probe() { # $1=home $2=tag -> sets PROBE_EXIT / PROBE_READY
  local state
  "$SERVICE_BIN" --home "$1" --bind 127.0.0.1:0 \
    > "$EVIDENCE_DIR/$2.out" 2> "$EVIDENCE_DIR/$2.err" &
  SERVICE_PID=$!
  record_pid "$SERVICE_PID"
  PROBE_READY=0
  for _ in $(seq 1 200); do
    if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/$2.out" 2>/dev/null; then
      PROBE_READY=1
      break
    fi
    kill -0 "$SERVICE_PID" 2>/dev/null || break
    sleep 0.05
  done
  if [ "$PROBE_READY" = "1" ]; then
    # started: stop it cleanly and collect the real exit code
    [ "$(child_state "$SERVICE_PID")" = "owned" ] || fail "$2: ready service no longer owned before TERM"
    state="$(bounded_term_stop_owned "$SERVICE_PID")"
    case "$state" in exited) ;; *) fail "$2: service remained or ownership changed after TERM/KILL budget (state=$state)" ;; esac
    if wait "$SERVICE_PID" 2>/dev/null; then PROBE_EXIT=0; else PROBE_EXIT=$?; fi
  else
    # 无 READY 时只能回收已退出的子进程，仍活着就是启动超时，交给 EXIT 有界清理。
    state="$(child_state "$SERVICE_PID")"
    case "$state" in exited) ;; *) fail "$2: startup never returned, changed ownership, or never became ready (state=$state)" ;; esac
    if wait "$SERVICE_PID" 2>/dev/null; then PROBE_EXIT=0; else PROBE_EXIT=$?; fi
  fi
  # R6-F02: the probe is REAPED — leave the active set immediately so a
  # later trap/residue check can never touch this number again.
  retire_pid "$SERVICE_PID"
  SERVICE_PID=""
}

hash_metadata() { # $1=home -> hashes of every pre-existing data file
  (cd "$1" 2>/dev/null && {
    for f in data-epoch.json data-epoch-transition.json \
             lingxi-service/data/runs.db lingxi-service/data/runs.db-wal \
             lingxi-service/data/runs.db-shm; do
      [ -f "$f" ] && shasum -a 256 "$f"
    done
  }) || true
}

craft_barrier_journal() { # $1=path $2=phase
  python3 - "$1" "$2" <<'PYEOF'
import json, sys
path, phase = sys.argv[1], sys.argv[2]
journal = {
    "schemaVersion": 1,
    "transitionId": "transition-1-2",
    "fromEpoch": 1,
    "toEpoch": 2,
    "migrationIds": ["preferences-1-to-2"],
    "affectedStoreIds": ["user-preferences"],
    "recoveryModes": {"preferences-1-to-2": "restore-only"},
    "phase": phase,
    "createdAt": "2026-09-26T08:00:00.000Z",
    "updatedAt": "2026-09-26T08:00:00.000Z",
    "lastVersion": "2.0.0",
}
if phase == "prepared":
    journal["checkpointId"] = None
    journal["checkpointReceipt"] = None
else:
    journal["checkpointId"] = "checkpoint-1-2"
    journal["checkpointReceipt"] = {"id": "checkpoint-1-2"}
with open(path, "w") as fh:
    json.dump(journal, fh, indent=2)
    fh.write("\n")
PYEOF
}

# ---- v1: torn stamp + readable barrier_raised journal (Node R7 mirror) ------
note "== v1 (R02-A12/drill): torn stamp + readable higher-epoch journal =="
V1=$(mktemp -d "$RUN_ROOT/v1-XXXXXX")
record_home "$V1"
printf '{ "epoch": 1, "dat' > "$V1/data-epoch.json"          # torn stamp
craft_barrier_journal "$V1/data-epoch-transition.json" barrier_raised
hash_metadata "$V1" > "$EVIDENCE_DIR/v1-hashes-before.txt"
run_probe "$V1" v1
[ "$PROBE_EXIT" = "2" ] || fail "v1 must refuse with exit 2, got $PROBE_EXIT"
[ "$PROBE_READY" = "0" ] || fail "v1 must not publish readiness"
grep -q "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE reason=corrupt-stamp" "$EVIDENCE_DIR/v1.err" \
  || fail "v1 stderr must carry the corrupt-stamp marker"
grep -q "evidence=transitionJournal(fromEpoch=1,toEpoch=2,phase=barrier_raised" "$EVIDENCE_DIR/v1.err" \
  || fail "v1 refusal must NAME the readable journal evidence"
grep -q "epochs 1→2" "$EVIDENCE_DIR/v1.err" \
  || fail "v1 refusal must state the higher-epoch target truthfully"
grep -q "no higher-epoch evidence was found" "$EVIDENCE_DIR/v1.err" \
  && fail "v1 must NOT print the incumbent lie"
[ ! -f "$V1/lingxi-service/local-token.json" ] || fail "v1: auth bootstrap must not run after a gate refusal"
hash_metadata "$V1" > "$EVIDENCE_DIR/v1-hashes-after.txt"
diff "$EVIDENCE_DIR/v1-hashes-before.txt" "$EVIDENCE_DIR/v1-hashes-after.txt" > /dev/null \
  || fail "v1: pre-existing files must be byte-identical after the refusal"
note "PASS v1 exit=2 reason=corrupt-stamp evidence 1→2 named; files byte-identical; no auth state written"

# ---- v2: valid stamp + torn journal (Node R9 mirror) -------------------------
note "== v2: valid stamp + torn transition journal =="
V2=$(mktemp -d "$RUN_ROOT/v2-XXXXXX")
record_home "$V2"
printf '{\n  "schemaVersion": 2,\n  "epoch": 1,\n  "minimumReaderEpoch": 1,\n  "committedDataEpoch": 1,\n  "lastVersion": "0.4.0",\n  "updatedAt": "2026-09-26T08:00:00.000Z"\n' > "$V2/data-epoch.json"   # torn: unterminated
printf '{ "schemaVersion": 1, "transitionId": "transition-1-2", "fro' > "$V2/data-epoch-transition.json"
hash_metadata "$V2" > "$EVIDENCE_DIR/v2-hashes-before.txt"
run_probe "$V2" v2
[ "$PROBE_EXIT" = "2" ] || fail "v2 must refuse with exit 2, got $PROBE_EXIT"
grep -q "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE reason=corrupt-journal" "$EVIDENCE_DIR/v2.err" \
  || fail "v2 stderr must carry the corrupt-journal marker"
[ ! -f "$V2/lingxi-service/local-token.json" ] || fail "v2: auth bootstrap must not run"
hash_metadata "$V2" > "$EVIDENCE_DIR/v2-hashes-after.txt"
diff "$EVIDENCE_DIR/v2-hashes-before.txt" "$EVIDENCE_DIR/v2-hashes-after.txt" > /dev/null \
  || fail "v2: files must be byte-identical after the refusal"
note "PASS v2 exit=2 reason=corrupt-journal; files byte-identical"

# ---- v3: MISSING stamp + readable barrier_raised journal (Node R10 mirror) ---
note "== v3 (headline fail-open shape): missing stamp + readable higher-epoch journal =="
V3=$(mktemp -d "$RUN_ROOT/v3-XXXXXX")
record_home "$V3"
craft_barrier_journal "$V3/data-epoch-transition.json" barrier_raised
hash_metadata "$V3" > "$EVIDENCE_DIR/v3-hashes-before.txt"
run_probe "$V3" v3
[ "$PROBE_EXIT" = "2" ] || fail "v3 must refuse with exit 2, got $PROBE_EXIT (the incumbent Node gate REALLY WROTE here)"
grep -q "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE reason=corrupt-transition" "$EVIDENCE_DIR/v3.err" \
  || fail "v3 stderr must carry the corrupt-transition marker"
grep -q "evidence=transitionJournal(fromEpoch=1,toEpoch=2,phase=barrier_raised" "$EVIDENCE_DIR/v3.err" \
  || fail "v3 refusal must NAME the readable journal evidence"
grep -q "no higher-epoch evidence was found" "$EVIDENCE_DIR/v3.err" \
  && fail "v3 must NOT print the incumbent lie"
[ ! -f "$V3/data-epoch.json" ] || fail "v3: no stamp may be invented for a refused home"
[ ! -f "$V3/lingxi-service/local-token.json" ] || fail "v3: auth bootstrap must not run"
hash_metadata "$V3" > "$EVIDENCE_DIR/v3-hashes-after.txt"
diff "$EVIDENCE_DIR/v3-hashes-before.txt" "$EVIDENCE_DIR/v3-hashes-after.txt" > /dev/null \
  || fail "v3: files must be byte-identical after the refusal"
note "PASS v3 exit=2 reason=corrupt-transition evidence 1→2 named; no stamp invented; files byte-identical"

# ---- v4: healthy control ------------------------------------------------------
note "== v4: healthy control (fresh home starts) =="
V4=$(mktemp -d "$RUN_ROOT/v4-XXXXXX")
record_home "$V4"
run_probe "$V4" v4
[ "$PROBE_READY" = "1" ] || fail "v4 healthy home must start (err: $(tail -2 "$EVIDENCE_DIR/v4.err"))"
[ "$PROBE_EXIT" = "0" ] || fail "v4 healthy service must stop cleanly (exit 0), got $PROBE_EXIT"
python3 - "$V4/data-epoch.json" <<'PYEOF' || fail "v4 stamp must be v2 epoch 1"
import json, sys
stamp = json.load(open(sys.argv[1]))
assert stamp["schemaVersion"] == 2, stamp
assert stamp["minimumReaderEpoch"] == 1 and stamp["committedDataEpoch"] == 1, stamp
PYEOF
note "PASS v4 fresh home stamped (v2, epoch 1) and served; graceful stop exit 0"

# ---- v5: garbage main db file -------------------------------------------------
note "== v5 (A12): garbage main db file =="
V5=$(mktemp -d "$RUN_ROOT/v5-XXXXXX")
record_home "$V5"
mkdir -p "$V5/lingxi-service/data"
python3 - <<PYEOF
import os
blob = bytes((i % 251) for i in range(8192))
blob = b"GARBAGE-NOT-A-DB" + blob[16:]
open("$V5/lingxi-service/data/runs.db", "wb").write(blob)
PYEOF
shasum -a 256 "$V5/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v5-hashes-before.txt"
run_probe "$V5" v5
[ "$PROBE_EXIT" = "2" ] || fail "v5 must refuse with exit 2, got $PROBE_EXIT"
grep -Eq "Corrupted|not a database|bootstrap failed" "$EVIDENCE_DIR/v5.err" \
  || fail "v5 stderr must name the corruption"
shasum -a 256 "$V5/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v5-hashes-after.txt"
diff "$EVIDENCE_DIR/v5-hashes-before.txt" "$EVIDENCE_DIR/v5-hashes-after.txt" > /dev/null \
  || fail "v5: the corrupt file must be preserved byte-identical (no empty-db substitution)"
note "PASS v5 exit=2 explicit corruption refusal; corrupt file byte-identical; no empty db substituted"

# ---- v6 (INFO row): torn WAL tail — recovery by design ------------------------
# Observed semantics (probe recorded in the report): after a REAL crash, an
# incomplete trailing WAL frame is the exact "transaction never committed"
# case SQLite is designed to recover from — recovery drops it and the
# database stays transaction-consistent (frames are only applied up to a
# commit frame; no half transaction can ever appear). This row therefore
# records the actual outcome and asserts the CONSISTENCY invariant instead
# of a refusal; the A12 refusal forms are v1/v2/v5/v7 (+v3 from the drill).
note "== v6 (INFO row): torn WAL tail (SQLite recovery by design) =="
V6=$(mktemp -d "$RUN_ROOT/v6-XXXXXX")
record_home "$V6"
mkdir -p "$V6/lingxi-service/data"
# Seed via the REAL service and crash it (kill -9) so a REAL WAL with
# committed-but-uncheckpointed data survives on disk.
"$SERVICE_BIN" --home "$V6" --bind 127.0.0.1:0 > "$EVIDENCE_DIR/v6-seed.out" 2> "$EVIDENCE_DIR/v6-seed.err" &
SEED_PID=$!
record_pid "$SEED_PID"
for _ in $(seq 1 200); do
  grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/v6-seed.out" 2>/dev/null && break
  kill -0 "$SEED_PID" 2>/dev/null || break
  sleep 0.05
done
if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/v6-seed.out" 2>/dev/null; then
  TOKEN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' "$V6/lingxi-service/local-token.json")
  ADDR=$(sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' "$EVIDENCE_DIR/v6-seed.out" | head -1)
  curl -sS -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/json' -d '{"input":"v6 wal seed"}' \
    "http://$ADDR/lingxi/v1/sessions/sess_local_alpha/execute" \
    > "$EVIDENCE_DIR/v6-seed-execute.txt" || true
fi
crash_active_pid "$SEED_PID"
SEED_PID=""
[ -s "$V6/lingxi-service/data/runs.db-wal" ] || fail "v6 setup: WAL not present after the crash seeding"
python3 - "$V6/lingxi-service/data/runs.db-wal" <<'PYEOF'
import sys
p = sys.argv[1]
data = open(p, "rb").read()
open(p, "wb").write(data[:-11])  # tear mid-frame at the tail
PYEOF
run_probe "$V6" v6
if [ "$PROBE_READY" = "1" ]; then
  # Recovery by design: the recovered view must be transaction-consistent —
  # a run is either fully terminal (row + its terminal event) or not
  # terminal at all; no half commit may exist.
  "$INSPECT_BIN" "$V6/lingxi-service/data/runs.db" dump > "$EVIDENCE_DIR/v6-recovered-dump.jsonl" 2>/dev/null
  python3 - "$EVIDENCE_DIR/v6-recovered-dump.jsonl" <<'PYEOF' || fail "v6: recovered view is NOT transaction-consistent"
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip().startswith("{")]
runs = {r["row"][0]: r["row"][6] for r in rows if r["table"] == "runs"}
events = [r for r in rows if r["table"] == "key_events"]
for run_id, status in runs.items():
    done = any(r["row"][0] == f"{run_id}-done" for r in events)
    if status == "completed" and not done:
        raise SystemExit(f"half terminal state for {run_id}: completed row without its event")
    if status != "completed" and done:
        raise SystemExit(f"half terminal state for {run_id}: event without terminal row")
print(f"runs={len(runs)} events={len(events)} transaction-consistent")
PYEOF
  note "v6 observed: service started after WAL recovery (by design); recovered view transaction-consistent; graceful stop exit=$PROBE_EXIT"
else
  # A refusal is equally acceptable (SQLite build differences): explicit failure.
  [ "$PROBE_EXIT" = "2" ] || fail "v6: unexpected outcome (exit=$PROBE_EXIT)"
  note "v6 observed: explicit refusal (exit 2) on the torn WAL"
fi

# ---- v7: tampered migration receipt -------------------------------------------
note "== v7 (A12): tampered migration receipt =="
V7=$(mktemp -d "$RUN_ROOT/v7-XXXXXX")
record_home "$V7"
mkdir -p "$V7/lingxi-service/data"
# Seed a healthy db by running the service once (graceful stop).
run_probe "$V7" v7-seed
[ "$PROBE_READY" = "1" ] || fail "v7 seed start failed"
sqlite3 "$V7/lingxi-service/data/runs.db" \
  "UPDATE schema_migrations SET fingerprint='deadbeef' WHERE version=1" \
  || fail "v7: cannot tamper the receipt (sqlite3 CLI missing?)"
shasum -a 256 "$V7/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v7-hashes-before.txt"
run_probe "$V7" v7
[ "$PROBE_EXIT" = "2" ] || fail "v7 must refuse with exit 2, got $PROBE_EXIT"
grep -Eq "SchemaTampered|fingerprint|tampered" "$EVIDENCE_DIR/v7.err" \
  || fail "v7 stderr must name the schema tampering"
shasum -a 256 "$V7/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/v7-hashes-after.txt"
diff "$EVIDENCE_DIR/v7-hashes-before.txt" "$EVIDENCE_DIR/v7-hashes-after.txt" > /dev/null \
  || fail "v7: the tampered db must be preserved byte-identical"
note "PASS v7 exit=2 SchemaTampered refusal; file byte-identical"

# ---- v8 (INFO): garbage WAL header — recovery by design -----------------------
note "== v8 (INFO row): garbage WAL header (SQLite recovery semantics) =="
V8=$(mktemp -d "$RUN_ROOT/v8-XXXXXX")
record_home "$V8"
mkdir -p "$V8/lingxi-service/data"
"$SERVICE_BIN" --home "$V8" --bind 127.0.0.1:0 > "$EVIDENCE_DIR/v8-seed.out" 2> "$EVIDENCE_DIR/v8-seed.err" &
SEED_PID=$!
record_pid "$SEED_PID"
for _ in $(seq 1 200); do
  grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/v8-seed.out" 2>/dev/null && break
  kill -0 "$SEED_PID" 2>/dev/null || break
  sleep 0.05
done
TOKEN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' "$V8/lingxi-service/local-token.json" 2>/dev/null || echo "")
if [ -n "$TOKEN" ]; then
  ADDR=$(sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' "$EVIDENCE_DIR/v8-seed.out" | head -1)
  curl -sS -o /dev/null -X POST -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/json' -d '{"input":"v8 wal seed"}' \
    "http://$ADDR/lingxi/v1/sessions/sess_local_alpha/execute" || true
fi
crash_active_pid "$SEED_PID"
SEED_PID=""
if [ -s "$V8/lingxi-service/data/runs.db-wal" ]; then
  python3 - "$V8/lingxi-service/data/runs.db-wal" <<'PYEOF'
import sys
p = sys.argv[1]
data = bytearray(open(p, "rb").read())
data[0:4] = b"\xde\xad\xbe\xef"   # garbage magic, size preserved
open(p, "wb").write(bytes(data))
PYEOF
  run_probe "$V8" v8
  note "v8 observed: exit=$PROBE_EXIT ready=$PROBE_READY (SQLite may recover or refuse; files: $(ls "$V8/lingxi-service/data" | tr '\n' ' '))"
  cat "$EVIDENCE_DIR/v8.err" >> "$EVIDENCE_DIR/v8-observed.err" || true
else
  note "v8 skipped: no WAL survived seeding"
fi

# ---- residue check (R5-F03: OWN pids and OWN homes only) ----------------------
# R9-F02 honesty note: this block is a SELF-CONTAINED fixture comparison.
# `cleanup_groupA` below is an ad-hoc removal of exactly these two fixture
# paths — it is NOT this run's real cleanup routine (the EXIT trap walking
# the OWN_HOMES/RUN_ROOT ledgers), and this block must never be cited as
# exercising or proving the real cleanup path's ownership logic (that runs
# only at exit). What it does prove: the LITERAL paths this block removes
# are removed, and a sibling fixture's bytes/process are untouched by THIS
# block. Group A stands in for "the homes this run owns", group B for a
# concurrent sibling run home + process under the same root: after the
# ad-hoc removal of group A, group B must be byte-identical and its
# process untouched — then the fixture (ours) is reaped by hand.
COEX="$RUN_ROOT/coexist-fixtures"
mkdir -p "$COEX/groupA-home-1" "$COEX/groupA-home-2" "$COEX/groupB-sibling-home"
printf 'fixture-a-1\n' > "$COEX/groupA-home-1/sentinel.bin"
printf 'fixture-a-2\n' > "$COEX/groupA-home-2/sentinel.bin"
printf 'fixture-b-sibling-%s\n' "$(date +%s%N)" > "$COEX/groupB-sibling-home/sentinel.bin"
shasum -a 256 "$COEX/groupB-sibling-home/sentinel.bin" > "$EVIDENCE_DIR/coexist-before.txt"
sleep 30 &
SIBLING_PID=$!
record_pid "$SIBLING_PID"
cleanup_groupA() {
  rm -rf -- "$COEX/groupA-home-1" "$COEX/groupA-home-2"
}
cleanup_groupA
[ ! -e "$COEX/groupA-home-1" ] && [ ! -e "$COEX/groupA-home-2" ] \
  || fail "coexist: this run's own group-A homes were not removed"
shasum -a 256 "$COEX/groupB-sibling-home/sentinel.bin" > "$EVIDENCE_DIR/coexist-after.txt"
cmp -s "$EVIDENCE_DIR/coexist-before.txt" "$EVIDENCE_DIR/coexist-after.txt" \
  || fail "coexist: a sibling run's home bytes changed during this run's cleanup"
kill -0 "$SIBLING_PID" 2>/dev/null \
  || fail "coexist: a sibling run's process was killed by this run's cleanup"
note "PASS coexist-boundary (ad-hoc fixture comparison ONLY: literal group-A paths removed; sibling home byte-identical; sibling process alive — this is NOT a run of the real cleanup routine, see the block comment; R9-F02)"
# The sibling fixture belongs to THIS run — reap it here (never a global
# pattern) and retire it from the active set (R6-F02).
[ "$(child_state "$SIBLING_PID")" = "owned" ] || fail "coexist sibling no longer owned before TERM"
SIBLING_STATE="$(bounded_term_stop_owned "$SIBLING_PID")"
case "$SIBLING_STATE" in exited) ;; *) fail "coexist sibling remained or ownership changed after TERM/KILL budget (state=$SIBLING_STATE)" ;; esac
wait "$SIBLING_PID" 2>/dev/null || true
retire_pid "$SIBLING_PID"

# Residue (R6-F02): the ACTIVE set must be EMPTY here — every spawn was
# reaped and retired on its own path. The old check kill-0-probed the
# full HISTORY and would misfire the moment the OS recycled one of those
# numbers into an unrelated process; the active set is the honest ledger
# (only objects we still own), so an empty set means all $SPAWN_TOTAL
# spawns were collected, with no blind probing of retired numbers.
if [ "${#OWN_PIDS[@]}" -ne 0 ]; then
  for pid in "${OWN_PIDS[@]:-}"; do
    [ -n "$pid" ] && echo "unretired spawn: $pid" >&2
  done
  fail "spawned PIDs not all reaped/retired by their own paths: ${#OWN_PIDS[@]} still active"
fi
note "PASS residue-check (active PID set empty: $SPAWN_TOTAL spawned, all reaped and retired; no retired number probed or signalled)"
note "RESULT: R02-T06 recovery drill + A12 corruption matrix ALL GREEN (v8 informational; homes cleaned via own-path list: ${#OWN_HOMES[@]} tracked)"
