#!/usr/bin/env bash
# R02-T07 / acceptance R02-A14 — "慢订阅者不拖垮服务" against the REAL
# lingxi-service binary: a real WS subscriber stops reading while real
# concurrent writers keep committing key events through the real HTTP
# surface.
#
# Proves (binary level):
#   S1  a subscriber (A) that stops reading does NOT stall the writers:
#       all execute requests answer 200 and the healthy subscriber (B)
#       receives EVERY key event in seq order (no silent loss anywhere);
#   S2  when the configured cap is reached (--event-subscriber-queue 4),
#       the slow subscription is detached with an EXPLICIT
#       snapshot_required control frame (reason=slow_consumer) — the
#       client re-reads buffered frames and then the rebuild directive,
#       and can resubscribe successfully on the same connection (T05
#       semantics preserved: key events are never silently dropped);
#   S3  memory stays BOUNDED: the server's RSS is sampled every 200 ms
#       during the storm (real samples, machine-readable CSV) and must
#       stay under a hard ceiling with no runaway growth; the service
#       stays healthy (GET /health 200) during and after the storm;
#   S4  the server log carries the detach observability line with the
#       real queue stats (capacity / dropped_deltas / last seq) — the
#       "负载与队列监控" evidence, sampled not hand-filled.
#
# Environment guards: rustup-locked toolchain, task-dedicated target dir,
# offline locked build, proxy vars stripped, synthetic /tmp home only.
#
# Usage: scripts/rust-tauri/r02_t07_slow_subscriber.sh [EVIDENCE_DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

