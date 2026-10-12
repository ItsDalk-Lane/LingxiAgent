#!/usr/bin/env python3
"""R02-T03 binary-level WS matrix probe (stdlib only).

Drives REAL WebSocket upgrades against the real lingxi-service binary:
handshake shape, ticket lifecycle (expired / replayed / wrong path),
Origin policy on WS, and the post-upgrade lingxi.wire handshake plus an
authorized session_read. Prints one JSON line per case; exits non-zero if
any expected-rejection case unexpectedly succeeds (or vice versa).

Used by scripts/rust-tauri/r02_t03_auth_matrix.sh; evidence lands in
artifacts/rust-tauri/R02/T03/ws-matrix.json.
"""

import base64
import hashlib
import json
import os
import socket
import struct
import sys
import time

WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
EXPECTED_CASES = {
    "ws-evil-origin", "ws-foreign-host", "ws-no-credential",
    "ws-ticket-legit-origin-upgrade", "ws-server-hello", "ws-session-read-owner",
    "ws-ticket-replay", "ws-no-origin-cli-bearer", "ws-cli-server-hello",
    "ws-ticket-expired",
    "ws-live-revoke-issue", "ws-live-revoke-upgrade", "ws-live-revoke-hello",
    "ws-live-revoke-action", "ws-live-revoke-denial-frame",
    "ws-live-revoke-idle-close",
    "ws-web-logout-password-setup", "ws-web-logout-login",
    "ws-web-logout-cookie-issued", "ws-web-logout-ticket",
    "ws-web-logout-upgrade", "ws-web-logout-hello", "ws-web-logout-action",
    "ws-web-logout-denial-frame", "ws-web-logout-idle-close",
    "unknown-trailing-slash-/lingxi/v1/health/",
    "unknown-trailing-slash-/lingxi/v1/web-auth/session/",
} | {
    f"ws-unmasked-{name}-{phase}"
    for name in ("text", "close", "ping", "pong")
    for phase in ("upgrade", "hello", "protocol-close")
} | {
    f"ws-invalid-upgrade-{name}"
    for name in (
        "missing-version", "missing-key", "malformed-key", "wrong-upgrade",
        "substring-connection",
    )
}


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


def recv_body(sock, headers):
    for line in headers.splitlines():
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
            return sock.recv(length).decode("utf-8", "replace")
    return ""


def upgrade(host, port, path, headers):
    """Returns (status, headers, body_or_stream)."""
    sock = socket.create_connection((host, port), timeout=10)
    key = ws_key()
    request = (
        f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
        "Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
    )
    for name, value in headers:
        request += f"{name}: {value}\r\n"
    request += "\r\n"
    sock.sendall(request.encode())
    head = recv_headers(sock)
    status = int(head.split()[1]) if head.split() and len(head.split()) > 1 else 0
    if status != 101:
        body = recv_body(sock, head)
        sock.close()
        return status, head, body, None
    accept = ""
    for line in head.splitlines():
        if line.lower().startswith("sec-websocket-accept:"):
            accept = line.split(":", 1)[1].strip()
    expected = base64.b64encode(
        hashlib.sha1((key + WS_GUID).encode()).digest()
    ).decode()
    assert accept == expected, f"server accept key wrong: {accept} != {expected}"
    return 101, head, "", sock


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
    assert length <= 1024 * 1024, "server frame too large"
    payload = recv_exact(sock, length) if length else b""
    if opcode == 0x8:
        code = struct.unpack(">H", payload[:2])[0] if len(payload) >= 2 else 1005
        return ("close", code, payload[2:].decode("utf-8", "replace"))
    if opcode == 0x9:
        return ("ping", 0, payload.decode("utf-8", "replace"))
    if opcode == 0xA:
        return ("pong", 0, "")
    return ("text", 0, payload.decode("utf-8", "replace"))


def observed_frame(sock):
    try:
        return recv_frame(sock)
    except (EOFError, OSError) as exc:
        return ("read_error", 0, type(exc).__name__)


