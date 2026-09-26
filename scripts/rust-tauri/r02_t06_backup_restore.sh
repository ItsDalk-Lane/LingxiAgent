#!/usr/bin/env bash
# R02-T06 / acceptance R02-A11 — WAL-state backup & restore against the
# REAL lingxi-service binary, REAL runs.db files and the REAL SQLite
# Online Backup API (via lingxi-storage-inspect backup / restore-verify,
# which call lingxi_adapters::storage::backup directly).
#
# Proves (binary/file level):
#   S1  precondition: committed-but-uncheckpointed data (WAL non-empty);
#       crash stop (kill -9) leaves the WAL intact; the backup captures the
#       WAL-resident committed data; restore into ANOTHER directory passes
#       the full recovery open (receipts + integrity) and the logical dump
#       of the restored copy equals the source (transaction boundaries:
#       everything committed at backup time is present, nothing partial);
#   S2  a graceful stop (SIGTERM) with the new shutdown coordinator exits 0;
#   S3  a backup interruption (unwritable destination) fails EXPLICITLY and
#       leaves NO artifact that could masquerade as a good snapshot;
#   S4  an OPEN WebSocket session does not block the graceful stop: the
#       session receives close(1001) and the service exits 0 within the
#       deadline (managed-task shutdown; previously an open WS would hang
#       the transport's graceful wait forever).
#
# Environment guards: rustup-locked toolchain 1.98.1, task-dedicated target
# dir, offline locked build, proxy vars stripped, synthetic /tmp home only.
#
# Usage: scripts/rust-tauri/r02_t06_backup_restore.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T06}"
EVIDENCE_DIR="$EVIDENCE_DIR/backup-restore"
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t06}"

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
PROBE_PID=""
cleanup() {
  [ -n "$SERVICE_PID" ] && kill -9 "$SERVICE_PID" 2>/dev/null || true
  [ -n "$PROBE_PID" ] && kill -9 "$PROBE_PID" 2>/dev/null || true
  wait 2>/dev/null || true
  if [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ]; then rm -rf "$HOME_DIR"; fi
}
trap cleanup EXIT

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

start_service() { # $1=home $2=tag
  rm -f "$EVIDENCE_DIR/service-$2.addr"
  "$SERVICE_BIN" --home "$1" --bind 127.0.0.1:0 \
    > "$EVIDENCE_DIR/service-$2.out" 2> "$EVIDENCE_DIR/service-$2.err" &
  SERVICE_PID=$!
  echo "$SERVICE_PID" > "$EVIDENCE_DIR/service.pid"
  for _ in $(seq 1 200); do
    if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/service-$2.out" 2>/dev/null; then
      sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' \
        "$EVIDENCE_DIR/service-$2.out" | head -n 1 > "$EVIDENCE_DIR/service-$2.addr"
      return 0
    fi
    kill -0 "$SERVICE_PID" 2>/dev/null || return 1
    sleep 0.05
  done
  return 1
}

token_of() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' "$1/lingxi-service/local-token.json"; }

http_post() { # $1=addr $2=path $3=token $4=body
  curl -sS -o /dev/null -w '%{http_code}' -X POST \
    -H "Authorization: Bearer $3" -H 'Content-Type: application/json' \
    -d "$4" "http://$1$2"
}

db_hash() { # $1=home  (concatenated digest of the db file set)
  (cd "$1/lingxi-service/data" && cat runs.db* 2>/dev/null | shasum -a 256)
}

wal_bytes() { # $1=home
  python3 - "$1/lingxi-service/data/runs.db-wal" <<'PYEOF'
import os, sys
p = sys.argv[1]
print(os.path.getsize(p) if os.path.exists(p) else 0)
PYEOF
}

# ---- S1 (A11 core): WAL-resident committed data -> backup -> restore --------
note "== S1 (R02-A11): WAL-resident committed data survives backup+restore =="
HOME_DIR=$(mktemp -d /tmp/lingxi-r02t06-bk-XXXXXX)
start_service "$HOME_DIR" s1 || fail "S1 service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service-s1.addr")
TOKEN=$(token_of "$HOME_DIR")
for i in 1 2 3; do
  CODE=$(http_post "$ADDR" "/lingxi/v1/sessions/sess_local_alpha/execute" "$TOKEN" "{\"input\":\"a11 committed run $i\"}")
  [ "$CODE" = "200" ] || fail "S1 execute $i expected 200, got $CODE"
