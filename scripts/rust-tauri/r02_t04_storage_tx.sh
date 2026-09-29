#!/usr/bin/env bash
# R02-T04 / acceptances R02-A07 + R02-A08 — storage ports & run-database
# transactions against the REAL lingxi-service binary and the REAL
# runs.db files (not in-process mocks).
#
# Proves (A07 提交失败无假成功, binary/file level):
#   - a data directory that cannot be written makes startup FAIL loudly
#     (exit 2, explicit storage error) — never a silent in-memory
#     fallback that would later claim success;
#   - a crash (kill -9) right after a successful execute leaves NO half
#     terminal state: restart reads the same committed run count and the
#     inspect queries show run=completed TOGETHER WITH its key events
#     (same-transaction semantics; the crash-window distinction is
#     additionally proven in-process by tests/disk_full_fault.rs +
#     tests/storage_transactions.rs with real kernel-level write faults).
# Proves (A08 重放迁移幂等, binary/file level):
#   - three consecutive migration checks (service restarts) keep the
#     schema version, fingerprints and row counts byte-stable;
#   - the inspector is read-only (db file hashes unchanged around it).
# Proves (R02-T04 wiring): startup -> authenticated execute -> graceful
# stop -> restart -> the run facts come back from the database.
#
# Environment guards: rustup-locked toolchain 1.98.1, task-dedicated
# target dir, offline locked build, proxy vars stripped, synthetic /tmp
# home only, no real user directory touched.
#
# Usage: scripts/rust-tauri/r02_t04_storage_tx.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T04}"
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
TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r02-t04}"

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

HOME_DIR=""
SERVICE_PID=""
# R02 stage-repair R7 / R7-F02: SERVICE_PID is the CURRENT handle — set on
# spawn/reload, RETIRED (cleared) after every wait/reap (the normal stop
# paths already did; a wait returning non-zero can abort under set -e
# AFTER reaping). The trap KILL-9s it ONLY while it still proves CURRENT
# ownership (exists AND ppid is THIS shell): a retired or recycled number
# is never signalled (R6-F02 A12 pattern).
# R12-F01: the boolean probe's false branch conflated exited/foreign/
# unobservable, and the trap's owned branch was `kill -9; wait` with NO
# deadline — KILL cannot be ignored, but an unreapable direct child
# (D-state) would still hang the trap forever. Four-state probe + a
# bounded KILL re-check below (≈5 s worst case per handle): residue is
# reported loudly at expiry, never an unbounded wait. KILL-9 stays the
# FIRST signal by this script's own contract (the S2 crash-stop
# semantics); no TERM stage is inserted.
child_state() {
  # child_state <pid> → exited | owned | foreign | unobservable
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
# R12-F01: bounded crash-stop for ONE provably-owned handle: KILL-9 (the
# contractual first signal here — there is no gentler stage to try and
# KILL cannot be ignored) → ≤5 s re-check poll. Prints the final state;
# an unreapable survivor is reported as residue, never waited on.
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
# 数据路径的正常停机须先 TERM，并在有限时间后仅对仍属本脚本的子进程 KILL。
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
  if [ "${state:-owned}" = "owned" ]; then
    state="$(bounded_kill_stop_owned "$pid")"
  fi
  printf '%s\n' "${state:-unobservable}"
}
stop_service_gracefully() {
  local state
  [ "$(child_state "$SERVICE_PID")" = "owned" ] || fail "service not owned before graceful stop"
  state="$(bounded_term_stop_owned "$SERVICE_PID")"
  case "$state" in exited) ;; *) fail "service remained or ownership changed after TERM/KILL budget (state=$state)" ;; esac
  if wait "$SERVICE_PID" 2>/dev/null; then SERVICE_STOP_RC=0; else SERVICE_STOP_RC=$?; fi
  SERVICE_PID=""
  [ "$SERVICE_STOP_RC" -eq 0 ] || fail "service did not stop cleanly (exit=$SERVICE_STOP_RC)"
}
crash_service_owned() {
  local state
  [ "$(child_state "$SERVICE_PID")" = "owned" ] || fail "service not owned before crash signal"
  state="$(bounded_kill_stop_owned "$SERVICE_PID")"
  case "$state" in exited) ;; *) fail "service remained or ownership changed after KILL budget (state=$state)" ;; esac
  if wait "$SERVICE_PID" 2>/dev/null; then SERVICE_STOP_RC=0; else SERVICE_STOP_RC=$?; fi
  SERVICE_PID=""
  [ "$SERVICE_STOP_RC" -eq 137 ] || fail "crash signal expected exit 137, got $SERVICE_STOP_RC"
}
cleanup() {
  local cleanup_residue=0
  if [ -n "$SERVICE_PID" ]; then
    case "$(child_state "$SERVICE_PID")" in
      owned)
        case "$(bounded_kill_stop_owned "$SERVICE_PID")" in
          exited)
            wait "$SERVICE_PID" 2>/dev/null || true
            ;;
          foreign)
            echo "cleanup: pid $SERVICE_PID 已不属于本脚本，不等待或发信号" >&2
            cleanup_residue=1
            ;;
          owned)
            echo "cleanup: pid $SERVICE_PID still OWNED after the KILL budget — RESIDUE left behind (unreapable?), no unbounded wait" >&2
            cleanup_residue=1
            ;;
          unobservable)
            echo "cleanup: pid $SERVICE_PID state UNOBSERVABLE after the KILL budget — not signalled further, no unbounded wait; possible residue" >&2
            cleanup_residue=1
            ;;
        esac
        ;;
      exited)
        wait "$SERVICE_PID" 2>/dev/null || true
        ;;
      foreign)
        echo "cleanup: pid $SERVICE_PID is NOT currently owned by this shell — NOT signalled" >&2
        cleanup_residue=1
        ;;
      unobservable)
        echo "cleanup: pid $SERVICE_PID ownership UNOBSERVABLE (ps unreadable) — NOT signalled" >&2
        cleanup_residue=1
        ;;
    esac
    SERVICE_PID=""
  fi
  if [ "$cleanup_residue" -eq 0 ]; then
    if [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ]; then rm -rf "$HOME_DIR"; fi
  else
    echo "cleanup: 进程仍存活或归属不明，保留本轮 home=$HOME_DIR 供核查" >&2
    exit 1
  fi
  return 0
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