def http_post(host, port, path, bearer, body=None):
    """Tiny HTTP POST used to mint ws tickets through the real service."""
    payload = (body or "").encode()
    request = (
        f"POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
        f"Authorization: Bearer {bearer}\r\n"
        "Content-Type: application/json\r\n"
        f"Content-Length: {len(payload)}\r\nConnection: close\r\n\r\n"
    ).encode() + payload
    sock = socket.create_connection((host, port), timeout=10)
    sock.sendall(request)
    buf = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buf += chunk
    sock.close()
    text = buf.decode("utf-8", "replace")
    status = int(text.split()[1]) if len(text.split()) > 1 else 0
    body = text.split("\r\n\r\n", 1)[1] if "\r\n\r\n" in text else ""
    return status, body


def raw_http_status(host, port, request):
    sock = socket.create_connection((host, port), timeout=10)
    sock.sendall(request.encode())
    head = recv_headers(sock)
    sock.close()
    return int(head.split()[1]) if len(head.split()) > 1 else 0


def http_call(host, port, method, path, payload="", bearer=None, cookie=None):
    data = payload.encode()
    headers = (
        f"{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
        f"Connection: close\r\nContent-Length: {len(data)}\r\n"
        "Content-Type: application/json\r\n"
    )
    if bearer is not None:
        headers += f"Authorization: Bearer {bearer}\r\n"
    if cookie is not None:
        headers += f"Cookie: {cookie}\r\n"
    sock = socket.create_connection((host, port), timeout=10)
    sock.sendall((headers + "\r\n").encode() + data)
    raw = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            break
        raw += chunk
    sock.close()
    text = raw.decode("utf-8", "replace")
    head, _, body = text.partition("\r\n\r\n")
    status = int(head.split()[1]) if len(head.split()) > 1 else 0
    return status, head, body


def invalid_upgrade_status(host, port, bearer, **replacements):
    headers = {
        "Upgrade": "websocket",
        "Connection": "Upgrade",
        "Sec-WebSocket-Key": ws_key(),
        "Sec-WebSocket-Version": "13",
    }
    headers.update(replacements)
    fields = "".join(
        f"{name}: {value}\r\n" for name, value in headers.items() if value is not None
    )
    return raw_http_status(
        host, port,
        f"GET /lingxi/v1/ws HTTP/1.1\r\nHost: {host}:{port}\r\n"
        f"Authorization: Bearer {bearer}\r\n{fields}\r\n",
    )