done
note "PASS S1 three committed executes (http 200 each)"

WAL_SIZE=$(wal_bytes "$HOME_DIR")
echo "wal_bytes=$WAL_SIZE" > "$EVIDENCE_DIR/s1-wal-size.txt"
[ "$WAL_SIZE" -gt 0 ] || fail "S1 precondition violated: WAL is empty (nothing un-checkpointed)"
note "PASS S1 precondition: WAL non-empty (committed data not yet checkpointed, bytes=$WAL_SIZE)"

# Crash stop: writers stopped abruptly, the WAL is NOT checkpointed away.
SERVICE_PID=$(cat "$EVIDENCE_DIR/service.pid")
[ -n "$SERVICE_PID" ] || fail "S1 service pid missing (script bug)"
kill -9 "$SERVICE_PID" 2>/dev/null || true
wait "$SERVICE_PID" 2>/dev/null || true
sleep 0.3
SERVICE_PID=""
kill -0 "$(cat "$EVIDENCE_DIR/service.pid")" 2>/dev/null && fail "S1 the crash did not terminate the service"
WAL_SIZE_AFTER_CRASH=$(wal_bytes "$HOME_DIR")
[ "$WAL_SIZE_AFTER_CRASH" -gt 0 ] || fail "S1 crash must keep the WAL (got $WAL_SIZE_AFTER_CRASH)"
db_hash "$HOME_DIR" > "$EVIDENCE_DIR/s1-source-hash.txt"
note "PASS S1 kill -9 (writers stopped; WAL preserved, bytes=$WAL_SIZE_AFTER_CRASH)"

# Backup via the REAL Online Backup API (the only connection in this
# process = quiescent writers; the API reads the committed WAL content).
BACKUP_DIR="$HOME_DIR-backup"
rm -rf "$BACKUP_DIR"
"$INSPECT_BIN" "$HOME_DIR/lingxi-service/data/runs.db" backup "$BACKUP_DIR" runs \
  > "$EVIDENCE_DIR/s1-backup-outcome.json" 2> "$EVIDENCE_DIR/s1-backup.err" \
  || { cat "$EVIDENCE_DIR/s1-backup.err"; fail "S1 backup command failed"; }
python3 - "$EVIDENCE_DIR/s1-backup-outcome.json" <<'PYEOF' || fail "S1 backup outcome invalid"
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc["result"] == "ok", doc
assert doc["bytes"] > 0 and len(doc["sha256"]) == 64, doc
assert doc["manifestName"] == "runs.manifest.json", doc
PYEOF
ls "$BACKUP_DIR" > "$EVIDENCE_DIR/s1-backup-dir-listing.txt"
grep -q "runs.db" "$EVIDENCE_DIR/s1-backup-dir-listing.txt" || fail "S1 backup file missing"
grep -q "runs.manifest.json" "$EVIDENCE_DIR/s1-backup-dir-listing.txt" || fail "S1 manifest missing"
note "PASS S1 online backup produced runs.db + manifest (sha256 recorded)"

RESTORE_DIR="$HOME_DIR-restore"
rm -rf "$RESTORE_DIR"
"$INSPECT_BIN" restore-verify "$BACKUP_DIR" runs "$RESTORE_DIR" \
  > "$EVIDENCE_DIR/s1-restore-outcome.json" 2> "$EVIDENCE_DIR/s1-restore.err" \
  || { cat "$EVIDENCE_DIR/s1-restore.err"; fail "S1 restore-verify failed (recovery open refused the restored copy)"; }
note "PASS S1 restore passed the full recovery open (receipts + integrity) in another directory"

# Logical comparison: restored dump == source dump (same committed facts;
# the crashed-but-acknowledged run IS present — WAL-resident data captured).
"$INSPECT_BIN" "$HOME_DIR/lingxi-service/data/runs.db" dump \
  > "$EVIDENCE_DIR/s1-source-dump.jsonl" 2>/dev/null
"$INSPECT_BIN" "$RESTORE_DIR/runs.db" dump \
  > "$EVIDENCE_DIR/s1-restored-dump.jsonl" 2>/dev/null