# ---- build the real binaries ------------------------------------------------
note "== building lingxi-service + lingxi-storage-inspect (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
SERVICE_BIN="$TARGET_DIR/debug/lingxi-service"
INSPECT_BIN="$TARGET_DIR/debug/lingxi-storage-inspect"
[ -x "$SERVICE_BIN" ] || fail "service binary missing"
[ -x "$INSPECT_BIN" ] || fail "inspect binary missing"
note "PASS build (locked, offline)"

# ---- helpers -----------------------------------------------------------------
# start_service $1=home $2=tag
# Starts the service as a CHILD OF THIS SHELL (no command substitution:
# `wait` only works for direct children), writes the pid to service.pid
# and the bound address to service.addr once the readiness line appears.
start_service() {
  rm -f "$EVIDENCE_DIR/service.addr"
  "$SERVICE_BIN" --home "$1" --bind 127.0.0.1:0 \
    > "$EVIDENCE_DIR/service-$2.out" 2> "$EVIDENCE_DIR/service-$2.err" &
  SERVICE_PID=$!
  echo "$SERVICE_PID" > "$EVIDENCE_DIR/service.pid"
  for _ in $(seq 1 200); do
    if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/service-$2.out" 2>/dev/null; then
      sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' \
        "$EVIDENCE_DIR/service-$2.out" | head -n 1 > "$EVIDENCE_DIR/service.addr"
      return 0
    fi
    if ! kill -0 "$SERVICE_PID" 2>/dev/null; then
      return 1
    fi
    sleep 0.05
  done
  return 1
}

token_of() { # $1=home
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' \
    "$1/lingxi-service/local-token.json"
}

http() { # $1=addr $2=method $3=path $4=token $5=body(optional)
  if [ $# -ge 5 ]; then
    curl -sS -o /dev/null -w '%{http_code}' -X "$2" \
      -H "Authorization: Bearer $4" -H 'Content-Type: application/json' \
      -d "$5" "http://$1$3"
  else
    curl -sS -o /dev/null -w '%{http_code}' -X "$2" \
      -H "Authorization: Bearer $4" "http://$1$3"
  fi
}

get_json() { # $1=addr $2=path $3=token
  curl -sS -H "Authorization: Bearer $3" "http://$1$2"
}

db_files_sha() { # $1=home -> stable digest of the db file set
  (cd "$1/lingxi-service/data" && ls runs.db* 2>/dev/null | sort | \
   xargs sh -c 'for f; do shasum -a 256 "$f"; done' _) | tee "$EVIDENCE_DIR/last-db-hashes.txt" >/dev/null
  (cd "$1/lingxi-service/data" && cat runs.db* 2>/dev/null | shasum -a 256)
}

# ---- S1: startup -> execute -> graceful stop -> restart ---------------------
note "== S1: full lifecycle over the real database =="
HOME_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r02t04-bin-XXXXXX")
DB="$HOME_DIR/lingxi-service/data/runs.db"
start_service "$HOME_DIR" s1 || fail "service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service.addr")
TOKEN=$(token_of "$HOME_DIR")

