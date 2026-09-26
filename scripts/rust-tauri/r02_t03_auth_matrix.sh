#!/usr/bin/env bash
# R02-T03 / acceptances R02-A05 + R02-A06 — HTTP/WS auth matrix against the
# REAL lingxi-service binary (not in-process mocks).
#
# Proves (A05 伪造身份无效):
#   - no-credential requests to the read/execute/ticket endpoints and the WS
#     upgrade are 401 and leave server state unchanged (runCount snapshot
#     before/after + auth registry file hashes before/after);
#   - forged identity headers never authenticate, and /me reflects the
#     SERVER-computed principal even when forged headers ride along;
#   - forged sessionId -> 404; expired device credential -> 401;
#   - a valid device credential for ANOTHER user reading/executing the
#     owner's session -> 403 with no side effects.
# Proves (A06 恶意网页无法借 loopback 越权):
#   - foreign Origin on HTTP (even the public health route) and on the WS
#     upgrade -> 403; Host tampering -> 403 (DNS-rebinding guard);
#   - expired / replayed WS tickets -> 401 invalid_ws_ticket;
#   - the legitimate desktop shape (allowed Origin + ticket) and the CLI
#     shape (NO Origin + bearer) keep working end to end (WS probe).
#
# Environment guards: same as r02_t02_dual_instance.sh (rustup-locked
# toolchain, task-dedicated target dir, offline locked build, proxy vars
# stripped, synthetic /tmp home only, no real user directory touched).
#
# Usage: scripts/rust-tauri/r02_t03_auth_matrix.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T03}"
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t03}"

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
    kill -TERM "$SERVICE_PID" 2>/dev/null || true
    wait "$SERVICE_PID" 2>/dev/null || true
  fi
  if [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ]; then rm -rf "$HOME_DIR"; fi
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

expect_code() { # $1=expected $2=actual $3=label
  if [ "$1" = "$2" ]; then
    note "PASS $3 (http=$2)"
  else
    fail "$3: expected HTTP $1, got $2"
  fi
}

expect_body() { # $1=needle $2=body $3=label
  if printf '%s' "$2" | grep -q "$1"; then
    note "PASS $3 (body contains '$1')"
  else
    fail "$3: body does not contain '$1': $2"
  fi
}

# ---- build the real binary -------------------------------------------------
note "== building lingxi-service (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  >"$EVIDENCE_DIR/build.log" 2>&1 \
  || { tail -30 "$EVIDENCE_DIR/build.log"; fail "build failed"; }
BIN="$TARGET_DIR/debug/lingxi-service"
[ -x "$BIN" ] || fail "binary not found at $BIN"

# ---- start the service on a synthetic home ---------------------------------
HOME_DIR="$(mktemp -d /tmp/lingxi-r02t03-home-XXXXXX)"
"$BIN" --home "$HOME_DIR" >"$EVIDENCE_DIR/server-stdout.log" 2>"$EVIDENCE_DIR/server-stderr.log" &
SERVICE_PID=$!

for _ in $(seq 1 300); do
  if grep -q "LINGXI_SERVICE_READY" "$EVIDENCE_DIR/server-stdout.log" 2>/dev/null; then break; fi
  if ! kill -0 "$SERVICE_PID" 2>/dev/null; then
    cat "$EVIDENCE_DIR/server-stderr.log" >&2
    fail "service died during startup"
  fi
  sleep 0.1
done
grep -q "LINGXI_SERVICE_READY" "$EVIDENCE_DIR/server-stdout.log" || fail "no READY line"
ADDR="$(sed -n 's/.*LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' "$EVIDENCE_DIR/server-stdout.log" | head -n 1)"
HOST="$(printf '%s' "$ADDR" | cut -d: -f1)"
PORT="$(printf '%s' "$ADDR" | cut -d: -f2)"
note "== service up addr=$ADDR home=$HOME_DIR =="

