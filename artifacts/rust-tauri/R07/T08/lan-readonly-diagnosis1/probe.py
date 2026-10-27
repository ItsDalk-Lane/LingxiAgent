"""只读诊断：仅空隔离资料、本机真实网卡 TCP，不改网络或产品。"""
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import signal
import socket
import subprocess
import tempfile
import threading
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[5]
BINARY = Path('/private/tmp/lingxi-r07-build/debug/lingxi-service')
sha = lambda b: hashlib.sha256(b).hexdigest()
result = {'scope': 'OWNED_EMPTY_HOME_LAN_TCP_DIAGNOSIS_NOT_GATE_PASS', 'requests': [], 'networkSettingsChanged': False, 'manualClockInjected': False}
home_root = Path(tempfile.mkdtemp(prefix='lingxi-r07-lan-readonly-'))
home = home_root / 'home'
home.mkdir()
result['isolatedHomeRoot'] = str(home_root)
result['binarySha256Before'] = sha(BINARY.read_bytes())
lan_ip = subprocess.check_output(['/usr/sbin/ipconfig', 'getifaddr', 'en0'], text=True).strip()
assert not lan_ip.startswith('127.')
result['actualInterface'] = 'en0'
result['actualLanIp'] = lan_ip
env = dict(os.environ)
env.pop('LINGXI_HOME', None)
env['LINGXI_PRODUCT_DIR'] = str(REPO / 'lib')
proc = subprocess.Popen([str(BINARY), '--home', str(home), '--bind', '0.0.0.0:0', '--network-mode', 'lan', '--shutdown-timeout-ms', '2000'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
ready = queue.Queue()
stdout, stderr = [], []
def collect(pipe, lines, look_ready=False):
    for line in pipe:
        lines.append(line)
        if look_ready and (m := re.search(r'LINGXI_SERVICE_READY addr=(\S+)', line)):
            ready.put(m[1])
threading.Thread(target=collect, args=(proc.stdout, stdout, True), daemon=True).start()
threading.Thread(target=collect, args=(proc.stderr, stderr), daemon=True).start()
owner = ''
credential = ''
def request(label, ip, port, method, path, headers=None, body=None, budget=20):
    started = time.monotonic()
    record = {'label': label, 'destination': f'{ip}:{port}', 'addressKind': 'loopback' if ip.startswith('127.') else 'actual-lan-interface', 'method': method, 'path': path, 'budgetSeconds': budget, 'connected': False, 'requestWritten': False, 'responseBytes': 0}
    raw = b''
    sock = None
    try:
        sock = socket.create_connection((ip, port), timeout=10)
        record['connected'] = True
        record['localSocket'] = list(sock.getsockname())
        record['peerSocket'] = list(sock.getpeername())
        data = body.encode() if body else b''
        hs = {'Host': f'{ip}:{port}', 'Connection': 'close', **(headers or {})}
        if body is not None:
            hs.update({'Content-Type': 'application/json', 'Content-Length': str(len(data))})
        message = (method + ' ' + path + ' HTTP/1.1\r\n' + ''.join(k+': '+v+'\r\n' for k,v in hs.items()) + '\r\n').encode() + data
        deadline = time.monotonic() + budget
        sock.settimeout(budget)
        sock.sendall(message)
        record['requestWritten'] = True
        while True:
            sock.settimeout(max(.001, deadline-time.monotonic()))
            chunk = sock.recv(4096)
            if not chunk:
                break
            raw += chunk
        if b'\r\n\r\n' in raw:
            head, payload = raw.split(b'\r\n\r\n', 1)
            record['status'] = int(head.split()[1])
            record['responseSha256'] = sha(raw)
            record['setCookiePresent'] = b'set-cookie:' in head.lower()
            try:
                value = json.loads(payload)
            except ValueError:
                value = None
        else:
            value = None
    except (TimeoutError, OSError) as error:
        record['errorType'] = type(error).__name__
        record['timedOut'] = isinstance(error, TimeoutError)
        value = None
    finally:
        if sock:
            sock.close()
        record['responseBytes'] = len(raw)
        record['elapsedSeconds'] = round(time.monotonic()-started, 3)
        result['requests'].append(record)
    return record, value

try:
    address = ready.get(timeout=20)
    port = int(address.rsplit(':', 1)[1])
    result['readyAddress'] = address
    owner = json.loads((home/'lingxi-service/local-token.json').read_text())['token']
    r, issued = request('loopback-issue-mobile', '127.0.0.1', port, 'POST', '/lingxi/v1/access/mobile-credentials', {'Authorization': 'Bearer '+owner}, '{"displayName":"isolated LAN diagnosis"}')
    assert r.get('status') == 200
    credential = issued['secret']
    body = json.dumps({'credential': credential, 'clientKind': 'mobile'})
    request('lan-first-foreign-origin-login', lan_ip, port, 'POST', '/lingxi/v1/web-auth/login', {'Origin': 'http://evil.example'}, body)
    request('loopback-foreign-origin-control', '127.0.0.1', port, 'POST', '/lingxi/v1/web-auth/login', {'Origin': 'http://evil.example'}, body, budget=5)
    request('lan-health', lan_ip, port, 'GET', '/lingxi/v1/health', budget=5)
    request('loopback-health-control', '127.0.0.1', port, 'GET', '/lingxi/v1/health', budget=5)
    request('lan-correct-origin-login', lan_ip, port, 'POST', '/lingxi/v1/web-auth/login', {'Origin': f'http://{lan_ip}:{port}'}, body, budget=5)
except Exception as error:
    result['probeErrorType'] = type(error).__name__
finally:
    proc.send_signal(signal.SIGTERM)
    try:
        result['serviceExitCode'] = proc.wait(timeout=7)
    except subprocess.TimeoutExpired:
        proc.kill()
        result['serviceExitCode'] = proc.wait(timeout=5)
        result['forcedKill'] = True
    for name, lines in [('stdout', stdout), ('stderr', stderr)]:
        content = ''.join(lines)
        for secret in [owner, credential]:
            if secret:
                content = content.replace(secret, '[credential-redacted]')
        (HERE/(name+'.log')).write_text(content)
        result[name+'Sha256'] = sha(content.encode())
    result['binarySha256After'] = sha(BINARY.read_bytes())
    result['binaryUnchanged'] = result['binarySha256Before'] == result['binarySha256After']
    result['probeSourceSha256'] = sha(Path(__file__).read_bytes())
    result['credentialFilesNotCopied'] = True
    (HERE/'receipt.json').write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
    print(json.dumps(result, ensure_ascii=False))
    assert proc.poll() is not None

raise SystemExit(1 if 'probeErrorType' in result or any('status' not in r for r in result['requests']) else 0)