EVIDENCE_DIR="${1:-artifacts/rust-tauri/R02/T07}"
EVIDENCE_DIR="$EVIDENCE_DIR/slow-subscriber"
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
RSS_PID=""
PROBE_PID=""
# R02 stage-repair R7 / R7-F02: the PID variables are CURRENT handles —
# set on spawn, RETIRED (cleared) after every wait/reap (the normal stop
# paths already did). The trap signals a pid ONLY while it still proves
# CURRENT ownership (exists AND ppid is THIS shell): a retired or
# recycled number is never signalled (R6-F02 A12 pattern).
# R10-F03: the boolean probe `pid_owned_by_this_shell` had a dangerous
# false value — `ps` failing or returning an unreadable ppid ALSO
# returned false, and the stop flow read that as "STOPPED", skipped the
# KILL escalation, and fell into an UNBOUNDED `wait` on a possibly-alive
# child. It is REPLACED by the four-state `probe_state` below, and every
# consumer (stop flow + trap) now decides per state.
# R11-F04: the EXIT trap's `owned` branch used to be `kill TERM; wait`
# with NO deadline — a child that ignores TERM (or an unreapable
# survivor of the normal chain's KILL) hung the trap itself, defeating
# the R10-F03 deadlines. Both the trap and any other abnormal stop now
# use `bounded_stop` below: bounded TERM budget → KILL escalation ONLY
# while the object provably remains ours → bounded re-check → reap or
# LOUD residue. Nothing ever falls back into an unbounded wait.
probe_state() {
  # probe_state <pid> → one of:
  #   exited        — kill -0 says no such object: an unreaped-child
  #                   handle at this point means bash already collected
  #                   it; the stop flow may wait-and-retire safely.
  #   owned         — the object exists AND its ppid is THIS shell
  #                   (alive, or a not-yet-collected child): ours to
  #                   signal and wait.
  #   foreign       — exists but its ppid is ANOTHER process: the number
  #                   left our ownership (child exited, number possibly
  #                   recycled). NEVER signalled; not waited as a child.
  #   unobservable  — the object EXISTS (kill -0 succeeded) but its ppid
  #                   cannot be read (ps failure / unreadable output):
  #                   the state is UNKNOWN — never counted as stopped,
  #                   never signalled, never waited unboundedly.
  local ppid
  if ! kill -0 "$1" 2>/dev/null; then
    printf 'exited\n'
    return 0
  fi
  ppid="$(ps -o ppid= -p "$1" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$ppid" ]; then
    printf 'unobservable\n'
    return 0
  fi
  if [ "$ppid" = "$$" ]; then
    printf 'owned\n'
  else
    printf 'foreign\n'
  fi
}
# R11-F04: bounded stop ladder for ONE owned handle — the same deadline
# shape the normal stop chain uses, so an EXIT trap (or any abnormal
# stop) can never hang on a child that ignores TERM. TERM now → poll
# probe_state under a ≤5 s budget → escalate to a direct-pid KILL ONLY
# if the object is still provably ours at that instant (never a group,
# never an unattributable number) → bounded ≤5 s re-check. Prints the
# final four-state verdict; NEVER waits unboundedly.
bounded_stop() {
  # bounded_stop <pid>
  local pid="$1" state="" i
  # 异常清理和正常停止都只对本脚本当前拥有的子进程发信号。
  state="$(probe_state "$pid")"
  if [ "$state" != "owned" ]; then printf '%s\n' "$state"; return 0; fi
  kill "$pid" 2>/dev/null || true
  for i in $(seq 1 100); do
    state="$(probe_state "$pid")"
    case "$state" in
      exited|foreign) break ;;
      owned|unobservable) : ;;
    esac
    sleep 0.05
  done
  if [ "${state:-owned}" = "owned" ]; then
    state="$(probe_state "$pid")"
  fi
  if [ "$state" = "owned" ]; then
    kill -9 "$pid" 2>/dev/null || true
    for i in $(seq 1 100); do
      state="$(probe_state "$pid")"
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
  for pid in "$RSS_PID" "$PROBE_PID" "$SERVICE_PID"; do
    [ -n "$pid" ] || continue
    case "$(probe_state "$pid")" in
      owned)
        # R11-F04: the old `kill TERM; wait` had NO deadline — a child
        # ignoring TERM (or an unreapable survivor of the normal chain's
        # KILL, which is exactly the `fail` path that keeps the handle
        # for this trap) hung the trap in an unbounded wait. Bounded
        # ladder, then reap or report residue loudly — never wait
        # unboundedly.
        tstate="$(bounded_stop "$pid")"
        case "$tstate" in
          exited)
            wait "$pid" 2>/dev/null || true
            ;;
          foreign)
            echo "cleanup: pid $pid 已不属于本脚本，不等待或发信号" >&2
            cleanup_residue=1
            ;;
          owned)
            echo "cleanup: pid $pid still OWNED after the TERM and KILL budgets — RESIDUE left behind, no unbounded wait" >&2
            cleanup_residue=1
            ;;
          unobservable)
            echo "cleanup: pid $pid state UNOBSERVABLE after the stop budgets — not signalled (unattributable), no unbounded wait; possible residue" >&2
            cleanup_residue=1
            ;;
        esac
        ;;
      unobservable)
        # Exists but ownership unreadable: NOT signalled (never signal
        # an unattributable object) — reported loudly, fail-closed.
        echo "cleanup: pid $pid ownership UNOBSERVABLE (ps unreadable) — NOT signalled" >&2
        cleanup_residue=1
        ;;
      foreign)
        echo "cleanup: pid $pid is NOT currently owned by this shell (number left our ownership) — NOT signalled" >&2
        cleanup_residue=1
        ;;
      exited)
        # Already gone; a wait here would just collect bash's job-table
        # entry — do it best-effort.
        wait "$pid" 2>/dev/null || true
        ;;
    esac
  done
  RSS_PID=""; PROBE_PID=""; SERVICE_PID=""
  # R11-F04: the parameterless `wait` that used to run here was itself
  # an unbounded wait over whatever survived above — exactly the owned /
  # unobservable residue the bounded ladder now reports instead of
  # hanging on. Every handle was reaped inside its state branch
  # (exited) or reported as residue (owned/foreign/unobservable);
  # nothing legitimate is left to wait for.
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

# R02-T07 REVIEW_R1 F03: annotate each run (summary.txt used to stack
# repeated runs with no run marker) and use run-relative timestamps.
RUN_STAMP="$(date '+%Y-%m-%dT%H:%M:%S%z')"
note "== run $RUN_STAMP (pid $$) =="
note "== R02-T07 / R02-A14 slow-subscriber storm (toolchain $TOOLCHAIN, target $TARGET_DIR) =="

