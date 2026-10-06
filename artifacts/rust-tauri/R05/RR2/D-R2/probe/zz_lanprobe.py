# Zero-Lingxi-code LAN self-connection probe, Apple-signed binary control.
# Server binds 0.0.0.0:<port> in a thread; main connects to <addr>:<port>,
# exchanges one message with hard timeouts, prints PROBE RESULT.
import socket, sys, threading, time

port = int(sys.argv[2])
addr = sys.argv[1]
result = {}

def server():
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("0.0.0.0", port))
    s.listen(4)
    s.settimeout(12)
    try:
        c, _ = s.accept()
        print("server: accepted", flush=True)
        data = c.recv(256)
        print(f"server: recv {len(data)} bytes", flush=True)
        c.sendall(b"PROBE-RESPONSE")
        c.close()
    except Exception as e:  # noqa: BLE001 - probe
        print(f"server: {type(e).__name__}: {e}", flush=True)
    finally:
        s.close()

t = threading.Thread(target=server, daemon=True)
t.start()
time.sleep(0.3)
c = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
c.settimeout(6)
try:
    c.connect((addr, port))
    print("client: connected", flush=True)
    c.sendall(b"PING")
    print("client: sent", flush=True)
    data = c.recv(256)
    if not data:
        print("PROBE RESULT: recv-empty", flush=True)
    else:
        print(f"PROBE RESULT: ok ({len(data)} bytes)", flush=True)
except Exception as e:  # noqa: BLE001 - probe
    print(f"PROBE RESULT: {type(e).__name__}: {e}", flush=True)
finally:
    c.close()
