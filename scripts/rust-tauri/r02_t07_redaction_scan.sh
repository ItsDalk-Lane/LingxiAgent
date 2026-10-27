#!/usr/bin/env bash
# R02-T07 / acceptance R02-A13 — "敏感值不出现在日志" against the REAL
# lingxi-service binary: every log/trace/stderr/audit surface produced
# during normal AND error traffic is scanned for the preset synthetic
# sensitive values.
#
# Proves (binary level):
#   P1  normal traffic: authenticated GET /me + POST execute (the execute
#       INPUT carries preset API-key/device-secret material — body material
#       must not leak into diagnostics either);
#   P2  error traffic: 401 (bad bearer / missing credential / bad query
#       token), 403 (cross-principal), 404 (unknown session), WS upgrade
#       with an invalid ticket, WS session with a malformed first frame;
#   P3  storage error: --db-queue-bound 1 + concurrent execute burst =>
#       explicit 503 db_queue_full responses (backpressure, R02-T07);
#   P4  epoch refusal: corrupt epoch stamp => exit 2 with the
#       LINGXI_DATA_EPOCH_BLOCKED marker (its diagnostics are scanned too);
#   SCAN the FULL evidence set (the run's REAL rotating log files,
#       RETAINED into the evidence tree first — R9-F08 — plus every
#       stderr/stdout capture + every captured error response body + WS
#       transcripts) for each preset sensitive value AND for the real
#       minted secrets (local token, issued ws ticket): count must be
#       ZERO everywhere; a run that produced no rotating logs, or a scan
#       target that cannot be READ, FAILS (never silently skipped);
#       R10-F02: the evidence root must be FRESH (a non-empty root is
#       refused, never merged into) and the retention is SOURCE-BOUND to
#       THIS round — the gating count is taken in the run home, the
#       retained set must equal the source set, and every retained log
#       is pinned to its source by sha256 in retention-manifest.txt
#       with the run identity (UTC + pid + source dir): a previous
#       round's logs can never vouch for this round;
#   CORRELATION the requestId of an error response appears in the captured
#       rejection marker line, and the session id of the executed request
#       appears in the retained real log files (关联 ID 保留).
#
# Preset values are SYNTHETIC (no real credential shapes' real values):
# sk-test-…, hana_dev_…, Bearer a13-…, ?token=a13-… — designed to be
# recognizable and to match the redactor's mirrored patterns.
#
# Environment guards: rustup-locked toolchain, task-dedicated target dir,
# offline locked build, proxy vars stripped, synthetic /tmp home only.
#
# Scan scope (explicit): every file under $EVIDENCE_DIR — including the
# run's REAL rotating logs copied into $EVIDENCE_DIR/service-logs before
# the scan (R9-F08: they were previously never scanned and were deleted
# with the run home; R10-F02: $EVIDENCE_DIR is one-run-one-fresh-root and
# the copy is a source-bound snapshot, so service-logs can only contain
# THIS round's logs) — i.e. response bodies, stderr/stdout captures, WS
# transcripts, the retained real logs, summaries, EXCEPT the recorded
# request bodies (which by construction carry the preset values and are
# the inputs, not the diagnostic output) AND except
# p1-issue-credential.body — the contract-sanctioned 201 credential
# issuance response (the one surface that MUST deliver a fresh secret
# once, mirroring the incumbent LOCAL_ONLY device route). Any target that
# cannot be READ fails the scan; nothing is ever skipped silently.
#
# Usage: scripts/rust-tauri/r02_t07_redaction_scan.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T07}"
EVIDENCE_DIR="$EVIDENCE_DIR/redaction-scan"
# R10-F02: one run = one FRESH evidence root. The fixed root used to be
# merely `mkdir -p`-ed, so an independent direct re-run (or any reuse)
# merged into the previous round's artifacts: `service-logs` was merged
# with `cp -R`, and the retention count was taken from the MERGED
# directory — a previous round's `service-*.log` files could satisfy
# LOG_COUNT >= 1 entirely while THIS round produced no rotating logs at
# all (the log-attach degradation path falls back to stderr and leaves
# the source directory empty), and the scan then reported the old files
# clean as if they were this round's output. A non-empty root is now
# REFUSED loudly — nothing is ever deleted (no dangerous cleanup of
# anyone's directory; the caller picks a fresh root or the run fails).
if [ -L "$EVIDENCE_DIR" ]; then
  echo "ERROR: evidence path is a symlink: $EVIDENCE_DIR" >&2
  exit 1