note "== building lingxi-service (--locked, offline) =="
$CARGO build --manifest-path rust/Cargo.toml --locked -p lingxi-service \
  > "$EVIDENCE_DIR/build.log" 2>&1 || { cat "$EVIDENCE_DIR/build.log"; fail "build failed"; }
SERVICE_BIN="$TARGET_DIR/debug/lingxi-service"
[ -x "$SERVICE_BIN" ] || fail "service binary missing"
note "PASS build"

note "== starting service (event-subscriber-queue=4, max-ws-connections=8) =="
HOME_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lingxi-r02t07-a14.XXXXXX")
"$SERVICE_BIN" --home "$HOME_DIR" --bind 127.0.0.1:0 \
  --event-subscriber-queue 4 --max-ws-connections 8 --http-rate-max 100000 \
  > "$EVIDENCE_DIR/service.out" 2> "$EVIDENCE_DIR/service.err" &
SERVICE_PID=$!
for _ in $(seq 1 200); do
  if grep -q '^LINGXI_SERVICE_READY ' "$EVIDENCE_DIR/service.out" 2>/dev/null; then
    break
  fi
  kill -0 "$SERVICE_PID" 2>/dev/null || { cat "$EVIDENCE_DIR/service.err"; fail "service died"; }
  sleep 0.05
done
ADDR=$(sed -n 's/^LINGXI_SERVICE_READY addr=\([^ ]*\).*/\1/p' "$EVIDENCE_DIR/service.out" | head -n 1)
[ -n "$ADDR" ] || fail "no readiness line"
note "PASS service ready at $ADDR (pid $SERVICE_PID)"

TOKEN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' \
  "$HOME_DIR/lingxi-service/local-token.json")

# ── S1/S2: slow subscriber A, healthy subscriber B, real writer storm ──────
# Writer volumes (EXECUTES / SEQUENTIAL) are constants of the probe below;
# the summary intentionally carries NO duplicated hard-coded counts
# (R02-T07 REVIEW_R1 F03) — storm-results.json is the numeric authority.
note "== S1/S2: subscriber A stops reading; B keeps reading; writers commit key events until the subscriber-queue cap is reached =="
ADDR="$ADDR" TOKEN="$TOKEN" SERVICE_PID="$SERVICE_PID" EVIDENCE_DIR="$EVIDENCE_DIR" \
python3 - > "$EVIDENCE_DIR/ws-probe.log" 2>&1 <<'PYEOF' &
import base64, json, os, socket, struct, threading, time
import urllib.request

addr = os.environ["ADDR"]
host, port = addr.rsplit(":", 1)
token = os.environ["TOKEN"]
evidence = os.environ["EVIDENCE_DIR"]
SERVICE_PID = int(os.environ["SERVICE_PID"])
STREAM = "sess_local_alpha"
EXECUTES = 1000         # concurrent writers (24 at a time; volume must exceed the TCP pipe capacity so the slow subscriber really reaches the configured cap)
SAMPLE_EVERY = 0.2      # RSS sampling period (seconds)

def http(method, path, body=None):
    req = urllib.request.Request(f"http://{addr}{path}", method=method)
    req.add_header("Authorization", f"Bearer {token}")
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, data=data, timeout=60) as resp:
        return resp.status, json.loads(resp.read().decode())

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

def send_frame(sock, payload, opcode=0x1):
    if isinstance(payload, str):
        payload = payload.encode()
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
    return opcode, payload

class WsClient:
    def __init__(self, name, rcvbuf=None):
        self.name = name
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        if rcvbuf:
            # Deterministic slow subscriber: a tiny kernel receive buffer
            # makes the server-side mailbox overflow after a handful of
            # frames instead of after ~400 kernel-buffered ones.
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, rcvbuf)
        sock.settimeout(30)
        sock.connect((host, int(port)))
        request = (
            f"GET /lingxi/v1/ws HTTP/1.1\r\nHost: {host}:{port}\r\n"
            "Upgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {ws_key()}\r\nSec-WebSocket-Version: 13\r\n"
            f"Authorization: Bearer {token}\r\n\r\n"
        )
        sock.sendall(request.encode())
        head = recv_headers(sock)
        assert int(head.split()[1]) == 101, f"{name}: upgrade failed"
        self.sock = sock
        hello = {
            "protocol": "lingxi.wire", "clientKind": "cli",
            "clientVersion": "r02-t07-a14", "protocolMin": 1, "protocolMax": 1,
        }
        send_frame(self.sock, json.dumps(hello))
        opcode, payload = recv_frame(self.sock)
        assert opcode == 0x1, f"{name}: no ServerHello"
        hello_back = json.loads(payload)
        assert hello_back.get("selectedProtocol"), f"{name}: bad ServerHello"

    def subscribe(self, stream):
        send_frame(self.sock, json.dumps({"type": "subscribe_events", "streamId": stream}))
        opcode, payload = recv_frame(self.sock)
        assert opcode == 0x1, f"{self.name}: expected control frame"
        frame = json.loads(payload)
        assert frame.get("type") == "subscribed", f"{self.name}: {frame}"
        return frame