python3 - "$EVIDENCE_DIR/s1-source-dump.jsonl" "$EVIDENCE_DIR/s1-restored-dump.jsonl" <<'PYEOF' || fail "S1 logical comparison failed"
import json, sys
def rows(path):
    out = []
    for line in open(path):
        line = line.strip()
        if line.startswith("{"):
            out.append(json.loads(line))
    return out
src, rst = rows(sys.argv[1]), rows(sys.argv[2])
assert len(src) == len(rst), f"row count differs: {len(src)} vs {len(rst)}"
for a, b in zip(src, rst):
    assert a["row"] == b["row"], f"row mismatch: {a} vs {b}"
runs = [r for r in src if r["table"] == "runs"]
events = [r for r in src if r["table"] == "key_events"]
assert len(runs) == 3, f"expected the 3 committed runs, got {len(runs)}"
statuses = [r["row"][6] for r in runs]
assert all(s == "completed" for s in statuses), statuses
assert len(events) == 6, f"expected 2 key events per run, got {len(events)}"
print(f"rows={len(src)} runs={len(runs)} key_events={len(events)} ALL COMPLETE")
PYEOF
note "PASS S1 logical dump equal; all 3 runs terminal-complete with both events (transaction boundaries intact)"
shasum -a 256 "$RESTORE_DIR/runs.db" > "$EVIDENCE_DIR/s1-restored-hash.txt" || true

# ---- S2: graceful stop with the shutdown coordinator ------------------------
note "== S2: graceful stop (SIGTERM) exits 0 =="
start_service "$HOME_DIR" s2 || fail "S2 service did not become ready (restart after crash = WAL recovery applied)"
SERVICE_PID=$(cat "$EVIDENCE_DIR/service.pid")
kill -TERM "$SERVICE_PID"
wait "$SERVICE_PID" 2>/dev/null
EXIT=$?
SERVICE_PID=""
[ "$EXIT" = "0" ] || fail "S2 graceful stop expected exit 0, got $EXIT"
note "PASS S2 graceful stop exit 0 (coordinator: drain -> checkpoint -> record removal)"

# ---- S3: interrupted backup leaves no masquerading artifact -----------------
note "== S3: backup interruption (unwritable destination) =="
FAIL_PARENT="$HOME_DIR-bkparent"
mkdir -p "$FAIL_PARENT"
chmod 0555 "$FAIL_PARENT"
set +e
"$INSPECT_BIN" "$HOME_DIR/lingxi-service/data/runs.db" backup "$FAIL_PARENT/dest" runs \
  > "$EVIDENCE_DIR/s3-backup-out.json" 2> "$EVIDENCE_DIR/s3-backup.err"
BK_EXIT=$?
set -e
chmod 0755 "$FAIL_PARENT"
[ "$BK_EXIT" != "0" ] || fail "S3 backup into an unwritable destination must fail"
grep -Eq "backup failed|Permission denied|cannot create" "$EVIDENCE_DIR/s3-backup.err" \
  || fail "S3 stderr must name the backup failure"
if [ -d "$FAIL_PARENT/dest" ]; then
  LEFT=$(ls -A "$FAIL_PARENT/dest" | grep -E '\.db$|\.json$|partial' | wc -l | tr -d ' ')
  [ "$LEFT" = "0" ] || fail "S3 leftover artifacts: $LEFT"
fi
note "PASS S3 interrupted backup: explicit failure, zero artifacts (no half backup masquerading as success)"

# ---- S4: open WS session does not block the graceful stop -------------------
note "== S4: open WebSocket session + SIGTERM -> close(1001), exit 0 =="
start_service "$HOME_DIR" s4 || fail "S4 service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service-s4.addr")
TOKEN=$(token_of "$HOME_DIR")
ADDR="$ADDR" TOKEN="$TOKEN" OUT="$EVIDENCE_DIR/s4-ws-probe.json" python3 - <<'PYEOF' &
import json, os, socket, struct, sys

WS_GUID = "258EAFA5-E914-47DA-C5CA-C5AB0DC85B11"
host_port = os.environ["ADDR"]
host, port = host_port.rsplit(":", 1)
bearer = os.environ["TOKEN"]

def ws_key():
    import base64
    return base64.b64encode(os.urandom(16)).decode()