TOKEN_FILE="$HOME_DIR/lingxi-service/local-token.json"
TOKEN="$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['token'])" "$TOKEN_FILE")"
[ -${#TOKEN} -ne 32 ] 2>/dev/null || note "loopback token loaded from owner-only file (len ok)"
BEARER="Authorization: Bearer $TOKEN"

req() { # $1=method $2=path $3..=extra curl args; prints body, sets REPLY_CODE
  local method="$1"; local path="$2"; shift 2
  set +e
  REPLY="$(curl -sS -o "$EVIDENCE_DIR/.last-body" -w '%{http_code}' \
    --noproxy '*' -X "$method" "http://$ADDR$path" "$@")"
  set -e
  BODY="$(cat "$EVIDENCE_DIR/.last-body")"
}

registry_hashes() {
  (cd "$HOME_DIR/lingxi-service" && shasum -a 256 devices.json device-credentials.json local-token.json 2>/dev/null) \
    | tee "$1"
}

run_counts() { # prints "<alpha> <beta>" as the owner sees them
  req GET "/lingxi/v1/sessions/sess_local_alpha" -H "$BEARER"
  [ "$REPLY" = 200 ] || fail "owner snapshot read failed ($REPLY): $BODY"
  local alpha beta
  alpha="$(printf '%s' "$BODY" | python3 -c 'import json,sys;print(json.load(sys.stdin)["runCount"])')"
  req GET "/lingxi/v1/sessions/sess_local_beta" -H "$BEARER"
  [ "$REPLY" = 200 ] || fail "owner snapshot read failed ($REPLY): $BODY"
  beta="$(printf '%s' "$BODY" | python3 -c 'import json,sys;print(json.load(sys.stdin)["runCount"])')"
  echo "$alpha $beta"
}

# =========================================================================
note "== A05: forged identity is ineffective =="
# =========================================================================

# 1. no credential: read / execute / ticket / me.
req GET "/lingxi/v1/sessions/sess_local_alpha"
expect_code 401 "$REPLY" "a05-no-credential-read"
expect_body "missing_credential" "$BODY" "a05-no-credential-read-reason"
req POST "/lingxi/v1/sessions/sess_local_alpha/execute" -H 'Content-Type: application/json' -d '{"input":"forged"}'
expect_code 401 "$REPLY" "a05-no-credential-execute"
req POST "/lingxi/v1/ws-ticket"
expect_code 401 "$REPLY" "a05-no-credential-ws-ticket"
req GET "/lingxi/v1/me"
expect_code 401 "$REPLY" "a05-no-credential-me"

# 2. forged principal headers do not authenticate...
req GET "/lingxi/v1/sessions/sess_local_alpha" \
  -H 'X-Lingxi-Principal: principal_device_forged' -H 'X-Lingxi-User: user_victim'
expect_code 401 "$REPLY" "a05-forged-principal-headers"
# ...and with a VALID token the server-computed principal wins.
req GET "/lingxi/v1/me" -H "$BEARER" -H 'X-Lingxi-Principal: principal_device_forged'
expect_code 200 "$REPLY" "a05-me-with-forged-header"
expect_body '"kind":"local_user"' "$BODY" "a05-me-server-computed"
FORGED_CHECK="$(printf '%s' "$BODY" | grep -c 'forged' || true)"
[ "$FORGED_CHECK" = "0" ] || fail "forged principal echoed in /me: $BODY"
note "PASS a05-me-never-echoes-forged-values"

# 3. identity fields in the execute body are a parse error, not an execution.
req POST "/lingxi/v1/sessions/sess_local_alpha/execute" -H "$BEARER" \
  -H 'Content-Type: application/json' \
  -d '{"input":"x","principalId":"forged","userId":"user_victim"}'
expect_code 400 "$REPLY" "a05-identity-shaped-body-rejected"
expect_body "invalid_message" "$BODY" "a05-identity-shaped-body-reason"

# 4. forged/unknown session ids.
req GET "/lingxi/v1/sessions/sess_does_not_exist" -H "$BEARER"
expect_code 404 "$REPLY" "a05-forged-session-id"
expect_body "not_found" "$BODY" "a05-forged-session-id-reason"
req POST "/lingxi/v1/sessions/sess_does_not_exist/execute" -H "$BEARER" \
  -H 'Content-Type: application/json' -d '{"input":"x"}'
expect_code 404 "$REPLY" "a05-forged-session-id-execute"

# 5. garbage token.
req GET "/lingxi/v1/sessions/sess_local_alpha" -H "Authorization: Bearer 00000000000000000000000000000000"
expect_code 401 "$REPLY" "a05-wrong-token"
expect_body "invalid_credential" "$BODY" "a05-wrong-token-reason"

# 6. EXPIRED device credential (owner-only management route mints it).
req POST "/lingxi/v1/devices/credentials" -H "$BEARER" -H 'Content-Type: application/json' \
  -d '{"userId":"user_remote","scopes":["chat"],"expiresAtUnixMs":1}'
expect_code 201 "$REPLY" "a05-issue-expired-credential"
EXPIRED_SECRET="$(printf '%s' "$BODY" | python3 -c 'import json,sys;print(json.load(sys.stdin)["secret"])')"

# 7. cross-principal: valid device credential for ANOTHER user; warm its
#    (authorized) lastUsedAt write BEFORE the invariance snapshot.
req POST "/lingxi/v1/devices/credentials" -H "$BEARER" -H 'Content-Type: application/json' \
  -d '{"userId":"user_remote_b","scopes":["chat"]}'
expect_code 201 "$REPLY" "a05-issue-foreign-credential"
FOREIGN_SECRET="$(printf '%s' "$BODY" | python3 -c 'import json,sys;print(json.load(sys.stdin)["secret"])')"
req GET "/lingxi/v1/me" -H "Authorization: Bearer $FOREIGN_SECRET"
expect_code 200 "$REPLY" "a05-foreign-credential-authenticates"
expect_body '"kind":"device"' "$BODY" "a05-foreign-principal-kind"

# ---- state snapshot: everything below this line must be DENIALS only ----
# Two invariance classes (documented in the task report):
#   (a) AUTHENTICATION failures (no/wrong/expired credential) must not touch
#       the registries at all -> byte-identical files;
#   (b) AUTHORIZATION failures (valid device credential, foreign session)
#       legitimately record lastUsedAt on successful AUTHENTICATION (mirrors
#       the incumbent device registry) -> files may differ ONLY in the
#       audit fields (lastUsedAtUnixMs/lastSeenAtUnixMs/updatedAtUnixMs).
registry_hashes "$EVIDENCE_DIR/state-before.txt"
COUNTS_BEFORE="$(run_counts)"
note "state before negatives: runCounts=$COUNTS_BEFORE"

req GET "/lingxi/v1/sessions/sess_local_alpha" -H "Authorization: Bearer $EXPIRED_SECRET"
expect_code 401 "$REPLY" "a05-expired-credential"
expect_body "invalid_credential" "$BODY" "a05-expired-credential-reason"
req POST "/lingxi/v1/sessions/sess_local_alpha/execute" -H "Authorization: Bearer $EXPIRED_SECRET" \
  -H 'Content-Type: application/json' -d '{"input":"x"}'
expect_code 401 "$REPLY" "a05-expired-credential-execute"

# (a) authentication failures never touched the registries.
registry_hashes "$EVIDENCE_DIR/state-after-authn.txt"
if ! diff -q "$EVIDENCE_DIR/state-before.txt" "$EVIDENCE_DIR/state-after-authn.txt" >/dev/null; then
  diff "$EVIDENCE_DIR/state-before.txt" "$EVIDENCE_DIR/state-after-authn.txt" | head -20
  fail "auth registry files changed during authentication-failure negatives"
fi
note "PASS a05-authn-failures-registry-files-byte-identical"

registry_hashes "$EVIDENCE_DIR/state-before-authz.txt"
cp "$HOME_DIR/lingxi-service/devices.json" "$EVIDENCE_DIR/devices-before-authz.json"
cp "$HOME_DIR/lingxi-service/device-credentials.json" "$EVIDENCE_DIR/creds-before-authz.json"

req GET "/lingxi/v1/sessions/sess_local_alpha" -H "Authorization: Bearer $FOREIGN_SECRET"
expect_code 403 "$REPLY" "a05-cross-principal-read"
expect_body "cross_principal_access" "$BODY" "a05-cross-principal-read-reason"
req POST "/lingxi/v1/sessions/sess_local_alpha/execute" -H "Authorization: Bearer $FOREIGN_SECRET" \
  -H 'Content-Type: application/json' -d '{"input":"hijack"}'
expect_code 403 "$REPLY" "a05-cross-principal-execute"

# (b) authorization failures changed nothing but the audit fields.
registry_hashes "$EVIDENCE_DIR/state-after.txt"
cp "$HOME_DIR/lingxi-service/devices.json" "$EVIDENCE_DIR/devices-after-authz.json"
cp "$HOME_DIR/lingxi-service/device-credentials.json" "$EVIDENCE_DIR/creds-after-authz.json"
python3 - "$EVIDENCE_DIR" << 'PYEOF' | tee -a "$EVIDENCE_DIR/summary.txt"
import json, os, sys
ev = sys.argv[1]
AUDIT_FIELDS = {"lastUsedAtUnixMs", "lastSeenAtUnixMs", "updatedAtUnixMs"}
def strip_audit(node):
    if isinstance(node, dict):
        return {k: strip_audit(v) for k, v in node.items() if k not in AUDIT_FIELDS}
    if isinstance(node, list):
        return [strip_audit(v) for v in node]
    return node
def load(path):
    with open(path) as fh:
        return json.load(fh)
ok = True
for pre_name, post_name in (
    ("devices-before-authz.json", "devices-after-authz.json"),
    ("creds-before-authz.json", "creds-after-authz.json"),
):
    pre = strip_audit(load(os.path.join(ev, pre_name)))
    post = strip_audit(load(os.path.join(ev, post_name)))
    same = pre == post
    print(("PASS" if same else "FAIL"),
          f"a05-authz-negatives-audit-only-changes ({pre_name} vs {post_name})")
    if not same:
        ok = False
        print("pre :", json.dumps(pre, ensure_ascii=False)[:400])
        print("post:", json.dumps(post, ensure_ascii=False)[:400])
sys.exit(0 if ok else 1)
PYEOF
[ "${PIPESTATUS[0]:-0}" = 0 ] || fail "authorization negatives changed non-audit registry content"

# runCounts unchanged across ALL negatives.
COUNTS_AFTER="$(run_counts)"
note "state after negatives:  runCounts=$COUNTS_AFTER"
[ "$COUNTS_BEFORE" = "$COUNTS_AFTER" ] || fail "run counts changed ($COUNTS_BEFORE -> $COUNTS_AFTER)"
note "PASS a05-no-observable-business-state-change (runCounts stable)"

# 9. positive control: the owner CAN execute (proves the chain is live, not
#    a blanket-deny), then runCount advanced by exactly one.
req POST "/lingxi/v1/sessions/sess_local_alpha/execute" -H "$BEARER" \
  -H 'Content-Type: application/json' -d '{"input":"positive control"}'
expect_code 200 "$REPLY" "a05-positive-control-owner-execute"
expect_body '"runId":"run_' "$BODY" "a05-positive-control-run-id"
COUNTS_CONTROL="$(run_counts)"
ALPHA_BEFORE="$(printf '%s' "$COUNTS_BEFORE" | cut -d' ' -f1)"
ALPHA_AFTER="$(printf '%s' "$COUNTS_CONTROL" | cut -d' ' -f1)"
[ "$((ALPHA_AFTER - ALPHA_BEFORE))" = 1 ] || fail "positive control did not advance runCount by 1"
note "PASS a05-positive-control-advanced-run-count (owner execute works, denied ones did not)"

# =========================================================================
note "== A06: malicious web page cannot ride loopback =="
# =========================================================================

# 1. foreign Origin — even on the public health route.
req GET "/lingxi/v1/health" -H 'Origin: http://evil.example'
expect_code 403 "$REPLY" "a06-evil-origin-health"
expect_body "bad_origin" "$BODY" "a06-evil-origin-health-reason"
req GET "/lingxi/v1/me" -H 'Origin: https://attacker.invalid' -H "$BEARER"
expect_code 403 "$REPLY" "a06-evil-origin-with-valid-token"

# 2. allowed Origin shapes work on health.
for ORIGIN in "http://localhost:$PORT" "http://127.0.0.1:$PORT" "file://" "null"; do
  req GET "/lingxi/v1/health" -H "Origin: $ORIGIN"
  expect_code 200 "$REPLY" "a06-allowed-origin-$ORIGIN"
done

# 3. rebinding-shaped origins are rejected.
for ORIGIN in "http://sub.localhost:1" "http://127.0.0.1.evil.example"; do
  req GET "/lingxi/v1/health" -H "Origin: $ORIGIN"
  expect_code 403 "$REPLY" "a06-rebinding-origin-$ORIGIN"
done

# 4. Host tampering (raw sockets; curl cannot send an arbitrary Host easily).
python3 - "$HOST" "$PORT" "$TOKEN" << 'PYEOF' | tee -a "$EVIDENCE_DIR/summary.txt"
import socket, sys
host, port, token = sys.argv[1], int(sys.argv[2]), sys.argv[3]
def raw(request):
    s = socket.create_connection((host, port), timeout=5)
    s.sendall(request.encode())
    buf = b""
    while True:
        chunk = s.recv(4096)
        if not chunk:
            break
        buf += chunk
    s.close()
    return buf.decode("utf-8", "replace")
cases = [
    ("a06-foreign-host-health",
     "GET /lingxi/v1/health HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n",
     403, "loopback_host_mismatch"),
    ("a06-foreign-host-with-token",
     f"GET /lingxi/v1/me HTTP/1.1\r\nHost: evil.example\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n",
     403, None),
    ("a06-localhost-host-ok",
     "GET /lingxi/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
     200, None),
]
failed = False
for name, request, expected, needle in cases:
    text = raw(request)
    status = int(text.split()[1]) if len(text.split()) > 1 else 0
    ok = status == expected and (needle is None or needle in text)
    print(("PASS" if ok else "FAIL"), name, f"(http={status})")
    if not ok:
        failed = True
        print(text[:300])
sys.exit(1 if failed else 0)
PYEOF

# 5. rate limiting live check: hammer health until 429 (240/10s budget),
#    then let the window expire so later probes are not polluted.
SAW_429=0
for _ in $(seq 1 300); do
  CODE="$(curl -sS -o /dev/null -w '%{http_code}' --noproxy '*' "http://$ADDR/lingxi/v1/health")"
  if [ "$CODE" = "429" ]; then SAW_429=1; break; fi
done
[ "$SAW_429" = 1 ] || fail "rate limiter never answered 429"
note "PASS a06-rate-limit-live (429 observed)"
sleep 11
note "rate window expired; continuing"

# 6. body limit live check: >1MiB execute body -> 413.
python3 - "$HOST" "$PORT" "$TOKEN" > "$EVIDENCE_DIR/body-limit.txt" << 'PYEOF'
import socket, sys
host, port, token = sys.argv[1], int(sys.argv[2]), sys.argv[3]
body = b'{"input":"' + b'x' * (1024 * 1024 + 64) + b'"}'
head = (
    f"POST /lingxi/v1/sessions/sess_local_alpha/execute HTTP/1.1\r\nHost: {host}:{port}\r\n"
    f"Authorization: Bearer {token}\r\nContent-Type: application/json\r\n"
    f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n"
).encode()
s = socket.create_connection((host, port), timeout=10)
s.sendall(head)
try:
    # Send in chunks; a 413-rejecting server may reset mid-body — that is
    # the expected outcome, not an error.
    for i in range(0, len(body), 8192):
        s.sendall(body[i:i + 8192])
except (BrokenPipeError, ConnectionResetError):
    pass
buf = b""
try:
    while True:
        chunk = s.recv(4096)
        if not chunk:
            break
        buf += chunk
except ConnectionResetError:
    pass
s.close()
text = buf.decode("utf-8", "replace")
print(text.split("\r\n", 1)[0] if text else "EMPTY")
status = int(text.split()[1]) if len(text.split()) > 1 else 0
sys.exit(0 if status == 413 else 1)
PYEOF
grep -q "413" "$EVIDENCE_DIR/body-limit.txt" || fail "oversized body not rejected with 413"
note "PASS a06-body-limit-live (413 observed)"

# 7. WS matrix (ticket lifecycle / origins / CLI shape) via the real probe.
#    NOTE: the expired-ticket case waits out the real 30s ticket TTL.
note "== WS matrix (includes a real 30s ticket-TTL wait) =="
env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u all_proxy -u ALL_PROXY \
  python3 scripts/rust-tauri/r02_t03_ws_probe.py "$HOST" "$PORT" "$TOKEN" \
  > "$EVIDENCE_DIR/ws-matrix.json" \
  || { cat "$EVIDENCE_DIR/ws-matrix.json"; fail "WS matrix failed"; }
python3 -c '
import json, sys
data = json.load(open(sys.argv[1]))
for r in data["results"]:
    print(("PASS" if r["ok"] else "FAIL"), r["case"],
          "(expected=%s actual=%s)" % (r["expected"], r["actual"]))
sys.exit(0 if data["all_ok"] else 1)
' "$EVIDENCE_DIR/ws-matrix.json" | tee -a "$EVIDENCE_DIR/summary.txt"

# 8. CLI shape end-to-end on HTTP (no Origin, bearer, query token).
req GET "/lingxi/v1/sessions/sess_local_alpha" -H "$BEARER"
expect_code 200 "$REPLY" "a06-cli-no-origin-owner-read"
req GET "/lingxi/v1/me?token=$TOKEN"
expect_code 200 "$REPLY" "a06-cli-query-token-local"

# 9. negative startup shape: non-loopback bind without --network-mode lan.
set +e
"$BIN" --home "$HOME_DIR-startup-neg" --bind "0.0.0.0:8080" >"$EVIDENCE_DIR/neg-startup.log" 2>&1
NEG_RC=$?
set -e
[ "$NEG_RC" = 2 ] || fail "non-loopback bind under default mode must exit 2 (got $NEG_RC)"
grep -q "network-mode lan" "$EVIDENCE_DIR/neg-startup.log" \
  || fail "startup error must name the explicit opt-in"
note "PASS a06-non-loopback-bind-requires-explicit-lan-mode (exit 2)"
rm -rf "$HOME_DIR-startup-neg"

# ---- shutdown hygiene -------------------------------------------------------
kill -TERM "$SERVICE_PID"
wait "$SERVICE_PID" 2>/dev/null || true
SERVICE_PID=""
MARKERS="$(grep -c "LINGXI_AUTH_REJECTED\|LINGXI_TRANSPORT_REJECTED" "$EVIDENCE_DIR/server-stderr.log" || true)"
note "negative protocol log markers on stderr: ${MARKERS} lines"
note "== ALL CASES PASSED =="