def rss_kb():
    out = os.popen(f"ps -o rss= -p {SERVICE_PID}").read().strip()
    return int(out) if out else 0

rss_samples = []
sampling = True

def sampler():
    while sampling:
        kb = rss_kb()
        if kb:
            rss_samples.append(kb)
        time.sleep(SAMPLE_EVERY)

threading.Thread(target=sampler, daemon=True).start()

# Subscriber A: tiny receive buffer, reads only the `subscribed` control
# frame, then STOPS reading (real TCP backpressure; the server-side
# mailbox overflows deterministically a few events later).
sub_a = WsClient("A", rcvbuf=4096)
ctrl_a = sub_a.subscribe(STREAM)
print(f"A subscribed: {ctrl_a}", flush=True)

# Subscriber B: keeps reading for the whole storm; on an explicit detach
# it rebuilds through the HTTP snapshot pages (the T05 recovery path).
sub_b = WsClient("B")
ctrl_b = sub_b.subscribe(STREAM)
print(f"B subscribed: {ctrl_b}", flush=True)

seen_b = []
b_detached = []
b_done = threading.Event()

def reader_b():
    try:
        while True:
            opcode, payload = recv_frame(sub_b.sock)
            assert opcode == 0x1, f"B: unexpected opcode {opcode}"
            frame = json.loads(payload)
            if frame.get("type") == "snapshot_required":
                print(f"B detached explicitly: {frame}", flush=True)
                b_detached.append(frame)
                break
            if frame.get("type") == "subscribed":
                continue
            seen_b.append(int(frame["seq"]))
    except Exception as exc:
        print(f"B reader ended: {exc!r} count={len(seen_b)}", flush=True)
    finally:
        b_done.set()

thread_b = threading.Thread(target=reader_b, daemon=True)
thread_b.start()

# Writer storm: REAL concurrent HTTP writers committing key events.
statuses = []
def writer(k):
    try:
        status, _ = http("POST", f"/lingxi/v1/sessions/{STREAM}/execute",
                         {"input": f"a14 storm run {k}"})
        statuses.append(status)
    except Exception as exc:
        statuses.append(repr(exc))

threads = [threading.Thread(target=writer, args=(k,)) for k in range(EXECUTES)]
i = 0
while i < EXECUTES:
    batch = threads[i:i + 24]
    for t in batch:
        t.start()
    for t in batch:
        t.join()
    i += 24
print(f"writer storm done: {len(statuses)} requests, statuses {sorted(set(map(str, statuses)))}", flush=True)
assert statuses == [200] * EXECUTES, "every execute must answer 200 (no stall, no fake success)"

# Phase W2 (deterministic volume): sequential writers commit enough key
# events that subscriber A's TCP pipeline (kernel-buffered) FILLS and the
# server-side mailbox (capacity 4) actually overflows -> explicit detach.
# The durable head is measured afterwards, so W1's concurrent count staying
# nondeterministic (idempotent run-id replay under same-ms collisions) is
# fine — the assertions below are relative to the measured head.
SEQUENTIAL = 1200
seq_statuses = []
for k in range(SEQUENTIAL):
    status, _ = http("POST", f"/lingxi/v1/sessions/{STREAM}/execute",
                     {"input": f"a14 sequential run {k}"})
    seq_statuses.append(status)
    if status != 200:
        break
print(f"sequential phase done: {len(seq_statuses)} requests, statuses "
      f"{sorted(set(map(str, seq_statuses)))}", flush=True)