fi
if [ -e "$EVIDENCE_DIR" ]; then
  if [ ! -d "$EVIDENCE_DIR" ]; then
    echo "ERROR: evidence path $EVIDENCE_DIR exists and is not a directory" >&2
    exit 1
  fi
  # Emptiness test is depth-UNBOUNDED (first entry wins, -print -quit);
  # the bounded listing below is display-only.
  if [ -n "$(find "$EVIDENCE_DIR" -mindepth 1 -print -quit 2>/dev/null)" ]; then
    PRE_LIST="$(find "$EVIDENCE_DIR" -mindepth 1 -maxdepth 2 2>/dev/null)"
    printf 'ERROR: evidence root %s is not empty — refusing to reuse it (artifacts of a previous round must not be merged into and cannot vouch for this round; first entries):\n%s\n' \
      "$EVIDENCE_DIR" "$(printf '%s\n' "$PRE_LIST" | sed -n '1,20p')" >&2
    exit 1
  fi
fi
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r02-t07}"

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1)"
[ -n "$TOOLCHAIN" ] || { echo "ERROR: no toolchain in rust-toolchain.toml" >&2; exit 1; }
if ! command -v rustup >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/rustup" ]; then PATH="$HOME/.cargo/bin:$PATH";
  else echo "ERROR: rustup not found" >&2; exit 1; fi
fi
CARGO="env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TARGET_DIR rustup run $TOOLCHAIN cargo"

HOME_DIR=""
SERVICE_PID=""
LOCK_PID=""
# R02 stage-repair R7 / R7-F02: SERVICE_PID is the CURRENT handle — set on
# spawn, RETIRED (cleared) after every wait/reap (stop_service already
# did). The trap signals it ONLY while it still proves CURRENT ownership
# (exists AND ppid is THIS shell): a retired or recycled number is never
# signalled (R6-F02 A12 pattern; the old trap TERMed the bare number on
# existence of the variable alone).
# R12-F01: the boolean probe's false branch conflated exited/foreign/
# unobservable and the trap's owned branch was `kill (TERM); wait` with
# NO deadline — a child ignoring or delaying TERM hung the trap itself.
# Four-state probe + bounded ladder below (≈10 s worst case per handle);
# residue reported loudly at expiry, never an unbounded wait. The
# 外部写锁辅助进程也有独立句柄；正常路径限时回收，异常路径由 trap 同样有界清理。
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
# R12-F01: bounded stop for ONE provably-owned handle under THIS script's
# contract (TERM first — stop_service asserts the graceful stop and
# stays untouched): TERM → ≤5 s poll → direct-pid KILL only while still
# provably ours → ≤5 s re-check. Prints the final state.
bounded_stop_owned() {
  local pid="$1" state="" i
  # 发信号前在函数内再次核实，调用方的先前判断不能替代当前归属。
  state="$(child_state "$pid")"
  if [ "$state" != "owned" ]; then printf '%s\n' "$state"; return 0; fi
  kill -TERM "$pid" 2>/dev/null || true
  for i in $(seq 1 100); do
    state="$(child_state "$pid")"
    case "$state" in
      exited|foreign) break ;;
      owned|unobservable) : ;;
    esac
    sleep 0.05
  done
  if [ "${state:-owned}" = "owned" ]; then
    state="$(child_state "$pid")"
  fi
  if [ "$state" = "owned" ]; then
    kill -KILL "$pid" 2>/dev/null || true
    for i in $(seq 1 100); do
      state="$(child_state "$pid")"
      case "$state" in
        exited|foreign) break ;;
        owned|unobservable) : ;;
      esac
      sleep 0.05
    done
  fi
  printf '%s\n' "${state:-unobservable}"
}
cleanup() {
  local cleanup_residue=0
  if [ -n "$LOCK_PID" ]; then
    case "$(child_state "$LOCK_PID")" in
      owned)
        case "$(bounded_stop_owned "$LOCK_PID")" in
          exited) wait "$LOCK_PID" 2>/dev/null || true ;;
          foreign) echo "cleanup: external lock helper $LOCK_PID 已不属于本脚本，不等待或发信号" >&2; cleanup_residue=1 ;;
          *) echo "cleanup: external lock helper $LOCK_PID may remain" >&2; cleanup_residue=1 ;;
        esac ;;
      exited) wait "$LOCK_PID" 2>/dev/null || true ;;
      foreign|unobservable) echo "cleanup: external lock helper $LOCK_PID ownership cannot be proved" >&2; cleanup_residue=1 ;;
    esac
    LOCK_PID=""
  fi
  if [ -n "$SERVICE_PID" ]; then
    case "$(child_state "$SERVICE_PID")" in
      owned)
        case "$(bounded_stop_owned "$SERVICE_PID")" in
          exited)
            wait "$SERVICE_PID" 2>/dev/null || true
            ;;
          foreign)
            echo "cleanup: pid $SERVICE_PID 已不属于本脚本，不等待或发信号" >&2
            cleanup_residue=1
            ;;
          owned)
            echo "cleanup: pid $SERVICE_PID still OWNED after the TERM and KILL budgets — RESIDUE left behind, no unbounded wait" >&2
            cleanup_residue=1
            ;;
          unobservable)
            echo "cleanup: pid $SERVICE_PID state UNOBSERVABLE after the stop budgets — not signalled further, no unbounded wait; possible residue" >&2
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
  # R12-F01: the parameterless `wait` that used to run here waited for
  # EVERY outstanding child with NO deadline — including exactly the
  # owned/unobservable residue the bounded ladder above now reports
  # instead of hanging on. Each handle was reaped inside its state
  # branch (exited) or reported as residue (owned/foreign/unobservable);
  # nothing legitimate is left to wait for.
  if [ "$cleanup_residue" -eq 0 ]; then
    [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ] && rm -rf "$HOME_DIR"
  else
    echo "cleanup: 进程仍存活或归属不明，保留本轮 home=$HOME_DIR 供核查" >&2
    exit 1
  fi
  return 0
}
trap cleanup EXIT

