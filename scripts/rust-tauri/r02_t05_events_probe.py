#!/usr/bin/env python3
"""R02-T05 binary-level event subscription matrix probe (stdlib only).

Drives the REAL lingxi-service binary over real HTTP + WebSocket:

  A09 (snapshot + subscription without a gap):
    - keep executing runs over HTTP while a WS subscriber subscribes with
      no cursor (the race: snapshot cut vs concurrent commits);
    - merge the snapshot cut with the live tail; assert the boundary is
      explicitly reported (subscribed.snapshotSeq), seqs are contiguous,
      nothing is duplicated, and the merged view covers every committed
      event — cross-checked against the durable key_events dump captured
      through lingxi-storage-inspect by the calling script.

  A10 (expired cursor recovery):
    - take a small page to obtain a REAL cursor, run the retention purge
      (same predicate the storage port exposes: seq < floor) through a
      second SQLite connection, then resume over HTTP (expect 409
      cursor_expired + details.reason=snapshot_required, never an empty
      page) and over WS (expect the snapshot_required CONTROL frame while
      the connection stays usable);
    - rebuild from a fresh snapshot and diff against the post-purge
      authority.

  Negatives: unknown stream, forged-but-checksum-valid future cursor,
  malformed cursor, duplicate subscribe on one connection, cross-principal.

Usage: r02_t05_events_probe.py HOST PORT TOKEN DB_PATH AUTHORITY_JSON OUT_DIR
Prints one JSON line per case to stdout and writes evidence files into
OUT_DIR; exits non-zero on any unexpected outcome.
"""

import base64
import hashlib
import json
import os
import socket
import sqlite3
import struct
import subprocess
import sys
import threading

WS_GUID = "258EAFA5-E914-47DA-C5CA-C5AB0DC85B11"
STREAM = "sess_local_alpha"
CURSOR_TAG = "lingxi-events-cursor-v1"

results = []


def record(case, ok, detail=""):
    results.append({"case": case, "ok": bool(ok), "detail": str(detail)[:400]})
    print(json.dumps(results[-1], ensure_ascii=False), flush=True)
    return ok


# ── transport helpers (same shape as r02_t03_ws_probe.py) ──────────────────


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


def upgrade(host, port, bearer):
    sock = socket.create_connection((host, port), timeout=15)
    request = (
        f"GET /lingxi/v1/ws HTTP/1.1\r\nHost: {host}:{port}\r\n"
        "Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {ws_key()}\r\nSec-WebSocket-Version: 13\r\n"
        f"Authorization: Bearer {bearer}\r\n\r\n"
    )
    sock.sendall(request.encode())
    head = recv_headers(sock)
    status = int(head.split()[1]) if head.split() and len(head.split()) > 1 else 0
    assert status == 101, f"upgrade failed: {status} {head}"
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
    if opcode == 0x9:
        return ("ping", 0, "")
    if opcode == 0xA:
        return ("pong", 0, "")
    return ("text", 0, payload.decode("utf-8", "replace"))


def http(host, port, method, path, bearer, body=None):
    payload = (body or "").encode()
    headers = f"Host: {host}:{port}\r\nAuthorization: Bearer {bearer}\r\n"
    if body is not None:
        headers += "Content-Type: application/json\r\n"
    request = (
        f"{method} {path} HTTP/1.1\r\n{headers}"
        f"Content-Length: {len(payload)}\r\nConnection: close\r\n\r\n"
    ).encode() + payload
    sock = socket.create_connection((host, port), timeout=15)
    sock.sendall(request)
    buf = b""
    while True:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
    sock.close()
    text = buf.decode("utf-8", "replace")
    status = int(text.split()[1]) if text.split() and len(text.split()) > 1 else 0
    body_out = text.split("\r\n\r\n", 1)[1] if "\r\n\r\n" in text else ""
    return status, body_out


def ws_connect(host, port, bearer):
    sock = upgrade(host, port, bearer)
    hello = json.dumps({
        "protocol": "lingxi.wire", "clientKind": "probe", "clientVersion": "0",
        "protocolMin": 1, "protocolMax": 1,
    })
    send_frame(sock, hello.encode())
    kind, _, text = recv_frame(sock)
    assert kind == "text" and '"lingxi.wire"' in text, f"bad hello: {text[:120]}"
    return sock