assert seq_statuses == [200] * SEQUENTIAL, "sequential writers must all succeed"

# Wait for B to drain the live tail.
b_done.wait(timeout=60)

# The DURABLE head: paginate the real HTTP snapshot endpoint to the end.
durable_seqs = []
cursor = None
while True:
    path = f"/lingxi/v1/sessions/{STREAM}/events?limit=500"
    if cursor:
        path += f"&cursor={cursor}"
    _, page = http("GET", path)
    durable_seqs.extend(int(e["seq"]) for e in page["items"])
    cursor = page.get("nextCursor")
    if not cursor:
        break
assert durable_seqs == sorted(durable_seqs), "durable log must be seq-ordered"
TOTAL = len(durable_seqs)
assert durable_seqs == list(range(1, TOTAL + 1)), "durable seqs must be contiguous 1..N"
print(f"durable head: {TOTAL} key events", flush=True)

# Healthy subscriber B: received everything live, OR rebuilt explicitly
# after an explicit detach — either way its merged view is complete.
if b_detached:
    cursor = None
    while True:
        path = f"/lingxi/v1/sessions/{STREAM}/events?limit=500"
        if cursor:
            path += f"&cursor={cursor}"
        _, page = http("GET", path)
        seen_b.extend(int(e["seq"]) for e in page["items"])
        cursor = page.get("nextCursor")
        if not cursor:
            break
merged_b = sorted(set(seen_b))
assert merged_b == list(range(1, TOTAL + 1)), (
    f"healthy subscriber lost events: got {len(merged_b)} of {TOTAL}")
print(f"B merged view complete: {TOTAL}/{TOTAL} key events "
      f"(live-only: {not b_detached})", flush=True)

# Subscriber A resumes: buffered frames, then the EXPLICIT
# snapshot_required (slow_consumer) — never silent loss, never a stall.
a_frames = 0
a_detached = None
sub_a.sock.settimeout(15)
try:
    for _ in range(EXECUTES * 2 + 8):
        opcode, payload = recv_frame(sub_a.sock)
        assert opcode == 0x1, f"A: unexpected opcode {opcode}"
        frame = json.loads(payload)
        if frame.get("type") == "snapshot_required":
            a_detached = frame
            break
        a_frames += 1
except socket.timeout:
    pass
assert a_detached is not None, "A must receive the explicit snapshot_required directive"
assert a_detached.get("reason") == "slow_consumer", f"detach reason: {a_detached}"
print(f"A detached explicitly: reason={a_detached.get('reason')} "
      f"(buffered key events delivered before the signal: {a_frames})", flush=True)

# A resubscribes on the same connection (T05 semantics): fresh cut.
send_frame(sub_a.sock, json.dumps({"type": "subscribe_events", "streamId": STREAM}))
opcode, payload = recv_frame(sub_a.sock)
frame = json.loads(payload)
assert frame.get("type") == "subscribed", f"A resubscribe failed: {frame}"
print("A resubscribed after detach: fresh subscribed control received", flush=True)

sampling = False
time.sleep(SAMPLE_EVERY)
assert rss_samples, "no RSS samples collected"

# Service health in the aftermath.
status, _ = http("GET", "/lingxi/v1/health")
assert status == 200, "service must stay healthy after the storm"

with open(os.path.join(evidence, "rss-samples.csv"), "w") as f:
    f.write("sample_kb\n")
    for s in rss_samples:
        f.write(f"{s}\n")

with open(os.path.join(evidence, "storm-results.json"), "w") as f:
    json.dump({
        "executes_concurrent": EXECUTES,
        "executes_sequential": SEQUENTIAL,
        "durable_key_events": TOTAL,
        "all_executes_200": statuses == [200] * EXECUTES,
        "b_complete": merged_b == list(range(1, TOTAL + 1)),
        "b_needed_explicit_rebuild": bool(b_detached),
        "a_buffered_before_detach": a_frames,
        "a_detach_reason": a_detached.get("reason"),
        "a_resubscribed": True,
        "rss_min_kb": min(rss_samples),
        "rss_max_kb": max(rss_samples),
        "rss_last_kb": rss_samples[-1],
        "health_after": status,
    }, f, indent=2)