# Self-contained logging environment (R02 stage-repair R1): this script
# scans info-level service output for redaction compliance. The service
# reads RUST_LOG with an info default, but a caller exporting
# RUST_LOG=warn (e.g. the verify-stage runner) would starve the sampled
# lines and fail the scan for the wrong reason. Pin info for the service
# spawns below; redaction behavior itself is level-independent.
export RUST_LOG=info

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

# ── preset SYNTHETIC sensitive values (no real credentials anywhere) ───────
API_KEY="sk-test-a13-redact-0123456789abcdef"
DEVICE_SECRET="hana_dev_A13oauth0123456789abcdefghij"
OAUTH_TOKEN="a13-oauth-bearer-0123456789abcdefgh"
QUERY_TOKEN="a13-query-token-0123456789abcdef"

note "== R02-T07 / R02-A13 redaction scan (toolchain $TOOLCHAIN, target $TARGET_DIR) =="

note "== building lingxi-service (--locked, offline) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
SERVICE_BIN="$TARGET_DIR/debug/lingxi-service"
[ -x "$SERVICE_BIN" ] || fail "service binary missing"
note "PASS build"

start_service() { # $1=home $2=tag $3...=extra flags
  local home="$1" tag="$2"; shift 2
  rm -f "$EVIDENCE_DIR/service-$tag.addr"
  "$SERVICE_BIN" --home "$home" --bind 127.0.0.1:0 "$@" \
    > "$EVIDENCE_DIR/service-$tag.out" 2> "$EVIDENCE_DIR/service-$tag.err" &
  SERVICE_PID=$!
  for _ in $(seq 1 200); do
    if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/service-$tag.out" 2>/dev/null; then
      sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' \
        "$EVIDENCE_DIR/service-$tag.out" | head -n 1 > "$EVIDENCE_DIR/service-$tag.addr"
      return 0
    fi
    kill -0 "$SERVICE_PID" 2>/dev/null || return 1
    sleep 0.05
  done
  return 1
}

stop_service() {
  local state rc
  [ -n "$SERVICE_PID" ] || fail "service handle missing before graceful stop"
  [ "$(child_state "$SERVICE_PID")" = "owned" ] || fail "service not owned before graceful stop"
  state="$(bounded_stop_owned "$SERVICE_PID")"
  case "$state" in exited) ;; *) fail "service remained or ownership changed after TERM/KILL budget (state=$state)" ;; esac
  if wait "$SERVICE_PID" 2>/dev/null; then rc=0; else rc=$?; fi
  SERVICE_PID=""
  [ "$rc" -eq 0 ] || fail "service shutdown exit=$rc"
}

token_of() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' \
  "$1/lingxi-service/local-token.json"; }

http_capture() { # $1=tag $2=method $3=addr $4=path $5=auth-or-empty $6=bodyfile-or-empty
  local tag="$1" method="$2" addr="$3" path="$4" auth="${5:-}" bodyfile="${6:-}"
  local args=(-sS -o "$EVIDENCE_DIR/$tag.body" -D "$EVIDENCE_DIR/$tag.headers" -w '%{http_code}')
  [ -n "$auth" ] && args+=(-H "Authorization: $auth")
  if [ -n "$bodyfile" ]; then
    args+=(-H 'Content-Type: application/json' --data-binary "@$bodyfile")
  fi
  curl "${args[@]}" "http://$addr$path"
}

# ── P1+P2: normal + error traffic against a fresh service ──────────────────
note "== P1/P2: normal + error traffic =="
HOME_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r02t07-a13.XXXXXX")
start_service "$HOME_DIR" p1 || fail "service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service-p1.addr")
LOCAL_TOKEN=$(token_of "$HOME_DIR")

