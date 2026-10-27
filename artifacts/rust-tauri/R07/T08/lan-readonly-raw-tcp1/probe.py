"""最小实际 TCP 对照；不经过 Rust、HTTP、身份、时钟或代理库。"""
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import threading
import time

here = Path(__file__).resolve().parent
lan = subprocess.check_output(['/usr/sbin/ipconfig', 'getifaddr', 'en0'], text=True).strip()
listener = socket.socket()
listener.bind(('0.0.0.0', 0))
listener.listen(4)
listener.settimeout(.2)
port = listener.getsockname()[1]
accepted = []
stopped = threading.Event()
def serve():
    while not stopped.is_set():
        try:
            peer, address = listener.accept()
        except socket.timeout:
            continue
        except OSError:
            break
        with peer:
            peer.settimeout(2)
            r = {'peer': list(address)}
            try:
                data = peer.recv(128)
                r['receivedBytes'] = len(data)
                peer.sendall(data)
            except OSError as e:
                r['errorType'] = type(e).__name__
            accepted.append(r)
worker = threading.Thread(target=serve)
worker.start()
requests = []
for ip in [lan, '127.0.0.1']:
    start = time.monotonic()
    r = {'destination': f'{ip}:{port}', 'addressKind': 'loopback' if ip.startswith('127.') else 'actual-lan-interface', 'budgetSeconds': 5, 'connected': False}
    try:
        with socket.create_connection((ip, port), timeout=5) as peer:
            r['connected'] = True
            peer.sendall(b'owned-local-diagnosis')
            data = peer.recv(128)
            r['receivedBytes'] = len(data)
            r['exactEcho'] = data == b'owned-local-diagnosis'
    except OSError as e:
        r['errorType'] = type(e).__name__
        r['receivedBytes'] = 0
    r['elapsedSeconds'] = round(time.monotonic()-start, 3)
    requests.append(r)
stopped.set()
listener.close()
worker.join(timeout=3)
result = {'scope': 'OWNED_RAW_TCP_NOT_PRODUCT_ACCEPTANCE', 'networkSettingsChanged': False, 'actualInterface': 'en0', 'actualLanIp': lan, 'requests': requests, 'serverAccepted': accepted, 'listenerClosed': listener.fileno() == -1, 'workerStopped': not worker.is_alive(), 'probeSourceSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
(here/'receipt.json').write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
print(json.dumps(result, ensure_ascii=False))
raise SystemExit(0 if all(r.get('exactEcho') for r in requests) else 1)