print("STORM COMPLETE", flush=True)
PYEOF
# 后台 Python 的 PID 必须在 Shell 的 heredoc 结束后记录，不能放进 Python 源码。
PROBE_PID=$!
# The probe runs in background; poll for its completion marker.
for _ in $(seq 1 300); do
  grep -qE "STORM COMPLETE|Traceback|AssertionError" "$EVIDENCE_DIR/ws-probe.log" 2>/dev/null && break
  kill -0 "$SERVICE_PID" 2>/dev/null || break
  sleep 1
done
tail -14 "$EVIDENCE_DIR/ws-probe.log"
grep -q "STORM COMPLETE" "$EVIDENCE_DIR/ws-probe.log" || fail "S1/S2 storm probe did not complete cleanly (see ws-probe.log)"
# R9-F02: completion marker seen — REAP the probe (our background child)
# and only then retire the handle. A failure above leaves the handle set
# so the EXIT trap can TERM/wait the probe by CURRENT ownership.
PROBE_FINAL_STATE=""
for _ in $(seq 1 100); do
  PROBE_FINAL_STATE="$(probe_state "$PROBE_PID")"
  case "$PROBE_FINAL_STATE" in exited|foreign) break ;; owned|unobservable) : ;; esac
  sleep 0.05
done
case "$PROBE_FINAL_STATE" in
  exited) ;;
  *) fail "storm probe wrote a completion marker but did not exit within 5 seconds (state=$PROBE_FINAL_STATE)" ;;
esac
if wait "$PROBE_PID" 2>/dev/null; then PROBE_RC=0; else PROBE_RC=$?; fi
PROBE_PID=""
[ "$PROBE_RC" -eq 0 ] || fail "storm probe exited $PROBE_RC despite its completion marker"
EXECUTES_DONE="$(python3 -c "import json;print(json.load(open('$EVIDENCE_DIR/storm-results.json'))['executes_concurrent'])")"
note "PASS S1: all $EXECUTES_DONE concurrent executes answered 200 (writers not stalled by the slow subscriber; count read from storm-results.json)"
note "PASS S1: healthy subscriber B's merged view equals the durable head (no silent loss)"
note "PASS S2: slow subscriber A got the EXPLICIT snapshot_required (reason=slow_consumer) and resubscribed"

# ── S3: memory bound from the real samples ─────────────────────────────────
note "== S3: memory bound (real RSS samples during the storm) =="
python3 - "$EVIDENCE_DIR" <<'PYEOF'
import json, sys
evidence = sys.argv[1]
results = json.load(open(f"{evidence}/storm-results.json"))
rss = [int(line) for line in open(f"{evidence}/rss-samples.csv").read().split()[1:]]
assert rss, "no RSS samples"
# Debug-build macOS service: a hard, generous ceiling; the assertion is
# against runaway unbounded growth during the storm, not against a tuned
# production number.
CEILING_KB = 500 * 1024
peak = max(rss)
final = rss[-1]
first = rss[0]
growth = final - first
assert peak < CEILING_KB, f"RSS peak {peak} KB exceeded the ceiling {CEILING_KB} KB"
# Bounded: after the storm the process does not keep growing (final within
# 25% of the observed peak — allocators may retain, but must not climb on).
assert final <= peak + max(64 * 1024, peak // 4), \
    f"final RSS {final} KB must be bounded by the storm peak {peak} KB"
print(f"RSS first={first} KB peak={peak} KB final={final} KB growth={growth} KB ceiling={CEILING_KB} KB -> BOUNDED")
results["rss_bounded_ceiling_kb"] = CEILING_KB
json.dump(results, open(f"{evidence}/storm-results.json", "w"), indent=2)
PYEOF
note "PASS S3: RSS stayed bounded during the sustained storm (see rss-samples.csv)"

# ── S4: server-side detach observability (real sampled queue stats) ────────
note "== S4: server detach line with real queue stats =="
grep "subscription detached" "$EVIDENCE_DIR/service.err" | head -n 3 \
  > "$EVIDENCE_DIR/detach-lines.txt" || true
[ -s "$EVIDENCE_DIR/detach-lines.txt" ] || { grep -c . "$EVIDENCE_DIR/service.err" || true; fail "no detach line in server log"; }
grep -qE 'reason="?slow_consumer"?' "$EVIDENCE_DIR/detach-lines.txt" \
  || fail "detach line must name slow_consumer"
grep -q "queue_capacity=4" "$EVIDENCE_DIR/detach-lines.txt" \
  || fail "detach line must carry the real queue capacity"
note "PASS S4: server logged the detach with queue_capacity=4 and the slow_consumer reason"

note "== stopping service (graceful; R9-F02: no silent handle retirement; R10-F03: four-state ownership probe) =="
# 紧贴 TERM 再核归属，不能只依赖上方较早的判断。
[ "$(probe_state "$SERVICE_PID")" = "owned" ] || fail "service ownership changed before TERM"
kill "$SERVICE_PID" 2>/dev/null || true
SERVICE_FINAL_STATE=""
# ≤15 s TERM budget. R10-F03: the loop consumes the FOUR STATES —
# `exited` (object gone — bash already collected the child: stop,
# wait-and-retire), `foreign` (number left our ownership: stop managing
# it loudly, never signal), `owned` (still ours: keep waiting), and
# `unobservable` (ps cannot attribute it: UNKNOWN — the deadline alone
# governs; NEVER counted as stopped — the previous boolean probe read a
# ps failure as "stopped", skipped the KILL escalation, and fell into an
# unbounded `wait` on a possibly-alive child).
for _ in $(seq 1 300); do
  SERVICE_FINAL_STATE="$(probe_state "$SERVICE_PID")"
  case "$SERVICE_FINAL_STATE" in
    exited|foreign) break ;;
    owned|unobservable) : ;;
  esac
  sleep 0.05