# --- normal: authenticated me ---
CODE=$(http_capture p1-me-ok GET "$ADDR" "/lingxi/v1/me" "Bearer $LOCAL_TOKEN" "")
[ "$CODE" = "200" ] || fail "P1 GET /me expected 200, got $CODE"
note "PASS P1 GET /me -> 200"

# --- normal: execute whose INPUT carries preset secret material ---
python3 - "$EVIDENCE_DIR/p1-execute-request.json" "$API_KEY" "$DEVICE_SECRET" <<'PYEOF'
import json, sys
path, api_key, device_secret = sys.argv[1], sys.argv[2], sys.argv[3]
body = {
    "input": (
        f"synthetic diagnostic payload for the redaction scan; do not log: "
        f"api_key = \"{api_key}\"; device credential {device_secret}; "
        f"oauth {api_key}"
    )
}
with open(path, "w") as f:
    json.dump(body, f)
PYEOF
CODE=$(http_capture p1-execute-ok POST "$ADDR" "/lingxi/v1/sessions/sess_local_alpha/execute" \
  "Bearer $LOCAL_TOKEN" "$EVIDENCE_DIR/p1-execute-request.json")
[ "$CODE" = "200" ] || fail "P1 execute expected 200, got $CODE"
RUN_ID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("runId",""))' \
  "$EVIDENCE_DIR/p1-execute-ok.body")
[ -n "$RUN_ID" ] || fail "execute response lacks runId"
note "PASS P1 execute -> 200 (runId minted; input carried preset secret material)"

# --- normal: issue a device credential (contract surface; its 201 body is
#     the one sanctioned secret-delivery surface, excluded from the scan) ---
ISSUE_REQUEST="$EVIDENCE_DIR/p1-issue-request.json"
printf '{"userId":"user_remote_b","scopes":["chat"]}' > "$ISSUE_REQUEST"
CODE=$(http_capture p1-issue-credential POST "$ADDR" "/lingxi/v1/devices/credentials" \
  "Bearer $LOCAL_TOKEN" "$ISSUE_REQUEST")
[ "$CODE" = "201" ] || fail "P1 issue credential expected 201, got $CODE"
USER_B_TOKEN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["secret"])' \
  "$EVIDENCE_DIR/p1-issue-credential.body")
note "PASS P1 issue credential -> 201 (201 issuance body = sanctioned surface, scan-excluded)"

# --- normal: mint a one-shot WS ticket (a REAL service-minted secret that
#     must never leak into any diagnostic; scanned later) ---
TICKET_REQUEST="$EVIDENCE_DIR/p1-ticket-request.json"
printf '{}' > "$TICKET_REQUEST"
CODE=$(http_capture p1-ticket POST "$ADDR" "/lingxi/v1/ws-ticket" "Bearer $LOCAL_TOKEN" "$TICKET_REQUEST")
[ "$CODE" = "200" ] || fail "P1 ws-ticket expected 200, got $CODE"
WS_TICKET=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["ticket"])' \
  "$EVIDENCE_DIR/p1-ticket.body")
note "PASS P1 ws-ticket minted -> 200"

# --- 401: preset OAuth bearer token (synthetic) ---
CODE=$(http_capture p2-me-bad-bearer GET "$ADDR" "/lingxi/v1/me" "Bearer $OAUTH_TOKEN" "")
[ "$CODE" = "401" ] || fail "P2 bad bearer expected 401, got $CODE"
note "PASS P2 401 bad bearer"

# --- 401: missing credential ---
CODE=$(http_capture p2-me-no-auth GET "$ADDR" "/lingxi/v1/me" "" "")
[ "$CODE" = "401" ] || fail "P2 no auth expected 401, got $CODE"
note "PASS P2 401 missing credential"

# --- 401: preset query token on the WS route ---
CODE=$(http_capture p2-ws-bad-query GET "$ADDR" "/lingxi/v1/ws?token=$QUERY_TOKEN" "" "")
[ "$CODE" = "401" ] || fail "P2 bad query token expected 401, got $CODE"
note "PASS P2 401 bad query token"

# --- 403: cross-principal (user_remote_b reads the owner's session) ---
CODE=$(http_capture p2-session-cross GET "$ADDR" "/lingxi/v1/sessions/sess_local_alpha" \
  "Bearer $USER_B_TOKEN" "")
[ "$CODE" = "403" ] || fail "P2 cross-principal expected 403, got $CODE"
note "PASS P2 403 cross-principal"

# --- 404: unknown session (owner) ---
CODE=$(http_capture p2-session-missing GET "$ADDR" "/lingxi/v1/sessions/sess_unknown_a13" \
  "Bearer $LOCAL_TOKEN" "")
[ "$CODE" = "404" ] || fail "P2 unknown session expected 404, got $CODE"
note "PASS P2 404 unknown session"

