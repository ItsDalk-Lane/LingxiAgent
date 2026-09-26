#!/usr/bin/env python3
"""R02-T08 / R02-A15 full-chain probe (phase-driven).

Drives the REAL lingxi-service binary over real HTTP and real WebSocket:

  phase "boot":     health 200 (minimal surface), unauthenticated request 401,
                    loopback local-token from the owner-only token file
                    authenticates (/me 200, server-computed principal);
  phase "write":    POST execute commits a run + key events (write data);
  phase "subscribe": real WS upgrade + hello + subscribe_events; the client
                    receives the explicit subscription boundary
                    (subscribed.snapshotSeq) and then LIVE event frames for a
                    second execute committed while the subscription is open;
  phase "readback": after a real process stop + restart on the SAME home, the
                    pre-restart writes are still there (GET session, events
                    page contiguous), auth is re-established with the NEW
                    per-start token, and health is 200 again.

All expectations are asserted here (a failed assertion exits non-zero, so
the orchestrating bash script fails loudly). Prints one "PASS <label>" line
per assertion so the orchestrator's summary is reconstructable.
"""

import json
import os
import socket
import struct
import sys
import urllib.error
import urllib.request

HOST = "127.0.0.1"
SESSION = "sess_local_alpha"


def fail(msg):
    print(f"FAIL {msg}", file=sys.stderr)
    sys.exit(1)


def ok(label, detail=""):
    print(f"PASS {label}" + (f" ({detail})" if detail else ""))


def token_of(home):
    path = os.path.join(home, "lingxi-service", "local-token.json")
    with open(path, "r", encoding="utf-8") as fh:
        return json.load(fh)["token"]


def http(port, method, path, bearer=None, body=None):
    url = f"http://{HOST}:{port}{path}"
    data = body.encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    if bearer:
        req.add_header("Authorization", f"Bearer {bearer}")
    if data is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.status, resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as err:
        return err.code, err.read().decode("utf-8", "replace")


# ---- minimal RFC6455 client (client frames MUST be masked) ------------------

def ws_key():
    return os.urandom(16).hex()


def upgrade(port, bearer):
    request = (
        f"GET /lingxi/v1/ws HTTP/1.1\r\nHost: {HOST}:{port}\r\n"
        f"Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {ws_key()}\r\nSec-WebSocket-Version: 13\r\n"
        f"Authorization: Bearer {bearer}\r\n\r\n"
    ).encode()
    sock = socket.create_connection((HOST, port), timeout=15)
    sock.sendall(request)
    head = b""
    while b"\r\n\r\n" not in head:
        chunk = sock.recv(4096)
        if not chunk:
            fail("WS upgrade: connection closed during handshake")
        head += chunk
    status = int(head.split(b" ")[1])
    assert status == 101, f"upgrade failed: {status} {head[:200]!r}"
    return sock


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
            raise EOFError("connection closed mid-frame")
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
        return ("close", code, payload[2:].decode("utf-8", "replace"))
    return ("text", 0, payload.decode("utf-8", "replace"))


def ws_connect(port, bearer):
    sock = upgrade(port, bearer)
    hello = json.dumps({
        "protocol": "lingxi.wire", "clientKind": "probe", "clientVersion": "0",
        "protocolMin": 1, "protocolMax": 1,
    })
    send_frame(sock, hello.encode())
    kind, _, text = recv_frame(sock)
    assert kind == "text" and '"lingxi.wire"' in text, f"bad hello: {text[:120]}"
    return sock


def read_frames(sock, predicate, timeout=15.0):
    """Reads text frames until predicate(value) holds; returns collected."""
    sock.settimeout(timeout)
    seen = []
    while True:
        kind, _, text = recv_frame(sock)
        assert kind == "text", f"unexpected frame kind {kind}: {text[:120]}"
        value = json.loads(text)
        seen.append(value)
        if predicate(value):
            return seen