def main():
    host, port, bearer = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    results = []

    def record(case, expected, actual, ok_extra=""):
        ok = actual == expected
        results.append(
            {"case": case, "expected": expected, "actual": actual, "ok": ok,
             "detail": ok_extra}
        )
        return ok

    all_ok = True

    # 1. evil Origin on WS upgrade -> 403 before the 101.
    status, head, body, _ = upgrade(host, port, "/lingxi/v1/ws",
                                    [("Origin", "http://evil.example"),
                                     ("Authorization", f"Bearer {bearer}")])
    all_ok &= record("ws-evil-origin", 403, status, body)

    # 2. foreign Host on WS upgrade -> 403 (rebinding guard).
    sock = socket.create_connection((host, port), timeout=10)
    sock.sendall(
        f"GET /lingxi/v1/ws HTTP/1.1\r\nHost: rebinder.example\r\n"
        "Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {ws_key()}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode()
    )
    head = recv_headers(sock)
    status = int(head.split()[1]) if len(head.split()) > 1 else 0
    sock.close()
    all_ok &= record("ws-foreign-host", 403, status, head.splitlines()[0] if head else "")

    # 3. no credential -> 401.
    status, _, body, _ = upgrade(host, port, "/lingxi/v1/ws", [])
    all_ok &= record("ws-no-credential", 401, status, body)

    # 4. legit Origin + ticket -> 101 + lingxi.wire handshake + session_read.
    status, body = http_post(host, port, "/lingxi/v1/ws-ticket", bearer)
    assert status == 200, f"ticket issue failed: {status} {body}"
    ticket = json.loads(body)["ticket"]
    origin = f"http://localhost:{port}"
    status, _, _, sock = upgrade(
        host, port, f"/lingxi/v1/ws?wsTicket={ticket}",
        [("Origin", origin)],
    )
    all_ok &= record("ws-ticket-legit-origin-upgrade", 101, status)
    hello = json.dumps({
        "protocol": "lingxi.wire", "clientKind": "desktop", "clientVersion": "1",
        "protocolMin": 1, "protocolMax": 1,
    })
    send_frame(sock, hello.encode())
    kind, _, text = recv_frame(sock)
    server_hello_ok = kind == "text" and '"lingxi.wire"' in text and '"selectedProtocol":1' in text
    all_ok &= record("ws-server-hello", True, server_hello_ok, text[:120])
    request = json.dumps({"type": "session_read", "sessionId": "sess_local_alpha"})
    send_frame(sock, request.encode())
    kind, _, text = recv_frame(sock)
    read_ok = kind == "text" and '"sessionReadResult"' in text and '"sessionId":"sess_local_alpha"' in text
    all_ok &= record("ws-session-read-owner", True, read_ok, text[:120])
    sock.close()

    # 5. ticket REPLAY: same ticket again -> 401 invalid_ws_ticket.
    status, _, body, _ = upgrade(host, port, f"/lingxi/v1/ws?wsTicket={ticket}", [])
    all_ok &= record("ws-ticket-replay", 401, status, body)

    # 6. no-Origin CLI shape with bearer -> 101 + handshake.
    status, _, _, sock = upgrade(host, port, "/lingxi/v1/ws",
                                 [("Authorization", f"Bearer {bearer}")])
    all_ok &= record("ws-no-origin-cli-bearer", 101, status)
    send_frame(sock, hello.encode())
    kind, _, text = recv_frame(sock)
    all_ok &= record("ws-cli-server-hello", True,
                     kind == "text" and '"lingxi.wire"' in text, text[:120])
    # cross-principal read over WS with a foreign device credential is
    # covered on the HTTP side of the matrix; here close cleanly.
    send_frame(sock, b"", opcode=0x8)
    sock.close()

    # T03 F01/F03/F04: 原评审指出的失败分支逐项读取真实线级结果。
    for opcode, name in [(0x1, "text"), (0x8, "close"), (0x9, "ping"), (0xA, "pong")]:
        status, _, _, sock = upgrade(
            host, port, "/lingxi/v1/ws", [("Authorization", f"Bearer {bearer}")]
        )
        all_ok &= record(f"ws-unmasked-{name}-upgrade", 101, status)
        if sock is None:
            continue
        sock.settimeout(3)
        send_frame(sock, hello.encode())
        hello_kind, _, _ = observed_frame(sock)
        all_ok &= record(f"ws-unmasked-{name}-hello", "text", hello_kind)
        if hello_kind != "text":
            sock.close()
            continue
        sock.sendall(bytes([0x80 | opcode, 0]))
        kind, code, _ = observed_frame(sock)
        all_ok &= record(f"ws-unmasked-{name}-protocol-close", ("close", 1002), (kind, code))
        sock.close()

    for name, headers in [
        ("missing-version", {"Sec-WebSocket-Version": None}),
        ("missing-key", {"Sec-WebSocket-Key": None}),
        ("malformed-key", {"Sec-WebSocket-Key": "not-base64"}),
        ("wrong-upgrade", {"Upgrade": "h2c"}),
        ("substring-connection", {"Connection": "notupgrade"}),
    ]:
        status = invalid_upgrade_status(host, port, bearer, **headers)
        all_ok &= record(f"ws-invalid-upgrade-{name}", 400, status)

    for path in ["/lingxi/v1/health/", "/lingxi/v1/web-auth/session/"]:
        status = raw_http_status(
            host, port,
            f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n",
        )
        all_ok &= record(f"unknown-trailing-slash-{path}", 401, status)

    # T03 F02: 独立凭证在运行中的服务上撤销；已升级且静默的连接应自行关闭。
    status, body = http_post(
        host, port, "/lingxi/v1/devices/credentials", bearer,
        json.dumps({"userId": "user_ws_revoke", "scopes": ["chat"]}),
    )
    all_ok &= record("ws-live-revoke-issue", 201, status)
    if status == 201:
        issued = json.loads(body)
        device_secret = issued["secret"]
        credential_id = issued["credentialId"]
        status, _, _, sock = upgrade(
            host, port, "/lingxi/v1/ws",
            [("Authorization", f"Bearer {device_secret}")],
        )
        all_ok &= record("ws-live-revoke-upgrade", 101, status)
        if sock is not None:
            sock.settimeout(4)
            send_frame(sock, hello.encode())
            kind, _, _ = observed_frame(sock)
            all_ok &= record("ws-live-revoke-hello", "text", kind)
            if kind == "text":
                revoke_status, _ = http_post(
                    host, port,
                    f"/lingxi/v1/devices/credentials/{credential_id}/revoke", bearer,
                )
                all_ok &= record("ws-live-revoke-action", 200, revoke_status)
                kind, code, text = observed_frame(sock)
                # 先发可诊断的错误帧，再发 4401 close；不得把失败视为成功事件。
                all_ok &= record(
                    "ws-live-revoke-denial-frame", True,
                    kind == "text" and "invalid_credential" in text,
                )
                kind, code, _ = observed_frame(sock)
                all_ok &= record("ws-live-revoke-idle-close", ("close", 4401), (kind, code))
            sock.close()

    # 同根相邻入口：退出浏览器会话必须使已消费票据的静默 WS 自动失效。
    password = base64.urlsafe_b64encode(os.urandom(18)).decode().rstrip("=")
    set_status, _, _ = http_call(
        host, port, "PUT", "/lingxi/v1/access/account/password",
        json.dumps({"password": password}), bearer=bearer,
    )
    all_ok &= record("ws-web-logout-password-setup", 200, set_status)
    if set_status == 200:
        login_status, login_headers, _ = http_call(
            host, port, "POST", "/lingxi/v1/web-auth/login",
            json.dumps({
                "username": "local", "password": password, "clientKind": "desktop"
            }),
        )
        all_ok &= record("ws-web-logout-login", 200, login_status)
        cookie = next((
            line.split(":", 1)[1].strip().split(";", 1)[0]
            for line in login_headers.splitlines()
            if line.lower().startswith("set-cookie:")
        ), None)
        all_ok &= record("ws-web-logout-cookie-issued", True,
                         cookie is not None and cookie.startswith("hana_session="))
        if login_status == 200 and cookie is not None:
            ticket_status, _, ticket_body = http_call(
                host, port, "POST", "/lingxi/v1/ws-ticket", "{}", cookie=cookie,
            )
            all_ok &= record("ws-web-logout-ticket", 200, ticket_status)
            if ticket_status == 200:
                web_ticket = json.loads(ticket_body)["ticket"]
                status, _, _, sock = upgrade(
                    host, port, f"/lingxi/v1/ws?wsTicket={web_ticket}", []
                )
                all_ok &= record("ws-web-logout-upgrade", 101, status)
                if sock is not None:
                    sock.settimeout(4)
                    send_frame(sock, hello.encode())
                    kind, _, _ = observed_frame(sock)
                    all_ok &= record("ws-web-logout-hello", "text", kind)
                    if kind == "text":
                        logout_status, _, _ = http_call(
                            host, port, "POST", "/lingxi/v1/web-auth/logout",
                            cookie=cookie,
                        )
                        all_ok &= record("ws-web-logout-action", 200, logout_status)
                        kind, _, text = observed_frame(sock)
                        all_ok &= record("ws-web-logout-denial-frame", True,
                                         kind == "text" and "invalid_credential" in text)
                        kind, code, _ = observed_frame(sock)
                        all_ok &= record("ws-web-logout-idle-close", ("close", 4401),
                                         (kind, code))
                    sock.close()

    # 7. EXPIRED ticket: mint one and wait out the 30s TTL.
    status, body = http_post(host, port, "/lingxi/v1/ws-ticket", bearer)
    assert status == 200, f"ticket issue failed: {status} {body}"
    stale_ticket = json.loads(body)["ticket"]
    expires_at = json.loads(body)["expiresAtUnixMs"]
    wait_ms = expires_at - int(time.time() * 1000) + 1500
    if wait_ms > 0:
        time.sleep(wait_ms / 1000 + 0.2)
    status, _, body, _ = upgrade(host, port, f"/lingxi/v1/ws?wsTicket={stale_ticket}", [])
    all_ok &= record("ws-ticket-expired", 401, status, body)

    names = [item["case"] for item in results]
    issues = []
    if len(names) != len(set(names)):
        issues.append("duplicate case identity")
    if set(names) != EXPECTED_CASES:
        issues.append("case identities differ from pinned WS contract")
    all_ok = all_ok and not issues
    print(json.dumps({"results": results, "all_ok": all_ok, "issues": issues},
                     ensure_ascii=False, indent=1))
    sys.exit(0 if all_ok else 1)


if __name__ == "__main__":
    main()