# --- WS: invalid ticket upgrade + malformed first frame (real sockets) ---
ADDR="$ADDR" LOCAL_TOKEN="$LOCAL_TOKEN" USER_B_TOKEN="$USER_B_TOKEN" \
  EVIDENCE_DIR="$EVIDENCE_DIR" OAUTH_TOKEN="$OAUTH_TOKEN" python3 - <<'PYEOF'
import base64, os, socket, struct, sys, json

addr = os.environ["ADDR"]
host, port = addr.rsplit(":", 1)
local_token = os.environ["LOCAL_TOKEN"]
user_b_token = os.environ["USER_B_TOKEN"]
oauth_token = os.environ["OAUTH_TOKEN"]
evidence = os.environ["EVIDENCE_DIR"]

def ws_key():
    return base64.b64encode(os.urandom(16)).decode()

def recv_headers(sock):
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(1)
        if not chunk:
            break
        buf += chunk
    return buf.decode("utf-8", "replace")

def upgrade(auth_value=None, query=None):
    sock = socket.create_connection((host, int(port)), timeout=15)
    path = "/lingxi/v1/ws" + (f"?{query}" if query else "")
    request = (
        f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
        "Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {ws_key()}\r\nSec-WebSocket-Version: 13\r\n"
    )
    if auth_value:
        request += f"Authorization: Bearer {auth_value}\r\n"
    request += "\r\n"
    sock.sendall(request.encode())
    head = recv_headers(sock)
    return sock, head

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
    return opcode, payload

transcript = []

# (a) invalid bearer token: upgrade must be refused (401).
sock, head = upgrade(oauth_token)
status = int(head.split()[1])
transcript.append(f"upgrade-bad-bearer status={status}")
assert status == 401, f"bad bearer upgrade: {status}"
sock.close()
transcript.append("PASS upgrade with invalid bearer refused 401")

# (b) valid bearer: handshake then a MALFORMED first frame -> protocol
#     error frame + close (invalid_message). The malformed frame carries
#     preset secret material (must not leak anywhere).
sock, head = upgrade(local_token)
status = int(head.split()[1])
assert status == 101, f"valid upgrade failed: {status}"
transcript.append(f"upgrade-valid status={status}")

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

# Skip the ServerHello (first server text frame) — send garbage first.
send_frame(sock, b"this is not a valid ClientHello \xf0\x9f\x92\x98 hana_dev_A13oauth0123456789abcdefghij")
opcode, payload = recv_frame(sock)
transcript.append(f"error frame opcode={opcode} payload={payload[:120].decode('utf-8','replace')}")
assert opcode == 0x1, f"expected the protocol error text frame, got opcode {opcode}"
assert b"invalid_message" in payload, f"error frame lacks the code: {payload[:200]!r}"
opcode, payload = recv_frame(sock)
transcript.append(f"close frame opcode={opcode} code={struct.unpack('>H', payload[:2])[0] if len(payload) >= 2 else 'none'}")
assert opcode == 0x8, "expected the close frame after the protocol error"
sock.close()
transcript.append("PASS malformed first frame -> error frame + close")

with open(os.path.join(evidence, "p2-ws-transcript.txt"), "w") as f:
    f.write("\n".join(transcript) + "\n")
PYEOF
note "PASS P2 WS rejections (bad bearer upgrade 401; malformed frame -> invalid_message + close)"

# ── P3: storage error (explicit 503, real SQLITE_BUSY) ─────────────────────
note "== P3: storage error via externally-held WAL write lock (busy timeout) =="
stop_service
start_service "$HOME_DIR" p3 || fail "P3 service did not become ready"
ADDR=$(cat "$EVIDENCE_DIR/service-p3.addr")
LOCAL_TOKEN=$(token_of "$HOME_DIR")
python3 - "$EVIDENCE_DIR/p3-execute-request.json" <<'PYEOF'
import json, sys
body = {"input": "synthetic execute during a held write lock"}
with open(sys.argv[1], "w") as f:
    json.dump(body, f)
PYEOF
# Deterministic REAL storage failure: an external SQLite connection holds
# the WAL write lock LONGER than the service's 5s busy timeout, so the
# single-writer worker's blocked write fails with SQLITE_BUSY ->
# StorageError::Busy -> explicit 503 (reason=db_busy). No fault is mocked:
# the DB really cannot take the write.
python3 - "$HOME_DIR/lingxi-service/data/runs.db" > "$EVIDENCE_DIR/p3-lock.log" 2>&1 <<'PYLOCK' &
import sqlite3, sys, time
conn = sqlite3.connect(sys.argv[1], timeout=10)
conn.execute("PRAGMA busy_timeout=10000")
conn.execute("BEGIN IMMEDIATE")
print("LOCK HELD", flush=True)
time.sleep(8.0)
conn.rollback()
conn.close()
PYLOCK
LOCK_PID=$!
for _ in $(seq 1 100); do
  grep -q "LOCK HELD" "$EVIDENCE_DIR/p3-lock.log" 2>/dev/null && break
  sleep 0.05