def recv_headers(sock):
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(1)
        if not chunk:
            break
        buf += chunk
    return buf.decode("utf-8", "replace")

sock = socket.create_connection((host, int(port)), timeout=15)
request = (
    f"GET /lingxi/v1/ws HTTP/1.1\r\nHost: {host}:{port}\r\n"
    "Upgrade: websocket\r\nConnection: Upgrade\r\n"
    f"Sec-WebSocket-Key: {ws_key()}\r\nSec-WebSocket-Version: 13\r\n"
    f"Authorization: Bearer {bearer}\r\n\r\n"
)
sock.sendall(request.encode())
head = recv_headers(sock)
status = int(head.split()[1]) if head.split() and len(head.split()) > 1 else 0
assert status == 101, f"upgrade failed: {status}"

def send_frame(sock, payload, opcode=0x1):
    mask = os.urandom(4)
    header = bytes([0x80 | opcode])
    length = len(payload)
    if length < 126:
        header += bytes([0x80 | length])
    elif length <= 0xFFFF:
        header += bytes([0x80 | 126]) + struct.pack(">H", length)
    else:
        header += bytes([0x80 | 127]) + struct.pack(">Q", length)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    sock.sendall(header + mask + masked)

def recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise EOFError
        buf += chunk
    return buf

def recv_frame(sock):
    header = recv_exact(sock, 2)
    opcode = header[0] & 0x0F
    length = header[1] & 0x7F
    if length == 126:
        length = struct.unpack(">H", recv_exact(sock, 2))[0]
    elif length == 127:
        length = struct.unpack(">Q", recv_exact(sock, 8))[0]
    payload = recv_exact(sock, length) if length else b""
    if opcode == 0x8:
        code = struct.unpack(">H", payload[:2])[0] if len(payload) >= 2 else 1005
        return ("close", code)
    if opcode == 0x1:
        return ("text", 0)
    return ("other", 0)

hello = json.dumps({"protocol": "lingxi.wire", "clientKind": "probe",
                    "clientVersion": "0", "protocolMin": 1, "protocolMax": 1})
send_frame(sock, hello.encode())
kind, _ = recv_frame(sock)
assert kind == "text", "expected ServerHello"
send_frame(sock, json.dumps({"type": "subscribe_events", "streamId": "sess_local_alpha"}).encode())
# Wait for the server shutdown close frame (SIGTERM arrives any moment).
sock.settimeout(30)
for _ in range(200):
    kind, code = recv_frame(sock)
    if kind == "close":
        result = {"closed": True, "code": code}
        break
else:
    result = {"closed": False, "code": 0}
print(json.dumps(result), flush=True)
open(os.environ["OUT"] + ".probe-done", "w").write("done")
with open(os.environ["OUT"], "w") as fh:
    json.dump(result, fh)
sys.exit(0)
PYEOF
PROBE_PID=$!
# Wait until the probe is subscribed (ServerHello+subscribe processed), then stop.
sleep 1
SERVICE_PID=$(cat "$EVIDENCE_DIR/service.pid")
kill -TERM "$SERVICE_PID"
wait "$SERVICE_PID" 2>/dev/null
EXIT=$?
SERVICE_PID=""
wait "$PROBE_PID" 2>/dev/null || true
PROBE_PID=""
[ "$EXIT" = "0" ] || fail "S4 graceful stop with an open WS expected exit 0, got $EXIT (open WS must not deadlock the stop)"
python3 - "$EVIDENCE_DIR/s4-ws-probe.json" <<'PYEOF' || fail "S4 WS session did not receive the shutdown close frame"
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc.get("closed") is True, doc
assert doc.get("code") == 1001, doc
PYEOF
note "PASS S4 WS session received close(1001); service exited 0 (managed-task shutdown, no deadlock)"

# ---- residue check ----------------------------------------------------------
LEFT=$( { pgrep -f "lingxi-service --home $HOME_DIR" || true; } | wc -l | tr -d ' ')
[ "$LEFT" = "0" ] || fail "leftover service processes: $LEFT"
rm -rf "$HOME_DIR" "$HOME_DIR-backup" "$HOME_DIR-restore" "$HOME_DIR-bkparent"
note "RESULT: R02-T06 backup/restore binary evidence ALL GREEN"
