"""空隔离资料、同构建副本的一次健康读取；只观察连接，不改系统网络设置。"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import socket
import subprocess
import tempfile
import threading
import time

HERE = Path(__file__).resolve().parent
OUT = HERE.parent
REPO = HERE.parents[4]
sha = lambda data: hashlib.sha256(data).hexdigest()
build_path = OUT / 't08g-build.json'
build = json.loads(build_path.read_text())
assert build['exitCode'] == 0 and build['sourceUnchanged']
original = Path(build['binary']['path'])
assert sha(original.read_bytes()) == build['binary']['sha256']
assert not (HERE / 'receipt.json').exists()
root = Path(tempfile.mkdtemp(prefix='lingxi-r07-lan-layer-g1-', dir='/private/tmp'))
binary = root / 'owned-service'
shutil.copyfile(original, binary)
binary.chmod(0o500)
home = root / 'home'
home.mkdir()
lan_ip = subprocess.check_output(['/usr/sbin/ipconfig', 'getifaddr', 'en0'], text=True).strip()
assert not lan_ip.startswith('127.')
record = dict(scope='OWNED_SAME_G_BUILD_LAYER_DIAGNOSIS_NOT_ACCEPTANCE',
              startedAt=datetime.datetime.now(datetime.timezone.utc).isoformat(),
              binarySha256Before=sha(binary.read_bytes()), buildReceiptSha256=sha(build_path.read_bytes()),
              sourceFingerprint=build['sourceFingerprint'], isolatedHome=str(home), binary=str(binary),
              interface='en0', lanIp=lan_ip, networkSettingsChanged=False,
              credentialsUsed=False, manualClockInjected=False, requests=[], samples=[])
env = dict(os.environ)
env.pop('LINGXI_HOME', None)
env['LINGXI_PRODUCT_DIR'] = str(REPO / 'lib')
proc = subprocess.Popen([str(binary), '--home', str(home), '--bind', '0.0.0.0:0',
                         '--network-mode', 'lan', '--shutdown-timeout-ms', '2000'],
                        env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
ready = queue.Queue()
stdout, stderr = [], []
def collect(pipe, lines, find=False):
    for line in pipe:
        lines.append(line)
        if find and (match := re.search(r'LINGXI_SERVICE_READY addr=(\S+)', line)):
            ready.put(match[1])
threads = [threading.Thread(target=collect, args=(proc.stdout, stdout, True), daemon=True),
           threading.Thread(target=collect, args=(proc.stderr, stderr), daemon=True)]
for thread in threads:
    thread.start()
stop_samples = threading.Event()
def command(args):
    result = subprocess.run(args, capture_output=True, text=True, timeout=3)
    return dict(exitCode=result.returncode, stdout=result.stdout.splitlines(), stderr=result.stderr.splitlines())
def sample(port):
    began = time.monotonic()
    pattern = re.compile(r'[.:]' + str(port) + r'\b')
    while not stop_samples.is_set():
        item = dict(elapsed=round(time.monotonic() - began, 3))
        try:
            item['ownedTcpFds'] = command(['/usr/sbin/lsof', '-nP', '-a', '-p', f'{proc.pid},{os.getpid()}', '-iTCP'])
            for key, args in [('tcpStates', ['/usr/sbin/netstat', '-anv', '-p', 'tcp']),
                              ('listenQueues', ['/usr/sbin/netstat', '-Lan', '-p', 'tcp'])]:
                value = command(args)
                value['stdout'] = [line for line in value['stdout'] if pattern.search(line)]
                item[key] = value
        except Exception as error:
            item['observationErrorType'] = type(error).__name__
        record['samples'].append(item)
        stop_samples.wait(.2)
def request(label, address, port, budget):
    item = dict(label=label, method='GET', path='/lingxi/v1/health', destination=[address, port],
                budgetSeconds=budget, connected=False, requestWritten=False, responseBytes=0)
    raw = b''
    began = time.monotonic()
    sock = None
    try:
        sock = socket.create_connection((address, port), timeout=10)
        item.update(connected=True, localSocket=list(sock.getsockname()), peerSocket=list(sock.getpeername()))
        deadline = time.monotonic() + budget
        sock.settimeout(budget)
        sock.sendall(f'GET /lingxi/v1/health HTTP/1.1\r\nHost: {address}:{port}\r\nConnection: close\r\n\r\n'.encode())
        item['requestWritten'] = True
        while True:
            sock.settimeout(max(.001, deadline - time.monotonic()))
            chunk = sock.recv(4096)
            if not chunk:
                break
            raw += chunk
        if b'\r\n\r\n' in raw:
            item['status'] = int(raw.split(b'\r\n', 1)[0].split()[1])
    except (OSError, TimeoutError) as error:
        item.update(errorType=type(error).__name__, timedOut=isinstance(error, TimeoutError))
    finally:
        if sock:
            sock.close()
        item.update(responseBytes=len(raw), responseSha256=sha(raw), elapsedSeconds=round(time.monotonic() - began, 3))
        record['requests'].append(item)
try:
    port = int(ready.get(timeout=20).rsplit(':', 1)[1])
    record['port'] = port
    record['route'] = command(['/sbin/route', '-n', 'get', lan_ip])
    sampler = threading.Thread(target=sample, args=(port,), daemon=True)
    sampler.start()
    request('loopback-before', '127.0.0.1', port, 5)
    request('lan-only-health', lan_ip, port, 20)
    request('same-process-loopback-after', '127.0.0.1', port, 5)
except Exception as error:
    record['probeErrorType'] = type(error).__name__
finally:
    stop_samples.set()
    if 'sampler' in globals():
        sampler.join(timeout=10)
    proc.send_signal(signal.SIGTERM)
    try:
        record['serviceExitCode'] = proc.wait(timeout=7)
    except subprocess.TimeoutExpired:
        proc.kill()
        record.update(serviceExitCode=proc.wait(timeout=5), forcedKill=True)
    for thread in threads:
        thread.join(timeout=2)
    for name, lines in [('stdout', stdout), ('stderr', stderr)]:
        content = ''.join(lines)
        (HERE / (name + '.log')).write_text(content)
        record[name + 'Sha256'] = sha(content.encode())
    record.update(binarySha256After=sha(binary.read_bytes()), probeSha256=sha(Path(__file__).read_bytes()),
                  endedAt=datetime.datetime.now(datetime.timezone.utc).isoformat(), serviceGone=proc.poll() is not None,
                  conclusion='按双端FD/内核连接/队列实际观测分层；本记录不自动归因任何过滤器，也不声称真实第二设备通过')
    (HERE / 'receipt.json').write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(dict(requests=record['requests'], samples=len(record['samples']), serviceExitCode=record['serviceExitCode'], receiptSha256=sha((HERE/'receipt.json').read_bytes())), ensure_ascii=False))
raise SystemExit(0 if all(item.get('status') == 200 for item in record['requests']) and len(record['requests']) == 3 else 1)