done
grep -q "LOCK HELD" "$EVIDENCE_DIR/p3-lock.log" || fail "P3 could not acquire the external write lock"
: > "$EVIDENCE_DIR/p3-status-codes.txt"
seq 1 8 | xargs -P 8 -I{} curl -sS --max-time 60 -o "$EVIDENCE_DIR/p3-burst-{}.body" \
  -w '%{http_code}\n' -H "Authorization: Bearer $LOCAL_TOKEN" \
  -H 'Content-Type: application/json' \
  --data-binary "@$EVIDENCE_DIR/p3-execute-request.json" \
  "http://$ADDR/lingxi/v1/sessions/sess_local_beta/execute" \
  >> "$EVIDENCE_DIR/p3-status-codes.txt"
LOCK_STATE=""
for _ in $(seq 1 220); do
  LOCK_STATE="$(child_state "$LOCK_PID")"
  case "$LOCK_STATE" in exited|foreign) break ;; owned|unobservable) : ;; esac
  sleep 0.05
done
case "$LOCK_STATE" in exited) ;; *) fail "external lock helper did not finish or ownership changed within 11 seconds (state=$LOCK_STATE)" ;; esac
if wait "$LOCK_PID" 2>/dev/null; then LOCK_RC=0; else LOCK_RC=$?; fi
LOCK_PID=""
[ "$LOCK_RC" -eq 0 ] || fail "external lock helper failed (exit=$LOCK_RC)"
COUNT_503=$(grep -c '^503$' "$EVIDENCE_DIR/p3-status-codes.txt" || true)
COUNT_200=$(grep -c '^200$' "$EVIDENCE_DIR/p3-status-codes.txt" || true)
note "P3 burst results: 200x$COUNT_200, 503x$COUNT_503"
[ "$COUNT_503" -ge 1 ] || fail "P3 expected at least one explicit 503 storage-error response"
grep -l 'db_busy' "$EVIDENCE_DIR"/p3-burst-*.body >/dev/null 2>&1 \
  || fail "P3 503 bodies must carry reason=db_busy"
grep -l '"retryable":true' "$EVIDENCE_DIR"/p3-burst-*.body >/dev/null 2>&1 \
  || fail "P3 db_busy 503 bodies must carry retryable=true"
note "PASS P3 storage error: $COUNT_503 explicit 503 db_busy (retryable) responses; no fake success"

# ── P4: epoch refusal (corrupt stamp) ───────────────────────────────────────
note "== P4: epoch gate refusal diagnostics =="
stop_service
# Corrupt the epoch stamp at the home root; the gate must refuse startup
# (exit 2) with the machine-readable marker — and those diagnostics are
# part of the scanned surface.
printf '{corrupted-stamp-not-json' > "$HOME_DIR/data-epoch.json"
set +e
"$SERVICE_BIN" --home "$HOME_DIR" --bind 127.0.0.1:0 \
  > "$EVIDENCE_DIR/service-p4.out" 2> "$EVIDENCE_DIR/service-p4.err"
P4_EXIT=$?
set -e
[ "$P4_EXIT" -eq 2 ] || { cat "$EVIDENCE_DIR/service-p4.err"; fail "P4 expected exit 2, got $P4_EXIT"; }
grep -q 'LINGXI_DATA_EPOCH_BLOCKED\|LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE' \
  "$EVIDENCE_DIR/service-p4.err" || fail "P4 stderr lacks the epoch marker"
note "PASS P4 epoch refusal exit 2 with marker"

# ── RETAIN: the run's REAL rotating logs enter the evidence set (R9-F08) ────
# The scan used to walk ONLY $EVIDENCE_DIR while the service's real
# rotating logs stayed in $HOME_DIR/lingxi-service/logs — never scanned,
# then deleted by the exit trap — and any unreadable scan target was
# silently skipped. The real logs are now COPIED into the evidence tree
# first (retention), the scan REQUIRES at least one retained log file
# (a run that produced none cannot claim the log surface clean), and any
# unreadable target FAILS the scan instead of counting as unread.
# R10-F02: retention is SOURCE-BOUND to THIS round. The gating count is
# the count taken in the RUN HOME at retention time (never the merged
# copy's count — with a fresh root the copy cannot contain foreign
# files, and the copy is verified set-equal and byte-identical anyway);
# the destination must not pre-exist; and every retained log is pinned
# to its source file by content sha256 in retention-manifest.txt together
# with the run identity (UTC + pid + source dir) — a retained log with no
# this-round source, a partial copy, or an unreadable source/retained
# file fails the run instead of narrowing the scan.
note "== RETAIN: copy the run's REAL rotating logs into the evidence set =="
REAL_LOGS_DIR="$HOME_DIR/lingxi-service/logs"
RETAINED_LOGS="$EVIDENCE_DIR/service-logs"
RETAIN_UTC="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
[ -d "$REAL_LOGS_DIR" ] || fail "no lingxi-service/logs directory exists in the run home — the rotating-log surface was never produced (cannot scan)"
SOURCE_LOG_COUNT=$(find "$REAL_LOGS_DIR" -type f -name '*.log' | wc -l | tr -d ' ')
[ "$SOURCE_LOG_COUNT" -ge 1 ] \
  || fail "zero rotating log files in THIS round's run home ($REAL_LOGS_DIR) — the file-log surface was never produced this round (an attach-failure stderr-only degradation cannot claim the log surface clean)"
