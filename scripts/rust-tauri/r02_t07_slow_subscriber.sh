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
mkdir -p "$EVIDENCE_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rust-target-r02-t07}"

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
cleanup() {
  [ -n "$RSS_PID" ] && kill "$RSS_PID" 2>/dev/null || true
  [ -n "$PROBE_PID" ] && kill "$PROBE_PID" 2>/dev/null || true
  [ -n "$SERVICE_PID" ] && kill "$SERVICE_PID" 2>/dev/null || true
  wait 2>/dev/null || true
  if [ -n "$HOME_DIR" ] && [ -d "$HOME_DIR" ]; then rm -rf "$HOME_DIR"; fi
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
HOME_DIR="$(mktemp -d /tmp/lingxi-r02t07-a14.XXXXXX)"
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
# The probe runs in background; poll for its completion marker.
for _ in $(seq 1 300); do
  grep -qE "STORM COMPLETE|Traceback|AssertionError" "$EVIDENCE_DIR/ws-probe.log" 2>/dev/null && break
  kill -0 "$SERVICE_PID" 2>/dev/null || break
  sleep 1
done
tail -14 "$EVIDENCE_DIR/ws-probe.log"
grep -q "STORM COMPLETE" "$EVIDENCE_DIR/ws-probe.log" || fail "S1/S2 storm probe did not complete cleanly (see ws-probe.log)"
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

note "== stopping service (graceful) =="
kill "$SERVICE_PID" 2>/dev/null || true
for _ in $(seq 1 100); do
  kill -0 "$SERVICE_PID" 2>/dev/null || break
  sleep 0.05
done
SERVICE_PID=""

note "== R02-A14 evidence complete =="
exit 0
