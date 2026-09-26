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
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t04}"

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
cleanup() {
  if [ -n "$SERVICE_PID" ] && kill -0 "$SERVICE_PID" 2>/dev/null; then
    kill -9 "$SERVICE_PID" 2>/dev/null || true
    wait "$SERVICE_PID" 2>/dev/null || true
  fi
  if [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ]; then rm -rf "$HOME_DIR"; fi
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

wait_exit() { # $1=pid $2=timeout-50ms-ticks
  for _ in $(seq 1 "$2"); do
    kill -0 "$1" 2>/dev/null || wait "$1" 2>/dev/null && return 0
    sleep 0.05
  done
  return 1
}

# ---- S1: startup -> execute -> graceful stop -> restart ---------------------
note "== S1: full lifecycle over the real database =="
HOME_DIR=$(mktemp -d /tmp/lingxi-r02t04-bin-XXXXXX)
DB="$HOME_DIR/lingxi-service/data/runs.db"
start_service "$HOME_DIR" s1 || fail "service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service.addr")
TOKEN=$(token_of "$HOME_DIR")

CODE=$(http "$ADDR" POST /lingxi/v1/sessions/sess_local_alpha/execute "$TOKEN" '{"input":"binary evidence run 1"}')
[ "$CODE" = "200" ] || fail "S1 execute expected 200, got $CODE"
note "PASS S1 execute accepted (http=200)"

SERVICE_PID=$(cat "$EVIDENCE_DIR/service.pid")
kill -TERM "$SERVICE_PID"; wait "$SERVICE_PID" 2>/dev/null
EXIT=$?
[ "$EXIT" = "0" ] || fail "S1 graceful stop expected exit 0, got $EXIT"
SERVICE_PID=""
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
SERVICE_PID=$(cat "$EVIDENCE_DIR/service.pid")
kill -TERM "$SERVICE_PID"; wait "$SERVICE_PID" 2>/dev/null; SERVICE_PID=""
note "PASS S1 restart reads runCount=1 from the database"

# ---- S2: crash (kill -9) after a committed execute --------------------------
note "== S2: kill -9 after committed execute — no half terminal state =="
start_service "$HOME_DIR" s2 || fail "s2 service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service.addr")
TOKEN=$(token_of "$HOME_DIR")
CODE=$(http "$ADDR" POST /lingxi/v1/sessions/sess_local_beta/execute "$TOKEN" '{"input":"crash window run"}')
[ "$CODE" = "200" ] || fail "S2 execute expected 200, got $CODE"
SERVICE_PID=$(cat "$EVIDENCE_DIR/service.pid")
kill -9 "$SERVICE_PID"; wait "$SERVICE_PID" 2>/dev/null || true; SERVICE_PID=""
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
SERVICE_PID=$(cat "$EVIDENCE_DIR/service.pid")
kill -TERM "$SERVICE_PID"; wait "$SERVICE_PID" 2>/dev/null; SERVICE_PID=""
note "PASS S2 restart recovers the committed-but-crashed run (runCount=1)"

# ---- S3: unwritable data dir -> loud startup refusal ------------------------
note "== S3: real IO fault at startup — loud refusal, no fallback =="
FAULT_HOME=$(mktemp -d /tmp/lingxi-r02t04-fault-XXXXXX)
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
for round in 1 2 3; do
  # each start re-runs the open-time migration pass
  start_service "$HOME_DIR" "s4-$round" || fail "s4 round $round service did not become ready"
  kill -TERM "$SERVICE_PID"; wait "$SERVICE_PID" 2>/dev/null; SERVICE_PID=""
  MIG="$("$INSPECT_BIN" "$DB" migrations)"
  echo "$MIG" > "$EVIDENCE_DIR/s4-migrations-round$round.json"
  echo "$BASE_COUNTS" | diff -q - <("$INSPECT_BIN" "$DB" counts) >/dev/null \
    || fail "S4 round $round: row counts changed"
  python3 - "$EVIDENCE_DIR/s4-migrations-round$round.json" << 'PYEOF' || fail "S4 round $round: migration check inconsistent"
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc["userVersion"] == doc["supportedVersion"] == 1, doc
receipts = doc["receipts"]; compiled = doc["compiledIn"]
assert len(receipts) == len(compiled) == 1, doc
assert receipts[0]["fingerprint"] == compiled[0]["fingerprint"], doc
assert receipts[0]["version"] == compiled[0]["version"] == 1, doc
PYEOF
done
note "PASS S4 3x migration checks: version=1, fingerprint identical, row counts stable"

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