def subscribe_frame(stream, cursor=None):
    value = {"type": "subscribe_events", "streamId": stream}
    if cursor is not None:
        value["cursor"] = cursor
    return json.dumps(value)


def read_until_events(sock, n, drain_timeout=5.0):
    """Reads frames until n bare envelopes arrived; control frames kept."""
    sock.settimeout(drain_timeout)
    controls, events = [], []
    try:
        while len(events) < n:
            kind, _, text = recv_frame(sock)
            assert kind == "text", f"unexpected frame kind {kind}"
            value = json.loads(text)
            if value.get("frameKind") == "control":
                controls.append(value)
            else:
                assert "eventId" in value and "seq" in value, text[:200]
                events.append(value)
    except (socket.timeout, TimeoutError, EOFError, ConnectionResetError,
            BrokenPipeError):
        pass
    return controls, events


def drain(sock, timeout=1.0):
    sock.settimeout(timeout)
    frames = []
    try:
        while True:
            kind, code, text = recv_frame(sock)
            frames.append((kind, code, text))
    except (socket.timeout, TimeoutError, EOFError, ConnectionResetError,
            BrokenPipeError):
        pass
    return frames


def execute(host, port, bearer, text, session=STREAM):
    status, body = http(
        host, port, "POST",
        f"/lingxi/v1/sessions/{session}/execute", bearer,
        json.dumps({"input": text}),
    )
    assert status == 200, f"execute failed ({status}): {body}"
    return body


def forge_cursor(stream, seq):
    """Cursor with a VALID checksum but a seq beyond the durable head —
    proves the server bounds cursors against the durable log, not merely
    against the checksum."""
    checksum = hashlib.sha256(
        (CURSOR_TAG + "|" + stream + "|" + str(seq)).encode()
    ).hexdigest()
    body = json.dumps({"chk": checksum, "q": seq, "s": stream}, sort_keys=True,
                      separators=(",", ":"))
    return base64.urlsafe_b64encode(body.encode()).decode().rstrip("=")


def durable_log(db_path):
    """The committed key_events rows of the stream, straight from the
    database file (read-only; WAL readers do not disturb the writer)."""
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True, timeout=10)
    try:
        rows = conn.execute(
            "SELECT seq, event_id FROM key_events WHERE stream_id = ? "
            "ORDER BY seq ASC",
            (STREAM,),
        ).fetchall()
    finally:
        conn.close()
    return [(int(seq), event_id) for seq, event_id in rows]