def main():
    mode = sys.argv[1]          # "boot-and-write" or "readback"
    port = int(sys.argv[2])
    home = sys.argv[3]

    if mode == "boot-and-write":
        # ---- health: 200, minimal surface -------------------------------
        status, body = http(port, "GET", "/lingxi/v1/health")
        assert status == 200, f"health: {status} {body[:200]}"
        health = json.loads(body)
        assert health["status"] == "ok", health
        assert health["serverKind"] == "lingxi-service", health
        assert health["wireProtocolMin"] == 1 and health["wireProtocolMax"] == 1, health
        ok("health-200-minimal", body[:120])

        # ---- auth negative: no credential -> 401 -------------------------
        status, body = http(port, "GET", "/lingxi/v1/me")
        assert status == 401, f"unauthenticated /me: {status} {body[:200]}"
        ok("unauthenticated-me-401")

        # ---- auth positive: per-start loopback token ---------------------
        token = token_of(home)
        status, body = http(port, "GET", "/lingxi/v1/me", bearer=token)
        assert status == 200, f"bearer /me: {status} {body[:200]}"
        me = json.loads(body)
        assert me.get("userId") == "user_local", f"unexpected principal: {body[:240]}"
        ok("loopback-token-me-200", f"principalId={me.get('principalId', '?')[:48]}")

        # ---- write data: execute #1 (pre-subscription) --------------------
        status, body = http(port, "POST", f"/lingxi/v1/sessions/{SESSION}/execute",
                            bearer=token, body=json.dumps({"input": "a15-write-pre-subscribe"}))
        assert status == 200, f"execute #1: {status} {body[:300]}"
        run1 = json.loads(body)
        assert run1.get("runId"), f"no runId: {body[:300]}"
        ok("execute-write-1-committed", f"runId={run1['runId']}")

        # ---- subscribe: snapshot boundary then live events ----------------
        sock = ws_connect(port, token)
        send_frame(sock, json.dumps({"type": "subscribe_events", "streamId": SESSION}).encode())
        controls = read_frames(sock, lambda v: v.get("type") == "subscribed")
        subscribed = controls[-1]
        snapshot_seq = int(subscribed["snapshotSeq"])
        assert snapshot_seq >= 0, f"bad snapshotSeq: {subscribed}"
        ok("ws-subscribed-with-snapshot-boundary", f"snapshotSeq={snapshot_seq}")

        # execute #2 while subscribed -> must arrive LIVE with seq > snapshot
        status, body = http(port, "POST", f"/lingxi/v1/sessions/{SESSION}/execute",
                            bearer=token, body=json.dumps({"input": "a15-write-while-subscribed"}))
        assert status == 200, f"execute #2: {status} {body[:300]}"
        events = read_frames(sock, lambda v: v.get("frameKind") != "control"
                             and int(v.get("seq", -1)) > snapshot_seq)
        live = events[-1]
        assert live.get("eventId") and "seq" in live, f"bad live event: {live}"
        ok("ws-live-event-after-write", f"seq={live['seq']} eventId={live['eventId'][:18]}…")

        # durable head as seen over HTTP (read-your-writes)
        status, body = http(port, "GET", f"/lingxi/v1/sessions/{SESSION}/events?limit=500",
                            bearer=token)
        assert status == 200, f"events page: {status} {body[:300]}"
        page = json.loads(body)
        seqs = [int(e["seq"]) for e in page["items"]]
        assert seqs == sorted(seqs) and len(seqs) == len(set(seqs)), "page seqs not strictly increasing"
        assert seqs and seqs[-1] >= snapshot_seq, "HTTP head behind WS snapshot boundary"
        ok("http-events-page-contiguous", f"head={seqs[-1]} count={len(seqs)}")

        # ---- a forged future cursor is rejected EXPLICITLY ----------------
        # (second WS connection; the server never invents events for a
        # cursor beyond the durable head: WS semantics are an explicit
        # invalid_message error frame + close, code future_cursor)
        sock2 = ws_connect(port, token)
        send_frame(sock2, json.dumps({"type": "subscribe_events", "streamId": SESSION,
                                      "cursor": _future_cursor(SESSION, seqs[-1] + 1000)}).encode())
        transcript = []
        while True:
            kind, code, text = recv_frame(sock2)
            transcript.append((kind, code, text))
            if kind == "close" or "future_cursor" in text or "invalid_message" in text:
                break
        joined = " ".join(text for _, _, text in transcript)
        assert "future_cursor" in joined, f"future-cursor reject not explicit: {transcript}"
        ok("ws-future-cursor-explicit-reject", "invalid_message/future_cursor close")
        sock2.close()

        # ---- durable head for the restart check ---------------------------
        with open(os.environ["A15_HEAD_FILE"], "w", encoding="utf-8") as fh:
            json.dump({"head_seq": seqs[-1], "runs": 2, "token_before_restart": token}, fh)
        ok("phase-boot-write-subscribe-complete", f"head={seqs[-1]}")

    elif mode == "readback":
        # The restart minted a NEW per-start token: the OLD one must no
        # longer authenticate, and the new one must.
        with open(os.environ["A15_HEAD_FILE"], "r", encoding="utf-8") as fh:
            old_token = json.load(fh)["token_before_restart"]
        status, body = http(port, "GET", "/lingxi/v1/me", bearer=old_token)
        assert status == 401, f"pre-restart token still valid: {status} {body[:200]}"
        ok("pre-restart-token-rejected-401")
        new_token = token_of(home)
        status, body = http(port, "GET", "/lingxi/v1/me", bearer=new_token)
        assert status == 200, f"post-restart /me: {status} {body[:200]}"
        ok("post-restart-new-token-me-200")

        # ---- data written before the restart is still there ---------------
        status, body = http(port, "GET", f"/lingxi/v1/sessions/{SESSION}", bearer=new_token)
        assert status == 200, f"post-restart session: {status} {body[:300]}"
        session = json.loads(body)
        assert int(session.get("runCount", 0)) >= 2, f"runs lost after restart: {body[:300]}"
        ok("post-restart-session-readback", f"runCount={session.get('runCount')}")

        status, body = http(port, "GET", f"/lingxi/v1/sessions/{SESSION}/events?limit=500",
                            bearer=new_token)
        assert status == 200, f"post-restart events: {status} {body[:300]}"
        page = json.loads(body)
        seqs = [int(e["seq"]) for e in page["items"]]
        assert seqs == sorted(seqs) and len(seqs) == len(set(seqs)), "post-restart seqs broken"
        with open(os.environ["A15_HEAD_FILE"], "r", encoding="utf-8") as fh:
            head_before = json.load(fh)["head_seq"]
        assert seqs and seqs[-1] >= head_before, (
            f"post-restart head {seqs[-1]} behind pre-restart head {head_before}")
        ok("post-restart-events-preserved", f"head_before={head_before} head_after={seqs[-1]}")

        status, body = http(port, "GET", "/lingxi/v1/health")
        assert status == 200, f"post-restart health: {status}"
        ok("post-restart-health-200")
    else:
        fail(f"unknown mode {mode}")


def _future_cursor(stream, seq):
    """A cursor with a VALID checksum but a seq beyond the durable head —
    the server must answer with the explicit snapshot_required(reason=
    future_cursor) directive instead of inventing events (same forgery
    shape as the T05 probe's forge_cursor)."""
    import base64
    import hashlib
    checksum = hashlib.sha256(
        ("lingxi-events-cursor-v1|" + stream + "|" + str(seq)).encode()
    ).hexdigest()
    body = json.dumps({"chk": checksum, "q": seq, "s": stream}, sort_keys=True,
                      separators=(",", ":"))
    return base64.urlsafe_b64encode(body.encode()).decode().rstrip("=")


if __name__ == "__main__":
    try:
        main()
    except AssertionError as exc:
        fail(str(exc))
    except (OSError, ValueError) as exc:
        fail(f"{type(exc).__name__}: {exc}")
