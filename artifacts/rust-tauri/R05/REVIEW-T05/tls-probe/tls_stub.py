import json
import socket
import ssl
import sys
import threading

# Controlled TLS stub for the REVIEW-T05 C12 proof:
# serves a fixed JSON answer over TLS with the SELF-SIGNED certificate in
# /tmp/review_t05_cert.pem (never added to any trust store).

ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain("/tmp/review_t05_cert.pem", "/tmp/review_t05_key.pem")

srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", 0))
srv.listen(4)
port = srv.getsockname()[1]
print(f"PORT={port}", flush=True)

handshakes = {"accepted": 0, "failed": 0}
stop = threading.Event()


def serve():
    srv.settimeout(0.5)
    while not stop.is_set():
        try:
            conn, _ = srv.accept()
        except socket.timeout:
            continue
        except OSError:
            break
        try:
            tls = ctx.wrap_socket(conn, server_side=True)
            handshakes["accepted"] += 1
            req = tls.recv(65536)
            body = json.dumps({"ok": True}).encode()
            tls.sendall(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
                + str(len(body)).encode()
                + b"\r\nConnection: close\r\n\r\n"
                + body
            )
            tls.close()
        except ssl.SSLError:
            handshakes["failed"] += 1
        except OSError:
            pass


threading.Thread(target=serve, daemon=True).start()

# control plane: read commands from stdin ("stats" / "stop")
for line in sys.stdin:
    line = line.strip()
    if line == "stats":
        print(json.dumps(handshakes), flush=True)
    elif line == "stop":
        stop.set()
        break