CODE=$(http "$ADDR" POST /lingxi/v1/sessions/sess_local_alpha/execute "$TOKEN" '{"input":"binary evidence run 1"}')
[ "$CODE" = "200" ] || fail "S1 execute expected 200, got $CODE"
note "PASS S1 execute accepted (http=200)"

[ -n "$SERVICE_PID" ] || fail "S1 service handle missing"
stop_service_gracefully
note "PASS S1 graceful stop (exit 0, WAL checkpointed)"

"$INSPECT_BIN" "$DB" counts > "$EVIDENCE_DIR/s1-counts-after-stop.json"
grep -q '"runs": 1' "$EVIDENCE_DIR/s1-counts-after-stop.json" \
  || fail "S1 expected exactly 1 run row after stop"
grep -q '"key_events": 2' "$EVIDENCE_DIR/s1-counts-after-stop.json" \
  || fail "S1 expected 2 key events (start + terminal)"
note "PASS S1 inspect: runs=1 key_events=2 (terminal+event committed together)"

# Restart: the run facts must come back from the DATABASE.
start_service "$HOME_DIR" s1b || fail "restart did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service.addr")
TOKEN=$(token_of "$HOME_DIR")   # the loopback token rotates per start
RUN_COUNT=$(get_json "$ADDR" /lingxi/v1/sessions/sess_local_alpha "$TOKEN" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["runCount"])')
[ "$RUN_COUNT" = "1" ] || fail "S1 restart expected runCount=1, got $RUN_COUNT"
[ -n "$SERVICE_PID" ] || fail "S1 restart service handle missing"
stop_service_gracefully
note "PASS S1 restart reads runCount=1 from the database"

# ---- S2: crash (kill -9) after a committed execute --------------------------
note "== S2: kill -9 after committed execute — no half terminal state =="
start_service "$HOME_DIR" s2 || fail "s2 service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service.addr")
TOKEN=$(token_of "$HOME_DIR")
CODE=$(http "$ADDR" POST /lingxi/v1/sessions/sess_local_beta/execute "$TOKEN" '{"input":"crash window run"}')
[ "$CODE" = "200" ] || fail "S2 execute expected 200, got $CODE"
[ -n "$SERVICE_PID" ] || fail "S2 service handle missing"
crash_service_owned
note "PASS S2 crash delivered (kill -9 right after the committed execute)"

"$INSPECT_BIN" "$DB" dump --table runs > "$EVIDENCE_DIR/s2-runs-dump.jsonl"
"$INSPECT_BIN" "$DB" counts > "$EVIDENCE_DIR/s2-counts-after-crash.json"
grep -q '"completed"' "$EVIDENCE_DIR/s2-runs-dump.jsonl" \
  || fail "S2 the committed run must be terminal=completed in the file"
RUN2_EVENTS=$("$INSPECT_BIN" "$DB" dump --table key_events \
  | python3 -c '
import json,sys
rows=[json.loads(l)["row"] for l in sys.stdin if l.strip()]
done=[r for r in rows if r[0].endswith("-done")]
print(len(done))')
[ "$RUN2_EVENTS" -ge 2 ] || fail "S2 expected the completion events of both runs durable, got $RUN2_EVENTS"
note "PASS S2 inspect: terminal rows and completion events coexist (same transaction, crash-safe)"

start_service "$HOME_DIR" s2b || fail "s2b service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service.addr")
TOKEN=$(token_of "$HOME_DIR")   # rotated per start
BETA=$(get_json "$ADDR" /lingxi/v1/sessions/sess_local_beta "$TOKEN" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["runCount"])')
[ "$BETA" = "1" ] || fail "S2 restart expected beta runCount=1, got $BETA"
[ -n "$SERVICE_PID" ] || fail "S2 restart service handle missing"
stop_service_gracefully
note "PASS S2 restart recovers the committed-but-crashed run (runCount=1)"

# ---- S3: unwritable data dir -> loud startup refusal ------------------------
note "== S3: real IO fault at startup — loud refusal, no fallback =="
FAULT_HOME=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r02t04-fault-XXXXXX")
chmod 0555 "$FAULT_HOME"
set +e
"$SERVICE_BIN" --home "$FAULT_HOME" --bind 127.0.0.1:0 \
  > "$EVIDENCE_DIR/s3-fault.out" 2> "$EVIDENCE_DIR/s3-fault.err"