def main():
    host, port, bearer, db_path = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
    out_dir = sys.argv[5]
    os.makedirs(out_dir, exist_ok=True)
    ok = True

    # ── A09: concurrent writers vs snapshot+live subscription ────────────
    # Note on writer concurrency: the session run id is derived server-side
    # from (wall-clock ms, total_runs+1) and the counter read is not atomic
    # across concurrent executes, so two executes in the same millisecond
    # can collide on one run id (the T04 storage layer absorbs the second
    # as an idempotent replay — one durable run, both callers get 200).
    # That derivation belongs to R03's concurrency scope; this binary probe
    # therefore serializes its writes (the race under proof is WRITES vs
    # SUBSCRIBE, which stays fully live) and the genuinely-concurrent
    # two-writer matrix runs in the cargo test with distinct timestamps.
    PRESEEED_RUNS = 2
    RACED_RUNS = 4
    for r in range(PRESEEED_RUNS):
        execute(host, port, bearer, f"a09-pre-r{r}")

    writer_errors = []

    def writer():
        try:
            for r in range(RACED_RUNS):
                execute(host, port, bearer, f"a09-raced-r{r}")
        except Exception as exc:  # noqa: BLE001 — recorded, never swallowed
            writer_errors.append(str(exc))

    sock = ws_connect(host, port, bearer)
    thread = threading.Thread(target=writer)
    thread.start()
    # The race point: subscribe while the writer thread keeps committing.
    send_frame(sock, subscribe_frame(STREAM).encode())
    thread.join(30)
    ok &= record("a09-writers-clean", not writer_errors, writer_errors)

    total_events = (PRESEEED_RUNS + RACED_RUNS) * 2
    controls, events = read_until_events(sock, total_events, drain_timeout=5.0)
    extra = drain(sock, timeout=1.0)
    for kind, code, text in extra:
        if kind == "text":
            value = json.loads(text)
            if value.get("frameKind") == "control":
                controls.append(value)
            else:
                events.append(value)
    sock.close()

    subscribed = [c for c in controls if c.get("type") == "subscribed"]
    ok &= record("a09-one-subscribed-control", len(subscribed) == 1, controls)
    snapshot_seq = int(subscribed[0]["snapshotSeq"]) if subscribed else -1
    snapshot_n = len([e for e in events if int(e["seq"]) <= snapshot_seq])
    ok &= record(
        "a09-explicit-boundary",
        snapshot_n >= PRESEEED_RUNS * 2
        and snapshot_seq == int(
            [e for e in events if int(e["seq"]) <= snapshot_seq][-1]["seq"]
        ),
        f"snapshotSeq={snapshot_seq} snapshot-events={snapshot_n}",
    )
    seqs = [int(e["seq"]) for e in events]
    contiguous = all(b == a + 1 for a, b in zip(seqs, seqs[1:]))
    ok &= record(
        "a09-no-gap-no-dup",
        len(seqs) == total_events
        and seqs == sorted(set(seqs))
        and contiguous
        and seqs[:1] == [1],
        f"n={len(seqs)}/{total_events} first={seqs[:1]} last={seqs[-1:]}"
        f" contiguous={contiguous} unique={len(set(seqs)) == len(seqs)}",
    )
    # Cross-check against the durable log read straight from the database
    # file after the writers joined (the same committed fact set the
    # service itself reads through its port).
    durable = durable_log(db_path)
    merged = sorted((int(e["seq"]), e["eventId"]) for e in events)
    ok &= record(
        "a09-merged-equals-durable-log",
        merged == durable,
        f"merged={len(merged)} durable={len(durable)}",
    )
    terminal = [
        e for e in events
        if e.get("eventType") == "run_state_changed"
        and e.get("payload", {}).get("to") == "completed"
    ]
    ok &= record(
        "a09-terminal-events-recovered",
        len(terminal) == PRESEEED_RUNS + RACED_RUNS,
        f"completed transitions={len(terminal)} ({PRESEEED_RUNS + RACED_RUNS} runs)",
    )
    with open(os.path.join(out_dir, "a09-merged-view.json"), "w",
              encoding="utf-8") as handle:
        json.dump({"controls": controls, "events": events,
                   "snapshotSeq": snapshot_seq}, handle, indent=1)

    # ── A10: expired cursor → explicit snapshot_required, rebuild ────────
    execute(host, port, bearer, "a10-seed")
    status, body = http(
        host, port, "GET", f"/lingxi/v1/sessions/{STREAM}/events?limit=2",
        bearer,
    )
    page = json.loads(body)
    old_cursor = page["nextCursor"]
    floor = 8  # 6 a09 runs + 1 a10 seed run => 14 events; keep 8..=14
    conn = sqlite3.connect(db_path, timeout=10)
    removed = conn.execute(
        "DELETE FROM key_events WHERE stream_id = ? AND seq < ?", (STREAM, floor)
    ).rowcount
    conn.commit()
    conn.close()
    ok &= record("a10-purge-rows", removed == floor - 1, f"removed={removed}")

    status, body = http(
        host, port, "GET",
        f"/lingxi/v1/sessions/{STREAM}/events?cursor="
        + old_cursor.replace("+", "%2B"),
        bearer,
    )
    error = json.loads(body) if body else {}
    ok &= record(
        "a10-http-explicit-cursor-expired",
        status == 409
        and error.get("code") == "cursor_expired"
        and error.get("details", {}).get("reason") == "snapshot_required"
        and int(error.get("details", {}).get("floorSeq", "0")) == floor,
        f"status={status} body={body[:200]}",
    )

    sock = ws_connect(host, port, bearer)
    send_frame(sock, subscribe_frame(STREAM, old_cursor).encode())
    kind, _, text = recv_frame(sock)
    directive = json.loads(text) if kind == "text" else {}
    ok &= record(
        "a10-ws-snapshot-required-control",
        directive.get("frameKind") == "control"
        and directive.get("type") == "snapshot_required"
        and int(directive.get("floorSeq", "0")) == floor
        and directive.get("reason") == "events_truncated",
        text[:200],
    )
    # The directive is NOT a close: the same socket still serves requests.
    send_frame(sock, json.dumps(
        {"type": "session_read", "sessionId": STREAM}).encode())
    kind, _, text = recv_frame(sock)
    ok &= record("a10-socket-still-open", kind == "text" and "runCount" in text,
                 text[:120])

    # Rebuild from a fresh snapshot and diff against the current authority.
    send_frame(sock, subscribe_frame(STREAM).encode())
    controls, events = read_until_events(sock, 7, drain_timeout=5.0)
    sock.close()
    seqs = [int(e["seq"]) for e in events]
    status, body = http(
        host, port, "GET", f"/lingxi/v1/sessions/{STREAM}/events", bearer)
    authority_page = json.loads(body)
    auth_ids = [e["eventId"] for e in authority_page["items"]]
    ok &= record(
        "a10-rebuild-matches-authority",
        seqs[:1] == [floor]
        and [e["eventId"] for e in events] == auth_ids
        and len(events) == len(authority_page["items"]),
        f"rebuilt={len(events)} from={seqs[:1]} authority={len(auth_ids)}",
    )
    with open(os.path.join(out_dir, "a10-rebuild-diff.json"), "w",
              encoding="utf-8") as handle:
        json.dump({
            "rebuilt": [{"seq": e["seq"], "eventId": e["eventId"]} for e in events],
            "authority": [{"seq": e["seq"], "eventId": e["eventId"]}
                          for e in authority_page["items"]],
            "equal": [e["eventId"] for e in events] == auth_ids,
        }, handle, indent=1)

    # ── REVIEW-R1 F03 repair: purge-all stale cursor → snapshot_required,
    #    NOT future_cursor (recovery vocabulary; exercised on sess_local_beta
    #    so the alpha stream and the cross-check above stay untouched) ──────
    BETA = "sess_local_beta"
    execute(host, port, bearer, "f03-seed", session=BETA)
    status, body = http(
        host, port, "GET", f"/lingxi/v1/sessions/{BETA}/events?limit=1", bearer)
    page = json.loads(body)
    beta_cursor = page["nextCursor"]
    ok &= record("f03-beta-page",
                 status == 200 and bool(beta_cursor), body[:120])

    conn = sqlite3.connect(db_path, timeout=10)
    removed = conn.execute(
        "DELETE FROM key_events WHERE stream_id = ?", (BETA,)
    ).rowcount
    conn.commit()
    conn.close()
    ok &= record("f03-purge-all-rows", removed == 2, f"removed={removed}")

    # HTTP face: the stale cursor is the 409 cursor_expired rebuild
    # directive with NO floorSeq (nothing is retained to name).
    status, body = http(
        host, port, "GET",
        f"/lingxi/v1/sessions/{BETA}/events?cursor="
        + beta_cursor.replace("+", "%2B"),
        bearer,
    )
    error = json.loads(body) if body else {}
    ok &= record(
        "f03-http-stale-cursor-snapshot-required",
        status == 409
        and error.get("code") == "cursor_expired"
        and error.get("details", {}).get("reason") == "snapshot_required"
        and "floorSeq" not in error.get("details", {}),
        f"status={status} body={body[:200]}",
    )

    # WS face: the snapshot_required CONTROL frame (reason events_truncated,
    # no floorSeq) — not an invalid_message/close 4409 future_cursor.
    sock = ws_connect(host, port, bearer)
    send_frame(sock, subscribe_frame(BETA, beta_cursor).encode())
    kind, _, text = recv_frame(sock)
    directive = json.loads(text) if kind == "text" else {}
    ok &= record(
        "f03-ws-stale-cursor-snapshot-required",
        directive.get("frameKind") == "control"
        and directive.get("type") == "snapshot_required"
        and directive.get("reason") == "events_truncated"
        and "floorSeq" not in directive,
        text[:200],
    )
    # The directive is NOT a close: the same socket still serves requests.
    send_frame(sock, json.dumps(
        {"type": "session_read", "sessionId": BETA}).encode())
    kind, _, text = recv_frame(sock)
    ok &= record("f03-socket-still-open", kind == "text" and "runCount" in text,
                 text[:120])
    # Rebuild on the emptied stream: an explicit EMPTY cut (snapshotSeq 0),
    # never a rejection, never silence.
    send_frame(sock, subscribe_frame(BETA).encode())
    kind, _, text = recv_frame(sock)
    rebuild = json.loads(text) if kind == "text" else {}
    ok &= record(
        "f03-empty-rebuild-cut",
        rebuild.get("type") == "subscribed"
        and rebuild.get("snapshotSeq") == "0",
        text[:160],
    )
    sock.close()

    # ── negatives ──────────────────────────────────────────────────────────
    sock = ws_connect(host, port, bearer)
    send_frame(sock, subscribe_frame("stream_never_existed").encode())
    frames = drain(sock, timeout=3.0)
    ok &= record(
        "neg-unknown-stream-ws",
        any("stream_not_found" in t for _, _, t in frames)
        and any(k == "close" and c == 4404 for k, c, _ in frames),
        str(frames)[:200],
    )
    sock.close()
    status, _ = http(host, port, "GET",
                     "/lingxi/v1/sessions/stream_never_existed/events", bearer)
    ok &= record("neg-unknown-stream-http-404", status == 404, f"status={status}")

    sock = ws_connect(host, port, bearer)
    send_frame(sock, subscribe_frame(
        STREAM, forge_cursor(STREAM, 99999)).encode())
    frames = drain(sock, timeout=3.0)
    ok &= record(
        "neg-future-cursor-rejected",
        any("future_cursor" in t for _, _, t in frames),
        str(frames)[:200],
    )
    sock.close()

    sock = ws_connect(host, port, bearer)
    send_frame(sock, subscribe_frame(STREAM, "garbage!!not-base64").encode())
    frames = drain(sock, timeout=3.0)
    ok &= record(
        "neg-malformed-cursor-rejected",
        any("malformed_cursor" in t for _, _, t in frames),
        str(frames)[:200],
    )
    sock.close()

    sock = ws_connect(host, port, bearer)
    send_frame(sock, subscribe_frame(STREAM).encode())
    read_until_events(sock, 1, drain_timeout=5.0)
    send_frame(sock, subscribe_frame(STREAM).encode())  # duplicate
    frames = drain(sock, timeout=3.0)
    ok &= record(
        "neg-duplicate-subscribe-closed",
        any("already_subscribed" in t for _, _, t in frames)
        and any(k == "close" and c == 4409 for k, c, _ in frames),
        str(frames)[:200],
    )
    sock.close()

    status, body = http(host, port, "POST", "/lingxi/v1/devices/credentials",
                        bearer,
                        json.dumps({"userId": "user_remote_b",
                                    "scopes": ["chat"]}))
    secret = json.loads(body)["secret"] if status == 201 else ""
    ok &= record("neg-credential-issued", status == 201, f"status={status}")
    status, _ = http(host, port, "GET",
                     f"/lingxi/v1/sessions/{STREAM}/events", secret)
    ok &= record("neg-cross-principal-403", status == 403, f"status={status}")

    with open(os.path.join(out_dir, "matrix.json"), "w",
              encoding="utf-8") as handle:
        json.dump(results, handle, indent=1)
    failed = [r["case"] for r in results if not r["ok"]]
    if failed:
        print(f"PROBE FAILED: {failed}", file=sys.stderr)
        sys.exit(1)
    print("PROBE OK: all cases passed")


if __name__ == "__main__":
    main()
