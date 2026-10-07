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
cd "$J_REVIEW_COPY"

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T03}"
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
TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/rust-target-r02-t03}"

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
# R02-final G3: 完成标志。本机 bash 3.2.57 上 set -u 崩溃（unbound
# variable）进入 EXIT trap 时 $? 已经是 0，单独 `trap cleanup EXIT` 会让
# 崩溃的脚本以 0 退出（fail-open）。trap 现在先记录 rc、跑原封不动的
# cleanup、再显式以 rc 退出；若脚本从未到达末行（未完成），即使 rc 被
# 崩溃路径丢失也强制非零。cleanup 残留分支的 exit 1 仍然直接生效。
SCRIPT_COMPLETED=0
# R02 stage-repair R7 / R7-F02: SERVICE_PID is the CURRENT handle of this
# run's service child — retired (cleared) after every wait/reap (the
# shutdown-hygiene path already did). The trap signals it ONLY while it
# still proves CURRENT ownership (exists AND ppid is THIS shell): a
# retired or recycled number is never signalled (R6-F02 A12 pattern).
# R12-F01: the boolean probe's false branch conflated exited/foreign/
# unobservable and the trap's owned branch was `kill -TERM; wait` with
# NO deadline — a child ignoring TERM hung the trap itself. Four-state
# probe + bounded ladder below (≈10 s worst case per handle); residue is
# reported loudly at expiry, never an unbounded wait.
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
# contract (TERM first — the normal shutdown-hygiene stop asserts the
# graceful path and stays untouched): TERM → ≤5 s poll → direct-pid KILL
# only while still provably ours → ≤5 s re-check. Prints the final state.
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
  if [ "$cleanup_residue" -eq 0 ]; then
    if [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ]; then rm -rf "$HOME_DIR"; fi
  else
    echo "cleanup: 进程仍存活或归属不明，保留本轮 home=$HOME_DIR 供核查" >&2
    exit 1
  fi
  return 0
}
trap 'rc=$?; cleanup; if [ "$SCRIPT_COMPLETED" -ne 1 ] && [ "$rc" -eq 0 ]; then rc=1; fi; exit "$rc"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { printf '%s\n' "$*" | tee -a "$EVIDENCE_DIR/summary.txt"; }

# R14-F01 (R02 stage-repair R14): per-case structured records for the R00
# supplemental-leaf gate. Every expect_code assertion appends one ndjson line
# here; the assembler at the end of the script merges these with the WS
# matrix and the host-tampering records into leaf-cases.json — the machine
# contract the stage gate consumes per leaf (verify-stage checks the ACTUAL
# value of each declared case against the leaf's original assertion, so a
# green command exit alone can never pass a leaf whose assertion failed).
CASES_NDJSON="$EVIDENCE_DIR/leaf-cases.ndjson"
: > "$CASES_NDJSON"
record_case() { # $1=case $2=expect $3=actual $4=ok(1/0)
  python3 -c 'import json,sys
print(json.dumps({"case": sys.argv[1], "expect": int(sys.argv[2]),
                  "actual": int(sys.argv[3]), "ok": sys.argv[4] == "1"}))' \
    "$1" "$2" "$3" "$4" >> "$CASES_NDJSON"
}