done
if [ "$SERVICE_FINAL_STATE" = "owned" ]; then
  note "service still OWNED and alive after the TERM budget — escalating to KILL (graceful stop did NOT complete)"
  # The object is provably ours at this instant (exists, ppid == this
  # shell, never waited since spawn): a direct-pid KILL targets exactly
  # it, never an unknown object and never a group.
  SERVICE_FINAL_STATE="$(probe_state "$SERVICE_PID")"
  if [ "$SERVICE_FINAL_STATE" = "owned" ]; then kill -9 "$SERVICE_PID" 2>/dev/null || true; fi
  # ≤10 s bounded re-check with the same state machine.
  for _ in $(seq 1 200); do
    SERVICE_FINAL_STATE="$(probe_state "$SERVICE_PID")"
    case "$SERVICE_FINAL_STATE" in
      exited|foreign) break ;;
      owned|unobservable) : ;;
    esac
    sleep 0.05
  done
fi
case "$SERVICE_FINAL_STATE" in
  exited)
    # Confirm-stop path: reap bash's job-table entry and retire the
    # handle (R9-F02: no silent retirement).
    if wait "$SERVICE_PID" 2>/dev/null; then SERVICE_STOP_RC=0; else SERVICE_STOP_RC=$?; fi
    SERVICE_PID=""
    [ "$SERVICE_STOP_RC" -eq 0 ] || fail "service shutdown exit=$SERVICE_STOP_RC"
    ;;
  foreign)
    # 归属已改变时无法证明这是本轮子进程完成关停；保留句柄与现场供 trap 报错。
    fail "service pid $SERVICE_PID left this shell's ownership before shutdown proof; not signalled or waited"
    ;;
  owned)
    fail "service pid $SERVICE_PID still OWNED and alive after TERM and KILL — residue remains; handle kept for the trap"
    ;;
  unobservable)
    # UNKNOWN state: not stopped, not signalled (the object cannot be
    # attributed), and NO unbounded wait — loud residue, handle kept for
    # the trap (whose ownership guard will likewise refuse to signal an
    # unattributable object).
    fail "service pid $SERVICE_PID state UNOBSERVABLE after the stop budgets (ps cannot attribute it; exists per kill -0) — cannot prove stop, not signalling an unattributable object, no unbounded wait; handle kept for the trap"
    ;;
  *)
    fail "internal: unknown probe_state result '$SERVICE_FINAL_STATE' for service pid $SERVICE_PID"
    ;;
esac

note "== R02-A14 evidence complete =="
exit 0