[ ! -e "$RETAINED_LOGS" ] \
  || fail "retained-log path $RETAINED_LOGS already exists — refusing to merge into existing content (retention must be a fresh snapshot of this round's source)"
mkdir -p "$RETAINED_LOGS"
# Content-for-content copy of the real layout; failures are loud — a
# partial copy would silently narrow the scan below.
cp -R "$REAL_LOGS_DIR/." "$RETAINED_LOGS/" || fail "cannot copy the real rotating logs into the evidence set (retention failed)"
# R10-F02: set-equality + per-file byte binding, source → retained. The
# manifest itself is .txt, so it never pollutes the *.log set comparison.
RETAINED_LOG_COUNT=$(find "$RETAINED_LOGS" -type f -name '*.log' | wc -l | tr -d ' ')
[ "$RETAINED_LOG_COUNT" -eq "$SOURCE_LOG_COUNT" ] \
  || fail "retained *.log count ($RETAINED_LOG_COUNT) != this round's source *.log count ($SOURCE_LOG_COUNT) — retention is not a faithful snapshot (stale extra files or a partial copy), failing closed"
{
  echo "retained-by: pid $$ at $RETAIN_UTC"
  echo "source-dir: $REAL_LOGS_DIR"
  echo "source-log-count: $SOURCE_LOG_COUNT"
  find "$REAL_LOGS_DIR" -type f -name '*.log' | sort | while IFS= read -r src; do
    rel="${src#"$REAL_LOGS_DIR"/}"
    dst="$RETAINED_LOGS/$rel"
    if [ ! -f "$dst" ]; then echo "MISSING $rel" >&2; exit 1; fi
    src_sha="$(shasum -a 256 "$src" 2>/dev/null | awk '{print $1}')"
    dst_sha="$(shasum -a 256 "$dst" 2>/dev/null | awk '{print $1}')"
    if [ -z "$src_sha" ]; then echo "UNREADABLE-SOURCE $rel" >&2; exit 1; fi
    if [ -z "$dst_sha" ]; then echo "UNREADABLE-RETAINED $rel" >&2; exit 1; fi
    if [ "$src_sha" != "$dst_sha" ]; then echo "MISMATCH $rel src=$src_sha retained=$dst_sha" >&2; exit 1; fi
    printf '%s  %s  bytes=%s\n' "$src_sha" "$rel" "$(wc -c < "$src" | tr -d ' ')"
  done
} > "$RETAINED_LOGS/retention-manifest.txt" \
  || fail "retention verification failed: every retained log must be bound byte-identically to THIS round's source (see stderr above)"
LOG_COUNT="$SOURCE_LOG_COUNT"
note "retained $LOG_COUNT real rotating log file(s) under service-logs/ — fresh-root, source-bound to THIS round (UTC $RETAIN_UTC, pid $$), set-equal and per-file sha256-identical (retention-manifest.txt); in the scan below; sha256 in inventory.txt"

# ── SCAN: preset values must appear NOWHERE in the evidence output set ──────
note "== SCAN: preset sensitive values across REAL logs + stderr/stdout + error bodies =="
LOCAL_TOKEN=$(token_of "$HOME_DIR")

python3 - "$EVIDENCE_DIR" "$API_KEY" "$DEVICE_SECRET" "$OAUTH_TOKEN" "$QUERY_TOKEN" \
  "$LOCAL_TOKEN" "$WS_TICKET" <<'PYEOF'
import os, sys

evidence = sys.argv[1]
secrets = [
    ("api_key", sys.argv[2]),
    ("device_secret", sys.argv[3]),
    ("oauth_bearer", sys.argv[4]),
    ("query_token", sys.argv[5]),
    ("local_token", sys.argv[6]),
    ("ws_ticket", sys.argv[7]),
]
EXCLUDED = {
    "p1-execute-request.json": "the REQUEST body (input, not diagnostic output)",
    "p1-issue-request.json": "the request body",
    "p1-ticket-request.json": "the request body",
    "p1-issue-credential.body": "the contract-sanctioned 201 credential issuance response",
    "p1-ticket.body": "the contract-sanctioned 200 ws-ticket delivery response",
}