expect_code() { # $1=expected $2=actual $3=label
  if [ "$1" = "$2" ]; then
    note "PASS $3 (http=$2)"
    record_case "$3" "$1" "$2" 1
  else
    record_case "$3" "$1" "$2" 0
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

# /me 的原始叶项要求版本、身份和按主体投影的能力都真实对应。
assert_me_projection() { # $1=case $2=kind $3=user $4=credential
  local case_name="$1" expected_kind="$2" expected_user="$3" expected_credential="$4" actual=0
  if printf '%s' "$BODY" | python3 -c 'import json,sys
value=json.load(sys.stdin)
kind,user,credential=sys.argv[1:]
principal=value["principal"]
scopes=principal["scopes"]
assert isinstance(scopes,list) and all(isinstance(s,str) and s for s in scopes)
assert value["kind"] == kind and value["userId"] == user
assert value["credentialKind"] == credential
assert value["principalId"] == principal["principalId"]
assert value["kind"] == principal["kind"] and value["userId"] == principal["userId"]
assert value["credentialKind"] == principal["credentialKind"]
assert value["scopes"] == scopes
assert isinstance(value["serverVersion"],str) and value["serverVersion"]
assert value["version"] == value["serverVersion"]
assert value["serverNodeKind"] == "lingxi-service"
assert isinstance(value["serverId"],str) and value["serverId"]
assert value["serverNodeId"] == value["serverId"]
assert isinstance(value["studioId"],str) and value["studioId"]
assert set(value["capabilities"]) == set(scopes) | {s.split(".")[0] for s in scopes}
assert "secret" not in value and "token" not in value
' "$expected_kind" "$expected_user" "$expected_credential" 2>/dev/null; then
    actual=1
  fi
  record_case "$case_name" 1 "$actual" "$actual"
  [ "$actual" -eq 1 ] || fail "$case_name: /me version, identity, or scoped capability projection mismatch"
  note "PASS $case_name (version, identity, scoped capabilities)"
}

# ---- build the real binary -------------------------------------------------
note "== building lingxi-service (rustup $TOOLCHAIN, $TARGET_DIR, --locked) =="
printf "%s\n" "Independent review: exact H02 current-source binary reused; no Cargo build" > "$EVIDENCE_DIR/build.log"
BIN="$TARGET_DIR/debug/lingxi-service"
[ -x "$BIN" ] || fail "binary not found at $BIN"

# ---- start the service on a synthetic home ---------------------------------
HOME_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r02t03-home-XXXXXX")
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
# R14-F01: R00 leaf R00-T02-LA-5816DA563ED8 pins the ORIGINAL assertion for
# this route — "无主体返回 403" (incumbent server/routes/ws-auth.ts). The
# service now honors it (auth_guard answers an unauthenticated POST
# /lingxi/v1/ws-ticket with 403); the read/execute/me routes above keep 401.
req POST "/lingxi/v1/ws-ticket"
expect_code 403 "$REPLY" "a05-no-credential-ws-ticket"
expect_body "missing_credential" "$BODY" "a05-no-credential-ws-ticket-reason"
registry_hashes "$EVIDENCE_DIR/me-denied-before.sha256"
req GET "/lingxi/v1/me"
expect_code 401 "$REPLY" "a05-no-credential-me"
registry_hashes "$EVIDENCE_DIR/me-denied-after.sha256"
ME_DENIED_UNCHANGED=0
if cmp -s "$EVIDENCE_DIR/me-denied-before.sha256" "$EVIDENCE_DIR/me-denied-after.sha256"; then
  ME_DENIED_UNCHANGED=1
fi
record_case "a05-me-denied-no-registry-write" 1 "$ME_DENIED_UNCHANGED" "$ME_DENIED_UNCHANGED"
[ "$ME_DENIED_UNCHANGED" -eq 1 ] || fail "/me denial changed credential registries"

# 2. forged principal headers do not authenticate...
req GET "/lingxi/v1/sessions/sess_local_alpha" \
  -H 'X-Lingxi-Principal: principal_device_forged' -H 'X-Lingxi-User: user_victim'
expect_code 401 "$REPLY" "a05-forged-principal-headers"
# ...and with a VALID token the server-computed principal wins.
req GET "/lingxi/v1/me" -H "$BEARER" -H 'X-Lingxi-Principal: principal_device_forged'
expect_code 200 "$REPLY" "a05-me-with-forged-header"
expect_body '"kind":"local_user"' "$BODY" "a05-me-server-computed"
assert_me_projection "a05-me-owner-full-projection" local_user user_local loopback_token
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

# 6b. R14-F01 leaf cases: ws-ticket positive issuance (R00-T02-LA-5816DA563ED8
#     "读取已认证主体并签发…短期票据") and devices/credentials invalid-input
#     negative (R00-T02-LA-093F22C4FF63 "无效输入显式拒绝" — 400, no secret in
#     the body, no registry write).
req POST "/lingxi/v1/ws-ticket" -H "$BEARER" -H 'Content-Type: application/json' -d '{}'
expect_code 200 "$REPLY" "a05-ws-ticket-issue-owner"
expect_body '"ticket":"' "$BODY" "a05-ws-ticket-issue-owner-shape"
req POST "/lingxi/v1/devices/credentials" -H "$BEARER" -H 'Content-Type: application/json' \
  -d '{"userId":"   ","scopes":[]}'
expect_code 400 "$REPLY" "a05-devices-credentials-empty-user-id"
req POST "/lingxi/v1/devices/credentials" -H "$BEARER" -H 'Content-Type: application/json' \
  -d 'not-json]'
expect_code 400 "$REPLY" "a05-devices-credentials-invalid-json"
expect_body "invalid_message" "$BODY" "a05-devices-credentials-invalid-json-reason"

# 7. cross-principal: valid device credential for ANOTHER user; warm its
#    (authorized) lastUsedAt write BEFORE the invariance snapshot.
req POST "/lingxi/v1/devices/credentials" -H "$BEARER" -H 'Content-Type: application/json' \
  -d '{"userId":"user_remote_b","scopes":["chat"]}'
expect_code 201 "$REPLY" "a05-issue-foreign-credential"
FOREIGN_SECRET="$(printf '%s' "$BODY" | python3 -c 'import json,sys;print(json.load(sys.stdin)["secret"])')"
req GET "/lingxi/v1/me" -H "Authorization: Bearer $FOREIGN_SECRET"
expect_code 200 "$REPLY" "a05-foreign-credential-authenticates"
expect_body '"kind":"device"' "$BODY" "a05-foreign-principal-kind"
assert_me_projection "a05-me-device-full-projection" device user_remote_b device_credential
req POST "/lingxi/v1/devices/credentials" -H "$BEARER" -H 'Content-Type: application/json' \
  -d '{"userId":"user_no_chat","scopes":["resources.read"]}'
expect_code 201 "$REPLY" "a05-issue-no-chat-credential"
NO_CHAT_SECRET="$(printf '%s' "$BODY" | python3 -c 'import json,sys;print(json.load(sys.stdin)["secret"])')"
req POST "/lingxi/v1/ws-ticket" -H "Authorization: Bearer $NO_CHAT_SECRET" \
  -H 'Content-Type: application/json' -d '{}'
expect_code 403 "$REPLY" "a05-ws-ticket-insufficient-scope"
expect_body "insufficient_scope" "$BODY" "a05-ws-ticket-insufficient-scope-reason"

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

# 10. R14-F01 sessions-list leaf cases (R00-T02-LA-200D4E5D52C9): the
#     principal-scoped list route GET /lingxi/v1/sessions had NO registered
#     producer before — this section is that producer. Positive: the owner
#     lists its own session; the foreign device principal gets a 200 whose
#     list excludes the owner's sessions and is the empty-list shape (the
#     server-side share of "空列表"); negative: no credential -> 401.
#     Runs AFTER the invariance snapshots (the foreign read legitimately
#     touches that credential's audit fields).
req GET "/lingxi/v1/sessions" -H "$BEARER"
expect_code 200 "$REPLY" "a05-sessions-list-owner"
OWNER_SESS_HITS="$(printf '%s' "$BODY" | grep -c 'sess_local_alpha' || true)"
[ "$OWNER_SESS_HITS" -ge 1 ] || fail "owner sessions list does not contain sess_local_alpha: $BODY"
record_case "a05-sessions-list-owner-contains-own-session" 1 1 1
note "PASS a05-sessions-list-owner-contains-own-session"
req GET "/lingxi/v1/sessions" -H "Authorization: Bearer $FOREIGN_SECRET"
expect_code 200 "$REPLY" "a05-sessions-list-foreign-principal"
FOREIGN_SESS_HITS="$(printf '%s' "$BODY" | grep -c 'sess_local_alpha' || true)"
[ "$FOREIGN_SESS_HITS" = 0 ] || fail "foreign principal sees the owner's sessions: $BODY"
record_case "a05-sessions-list-foreign-excludes-owner-sessions" 0 "$FOREIGN_SESS_HITS" 1
note "PASS a05-sessions-list-foreign-excludes-owner-sessions"
EMPTY_SHAPE_HITS="$(printf '%s' "$BODY" | grep -c '"sessions":\[\]' || true)"
[ "$EMPTY_SHAPE_HITS" -ge 1 ] || fail "foreign principal list is not the empty-list shape: $BODY"
record_case "a05-sessions-list-empty-shape" 1 "$EMPTY_SHAPE_HITS" 1
note "PASS a05-sessions-list-empty-shape"
req GET "/lingxi/v1/sessions"
expect_code 401 "$REPLY" "a05-sessions-list-no-credential"
expect_body "missing_credential" "$BODY" "a05-sessions-list-no-credential-reason"
# Leaf-dedicated matrix file (pinned by the stage map as the leaf's own
# evidence artifact; the gate checks content, not just existence).
python3 - "$CASES_NDJSON" "$EVIDENCE_DIR/sessions-list-matrix.json" << 'PYEOF'
import json, sys
cases = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
leaf_cases = [c for c in cases if c["case"].startswith("a05-sessions-list-")
              and not c["case"].endswith("-reason")]
matrix = {
    "schema": "lingxi.leaf-case-results.v1",
    "leafId": "R00-T02-LA-200D4E5D52C9",
    "route": "GET /lingxi/v1/sessions",
    "cases": leaf_cases,
}
with open(sys.argv[2], "w") as fh:
    json.dump(matrix, fh, ensure_ascii=False, indent=1)
print(f"sessions-list-matrix.json: {len(leaf_cases)} cases")
PYEOF

# 11. 同一真实服务的 CLI 消费者。服务端列表成功不能证明 CLI 的空态、
#     错误出口或主体隔离；每种身份都从独立 CLI 进程读取，并保留原始输出。
command -v node >/dev/null 2>&1 || fail "Node is required for the CLI sessions leaf"
CLI_URL="http://$ADDR"
if node cli/entry.ts sessions --runtime rust --url "$CLI_URL" --token "$TOKEN" \
  >"$EVIDENCE_DIR/cli-sessions-owner.stdout.log" 2>"$EVIDENCE_DIR/cli-sessions-owner.stderr.log"; then
  CLI_OWNER_RC=0
else
  CLI_OWNER_RC=$?
fi
python3 - "$EVIDENCE_DIR/cli-sessions-owner.stdout.log" "$CLI_OWNER_RC" "$CASES_NDJSON" << 'PYEOF'
import json, re, sys
lines = [line.strip() for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
numbered = [line for line in lines if re.match(r"^\d+\. ", line)]
actual = int(sys.argv[2] == "0" and len(lines) == 2 and len(numbered) == 2
             and any("Synthetic session alpha" in line for line in numbered)
             and any("Synthetic session beta" in line for line in numbered))
with open(sys.argv[3], "a", encoding="utf-8") as out:
    out.write(json.dumps({"case": "a05-cli-sessions-owner-list", "expect": 1,
                          "actual": actual, "ok": bool(actual),
                          "observed": {"exit": int(sys.argv[2]), "lineCount": len(lines),
                                       "numberedCount": len(numbered)}}) + "\n")
if not actual: raise SystemExit("CLI owner sessions output does not match the real owner's two sessions")
PYEOF
note "PASS a05-cli-sessions-owner-list"

if node cli/entry.ts sessions --runtime rust --url "$CLI_URL" --token "$FOREIGN_SECRET" \
  >"$EVIDENCE_DIR/cli-sessions-foreign.stdout.log" 2>"$EVIDENCE_DIR/cli-sessions-foreign.stderr.log"; then
  CLI_FOREIGN_RC=0
else
  CLI_FOREIGN_RC=$?
fi
python3 - "$EVIDENCE_DIR/cli-sessions-foreign.stdout.log" "$CLI_FOREIGN_RC" "$CASES_NDJSON" << 'PYEOF'
import json, sys
text = open(sys.argv[1], encoding="utf-8").read().strip()
actual = int(sys.argv[2] == "0" and text == "No sessions yet." and "Synthetic session" not in text)
with open(sys.argv[3], "a", encoding="utf-8") as out:
    out.write(json.dumps({"case": "a05-cli-sessions-foreign-empty", "expect": 1,
                          "actual": actual, "ok": bool(actual),
                          "observed": {"exit": int(sys.argv[2]), "emptyMessage": text == "No sessions yet."}}) + "\n")
if not actual: raise SystemExit("CLI foreign principal did not show the real empty list")
PYEOF
note "PASS a05-cli-sessions-foreign-empty"

if node cli/entry.ts sessions --runtime rust --url "$CLI_URL" \
  >"$EVIDENCE_DIR/cli-sessions-unauthorized.stdout.log" 2>"$EVIDENCE_DIR/cli-sessions-unauthorized.stderr.log"; then
  CLI_UNAUTH_RC=0
else
  CLI_UNAUTH_RC=$?
fi
python3 - "$EVIDENCE_DIR/cli-sessions-unauthorized.stdout.log" \
  "$EVIDENCE_DIR/cli-sessions-unauthorized.stderr.log" "$CLI_UNAUTH_RC" "$CASES_NDJSON" << 'PYEOF'
import json, sys
stdout = open(sys.argv[1], encoding="utf-8").read()
stderr = open(sys.argv[2], encoding="utf-8").read()
actual = int(sys.argv[3] == "1" and not stdout.strip() and "HTTP 401" in stderr
             and "Synthetic session" not in stderr)
with open(sys.argv[4], "a", encoding="utf-8") as out:
    out.write(json.dumps({"case": "a05-cli-sessions-unauthorized-error", "expect": 1,
                          "actual": actual, "ok": bool(actual),
                          "observed": {"exit": int(sys.argv[3]), "http401": "HTTP 401" in stderr,
                                       "stdoutEmpty": not stdout.strip()}}) + "\n")
if not actual: raise SystemExit("CLI without credential did not show an error without session data")
PYEOF
note "PASS a05-cli-sessions-unauthorized-error"
node scripts/rust-tauri/r02_cli_sessions_limit.mjs \
  >"$EVIDENCE_DIR/cli-sessions-limit-case.json" \
  2>"$EVIDENCE_DIR/cli-sessions-limit.stderr.log" \
  || fail "CLI failed to cap a 25-session response at 20 rendered sessions"
python3 - "$EVIDENCE_DIR/cli-sessions-limit-case.json" "$CASES_NDJSON" << 'PYEOF'
import json, sys
case = json.load(open(sys.argv[1], encoding="utf-8"))
if case.get("case") != "a05-cli-sessions-limit-20" or case.get("ok") is not True \
        or case.get("expect") != 1 or case.get("actual") != 1:
    raise SystemExit("CLI sessions limit producer returned an incomplete case")
with open(sys.argv[2], "a", encoding="utf-8") as out:
    out.write(json.dumps(case) + "\n")
PYEOF
python3 - "$CASES_NDJSON" "$EVIDENCE_DIR/sessions-list-matrix.json" << 'PYEOF'
import json, sys
cases = [json.loads(line) for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
matrix = json.load(open(sys.argv[2], encoding="utf-8"))
client = [case for case in cases if case["case"].startswith("a05-cli-sessions-")]
expected = {"a05-cli-sessions-owner-list", "a05-cli-sessions-foreign-empty",
            "a05-cli-sessions-unauthorized-error", "a05-cli-sessions-limit-20"}
if len(client) != len(expected) or {case["case"] for case in client} != expected:
    raise SystemExit("CLI sessions evidence identities are incomplete or duplicated")
matrix["cases"].extend(client)
with open(sys.argv[2], "w", encoding="utf-8") as out:
    json.dump(matrix, out, ensure_ascii=False, indent=1)
PYEOF

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
#    R14-F01: each case is also appended to leaf-cases.ndjson (host-cases)
#    so the leaf gate consumes the ACTUAL status, not the script's exit.
python3 - "$HOST" "$PORT" "$TOKEN" "$CASES_NDJSON" << 'PYEOF' | tee -a "$EVIDENCE_DIR/summary.txt"
import json, socket, sys
host, port, token, ndjson_path = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
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
with open(ndjson_path, "a") as nd:
    for name, request, expected, needle in cases:
        text = raw(request)
        status = int(text.split()[1]) if len(text.split()) > 1 else 0
        ok = status == expected and (needle is None or needle in text)
        nd.write(json.dumps({"case": name, "expect": expected,
                             "actual": status, "ok": ok}) + "\n")
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

# R14-F01: assemble leaf-cases.json — the per-leaf machine contract the
# stage gate consumes. Runs AFTER every expect_code case (including the CLI
# shape and startup negatives below would-be positions). Merges every
# recorded case (expect_code assertions, host-tampering raw cases,
# sessions-list leaf cases) with the WS-matrix probe results (booleans
# normalized to 1/0). The file is written even when cases failed
# (diagnosability), and the assembler exits non-zero so the command itself
# fails — a leaf can never ride a green exit on red cases.
python3 - "$CASES_NDJSON" "$EVIDENCE_DIR/ws-matrix.json" "$EVIDENCE_DIR/leaf-cases.json" << 'PYEOF'
import json, sys
ndjson_path, ws_path, out_path = sys.argv[1:4]
cases = [json.loads(line) for line in open(ndjson_path) if line.strip()]
def norm(value):
    if isinstance(value, bool):
        return int(value)
    return int(value) if isinstance(value, int) else 0
for r in json.load(open(ws_path))["results"]:
    cases.append({"case": r["case"], "expect": norm(r["expected"]),
                  "actual": norm(r["actual"]), "ok": bool(r["ok"])})
seen = set()
deduped = []
for c in cases:
    if c["case"] not in seen:
        seen.add(c["case"])
        deduped.append(c)
bad = [c for c in deduped if not c["ok"]]
doc = {"schema": "lingxi.leaf-case-results.v1", "cases": deduped}
with open(out_path, "w") as fh:
    json.dump(doc, fh, ensure_ascii=False, indent=1)
print(f"leaf-cases.json: {len(deduped)} cases, {len(bad)} failing")
sys.exit(1 if bad else 0)
PYEOF
note "PASS leaf-cases-json-assembled"

# ---- shutdown hygiene -------------------------------------------------------
# 认证检查全绿也不能掩盖关停失败；归属、期限和退出码同样是本轮结果。
[ "$(child_state "$SERVICE_PID")" = "owned" ] || fail "service is not an owned live child before shutdown"
STOP_STATE="$(bounded_stop_owned "$SERVICE_PID")"
case "$STOP_STATE" in
  exited) ;;
  *) fail "service did not stop within the TERM/KILL budget (state=$STOP_STATE)" ;;
esac
if wait "$SERVICE_PID" 2>/dev/null; then STOP_RC=0; else STOP_RC=$?; fi
SERVICE_PID=""
[ "$STOP_RC" -eq 0 ] || fail "service shutdown was not clean (exit=$STOP_RC)"
MARKERS="$(grep -c "LINGXI_AUTH_REJECTED\|LINGXI_TRANSPORT_REJECTED" "$EVIDENCE_DIR/server-stderr.log" || true)"
note "negative protocol log markers on stderr: ${MARKERS} lines"
note "== ALL CASES PASSED =="
SCRIPT_COMPLETED=1