FAULT_EXIT=$?
set -e
chmod 0755 "$FAULT_HOME"; rm -rf "$FAULT_HOME"
[ "$FAULT_EXIT" = "2" ] || fail "S3 unwritable home must exit 2, got $FAULT_EXIT"
grep -Eq "bootstrap failed|cannot create data root|Permission denied" "$EVIDENCE_DIR/s3-fault.err" \
  || fail "S3 stderr must name the IO/bootstrap failure"
grep -q "LINGXI_SERVICE_READY" "$EVIDENCE_DIR/s3-fault.out" \
  && fail "S3 must NOT publish readiness"
note "PASS S3 unwritable home: exit=2, explicit error, no readiness line"

# ---- S4 (A08): migration idempotency at the binary level --------------------
note "== S4 (A08): three migration checks, stable version/counts/fingerprints =="
db_files_sha "$HOME_DIR" > "$EVIDENCE_DIR/s4-hashes-before.txt"
BASE_COUNTS="$("$INSPECT_BIN" "$DB" counts)"
# R03-T08 fix of the deferred T04 R1-F1: the expected migration set is the
# REGISTERED set (docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json), not a
# hardcoded version==1. The registry is the fingerprint authority; the
# binary's own compiledIn set must equal it, and the on-disk receipts must
# equal both.
REGISTRY_MIGRATIONS="$(python3 - << 'REGEOF'
import json
doc = json.load(open("docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json"))
migs = doc["new_persistence_points"][0]["migrations"]
print(json.dumps([
    {"version": m["version"], "name": m["name"],
     "fingerprint": m["fingerprint_sha256"]} for m in migs
]))
REGEOF
)" || fail "S4 cannot read the storage registry"
for round in 1 2 3; do
  # each start re-runs the open-time migration pass
  start_service "$HOME_DIR" "s4-$round" || fail "s4 round $round service did not become ready"
  stop_service_gracefully
  MIG="$("$INSPECT_BIN" "$DB" migrations)"
  echo "$MIG" > "$EVIDENCE_DIR/s4-migrations-round$round.json"
  echo "$BASE_COUNTS" | diff -q - <("$INSPECT_BIN" "$DB" counts) >/dev/null \
    || fail "S4 round $round: row counts changed"
  REGISTRY_MIGRATIONS="$REGISTRY_MIGRATIONS" python3 - "$EVIDENCE_DIR/s4-migrations-round$round.json" << 'PYEOF' || fail "S4 round $round: migration check inconsistent"
import json, os, sys
doc = json.load(open(sys.argv[1]))
registered = json.loads(os.environ["REGISTRY_MIGRATIONS"])
registered.sort(key=lambda m: m["version"])
supported = len(registered)
assert doc["userVersion"] == doc["supportedVersion"] == supported, (
    f"userVersion {doc['userVersion']} / supportedVersion {doc['supportedVersion']} "
    f"disagree with the registry's {supported} migrations"
)
receipts = doc["receipts"]
compiled = doc["compiledIn"]
assert len(receipts) == len(compiled) == supported, (
    f"expected {supported} receipts, got receipts={len(receipts)} compiled={len(compiled)}"
)
assert [r["version"] for r in receipts] == [m["version"] for m in registered], (
    "receipt versions disagree with the registry"
)
for r, m in zip(receipts, registered):
    assert r["version"] == m["version"], (r, m)
    assert r["name"] == m["name"], (r, m)
    assert r["fingerprint"] == m["fingerprint"], (
        f"receipt v{r['version']} fingerprint {r['fingerprint']} != registry {m['fingerprint']}"
    )
for c, m in zip(compiled, registered):
    assert c["version"] == m["version"] and c["name"] == m["name"], (c, m)
    assert c["fingerprint"] == m["fingerprint"], (
        f"compiledIn v{c['version']} fingerprint {c['fingerprint']} != registry {m['fingerprint']}"
    )
PYEOF
done
note "PASS S4 3x migration checks: registry-conformant version set, identical fingerprints, row counts stable"

# The inspector is read-only: file hashes unchanged around all inspections.
db_files_sha "$HOME_DIR" > "$EVIDENCE_DIR/s4-hashes-after.txt"
diff "$EVIDENCE_DIR/s4-hashes-before.txt" "$EVIDENCE_DIR/s4-hashes-after.txt" >/dev/null \
  || fail "S4 the inspector must not modify the database files"
note "PASS S4 inspect is read-only (db file hashes unchanged)"

# ---- residue check -----------------------------------------------------------
LEFT=$( { pgrep -f "lingxi-service --home $HOME_DIR" || true; } | wc -l | tr -d ' ')
[ "$LEFT" = "0" ] || fail "leftover service processes: $LEFT"
note "PASS no leftover processes"

rm -rf "$HOME_DIR"
note "RESULT: R02-T04 binary evidence ALL GREEN"