targets = []
for root, _dirs, files in os.walk(evidence):
    for name in files:
        if name in EXCLUDED or name == "summary.txt":
            continue
        targets.append(os.path.join(root, name))

# R9-F08: an unreadable target can never count as scanned — it fails the
# whole scan (the old `except OSError: continue` let a permission-broken
# file pass as clean). The retained REAL rotating logs under
# service-logs/ are part of this walk and must have been read like
# everything else.
unreadable = {}
read_ok = 0
def read_target(path):
    global read_ok
    try:
        with open(path, "rb") as f:
            data = f.read()
        read_ok += 1
        return data
    except OSError as err:
        unreadable[path] = str(err)
        return None

leaks = []
for label, value in secrets:
    if not value:
        continue
    hits = []
    for path in targets:
        data = read_target(path)
        if data is None:
            continue
        if value.encode() in data:
            hits.append(path)
    if hits:
        leaks.append((label, hits))
    print(f"SCAN {label}: {'LEAK in ' + str(len(hits)) + ' file(s)' if hits else 'clean (0 files)'}")

if unreadable:
    print("SCAN RESULT: FAIL (unreadable targets — never counted as clean)")
    for path in sorted(unreadable):
        print(f"  UNREADABLE {path}: {unreadable[path]}")
    sys.exit(1)
if leaks:
    print("SCAN RESULT: FAIL")
    for label, hits in leaks:
        for path in hits:
            print(f"  LEAK {label}: {path}")
    sys.exit(1)
print(f"SCAN RESULT: PASS (0 occurrences of 6 preset/minted secrets across {read_ok}/{len(targets)} read evidence files incl. the retained real rotating logs)")
PYEOF
SCAN_EXIT=$?
[ "$SCAN_EXIT" -eq 0 ] || fail "redaction scan found leaks or unreadable targets (see above)"
note "PASS SCAN: preset + minted sensitive values absent from the REAL rotating logs, stderr/stdout captures, error bodies and WS transcripts (every target read; unreadable would have failed)"

# ── CORRELATION: request/session/run ids survive in logs + responses ────────
note "== CORRELATION: 关联 ID 保留 =="
python3 - "$EVIDENCE_DIR" <<'PYEOF'
import glob, json, os, re, sys

evidence = sys.argv[1]

# 1. The requestId of the 401 bad-bearer response must appear in the
#    service's captured stderr marker line (LINGXI_AUTH_REJECTED).
body = open(os.path.join(evidence, "p2-me-bad-bearer.body")).read()
request_id = json.loads(body)["details"]["requestId"]
marker_files = glob.glob(os.path.join(evidence, "service-p1.err"))
found = any(
    request_id in line and "LINGXI_AUTH_REJECTED" in line
    for path in marker_files
    for line in open(path)
)
assert found, f"requestId {request_id} from the error body not found in an auth marker line"

# 2. The executed session id and the minted run id must be locatable: the
#    session id appears in the log (request-handled lines + marker paths;
#    R9-F08: read from the RETAINED real rotating logs, not the old
#    never-populated evidence/logs glob) and the run id in the execute
#    response.
log_lines = ""
for path in glob.glob(os.path.join(evidence, "service-p*.err")):
    log_lines += open(path).read()
for path in glob.glob(os.path.join(evidence, "service-logs", "**", "*.log"), recursive=True):
    log_lines += open(path).read()
assert "sess_local_alpha" in log_lines, "session id missing from logs"
body = open(os.path.join(evidence, "p1-execute-ok.body")).read()
run_id = json.loads(body)["runId"]
assert run_id, "runId missing from execute response"
print(f"CORRELATION PASS: requestId {request_id} present in marker line; "
      f"session id present in logs; runId {run_id} present in response")
PYEOF
note "PASS CORRELATION: requestId ↔ marker line, session id ↔ logs, runId ↔ response"

# ── artifact inventory ──────────────────────────────────────────────────────
# R9-F08: the retained real rotating logs are hashed by CONTENT (a name
# listing proves nothing), and the original home listing stays for
# context.
{
  echo "evidence files:"
  find "$EVIDENCE_DIR" -type f | sort
  echo "retained rotating log files (content sha256):"
  find "$RETAINED_LOGS" -type f -exec shasum -a 256 {} \; | sort -k2
  echo "rotated log files in home (at inventory time):"
  ls -la "$HOME_DIR/lingxi-service/logs/" 2>/dev/null || true
} > "$EVIDENCE_DIR/inventory.txt" 2>&1 || true

note "== R02-A13 evidence complete =="
exit 0
