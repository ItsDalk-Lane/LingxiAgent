# 真服务、真实供应商 HTTP 替身、实际模型工具与编辑器联合验证。
import hashlib
import base64
import socket
import struct
import urllib.parse
import http.server
import json
import os
import pathlib
import re
import sqlite3
import subprocess
import sys
import threading
import urllib.error
import urllib.request

root = pathlib.Path(os.environ['R06_SESSION_EVIDENCE'])
root.mkdir(parents=True, exist_ok=True)
root = root.resolve()
home = root / 'home'
home.mkdir()
workspace = root / 'workspace'
workspace.mkdir()
selected = root / 'selected'
selected.mkdir()
(workspace / 'owned.txt').write_text('WRONG GLOBAL WORKSPACE')
(selected / 'owned.txt').write_text('SELECTED SESSION WORKSPACE')
(selected / 'selected-only.txt').write_text('独占内容')
(workspace / 'global-only.txt').write_text('另一工作区')
requests = []
active_tool = 'read'
active_arguments = {'path': 'owned.txt'}


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(body)
        step = (len(requests) - 1) % 3
        if step < 2:
            name, args = ('mcp_describe_tool', {'name': active_tool}) if step == 0 else ('mcp_call', {'tool': active_tool, 'arguments': active_arguments})
            delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': f'focus-{len(requests)}', 'type': 'function', 'function': {'name': name, 'arguments': json.dumps(args)}}]}
            finish = 'tool_calls'
        else:
            delta, finish = {'role': 'assistant', 'content': '真实焦点会话完成'}, 'stop'
        frames = [{'choices': [{'index': 0, 'delta': delta, 'finish_reason': None}]}, {'choices': [{'index': 0, 'delta': {}, 'finish_reason': finish}]}, {'choices': [], 'usage': {'prompt_tokens': 20, 'completion_tokens': 4}}]
        payload = (''.join('data: ' + json.dumps(frame) + '\n\n' for frame in frames) + 'data: [DONE]\n\n').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


class Wire:
    def __init__(self, base, token):
        target = urllib.parse.urlsplit(base)
        self.socket = socket.create_connection((target.hostname, target.port), timeout=3)
        key = base64.b64encode(os.urandom(16)).decode()
        self.socket.sendall((f'GET /lingxi/v1/ws HTTP/1.1\r\nHost: {target.netloc}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {token}\r\n\r\n').encode())
        headers = b''
        while not headers.endswith(b'\r\n\r\n'):
            headers += self.exact(1)
        assert headers.startswith(b'HTTP/1.1 101'), headers
        self.send(json.dumps({'protocol': 'lingxi.wire', 'clientKind': 'test', 'clientVersion': '0', 'protocolMin': 1, 'protocolMax': 1}).encode())
        assert self.receive()['protocol'] == 'lingxi.wire'

    def exact(self, length):
        value = b''
        while len(value) < length:
            chunk = self.socket.recv(length - len(value))
            assert chunk, 'actual websocket closed'
            value += chunk
        return value

    def send(self, payload, opcode=1):
        mask = os.urandom(4)
        size = len(payload)
        header = bytes([128 | opcode, 128 | size]) if size < 126 else bytes([128 | opcode, 128 | 126]) + struct.pack('!H', size)
        self.socket.sendall(header + mask + bytes(value ^ mask[index % 4] for index, value in enumerate(payload)))

    def receive(self):
        first, second = self.exact(2)
        assert not second & 128
        size = second & 127
        if size == 126:
            size = struct.unpack('!H', self.exact(2))[0]
        elif size == 127:
            size = struct.unpack('!Q', self.exact(8))[0]
        assert size < 1024 * 1024
        payload = self.exact(size)
        if first & 15 == 9:
            self.send(payload, 10)
            return self.receive()
        assert first & 15 == 1, (first, payload)
        return json.loads(payload)

    def close(self):
        self.socket.close()


provider = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
threading.Thread(target=provider.serve_forever, daemon=True).start()
product = root / 'product' / 'yuan'
product.mkdir(parents=True)
(product / 'lingxi.md').write_text('你是{{agentName}}。')
config = {'home': str(home), 'workspace': str(workspace), 'providers': {'chat': {'protocol': 'openai-completions', 'endpoint': f'http://127.0.0.1:{provider.server_port}/v1', 'auth': {'kind': 'apiKey', 'apiKey': 'synthetic-focus'}}}, 'models': {'chat': {'provider': 'chat', 'model': 'default', 'capabilities': {'tools': True}}, 'title': {'provider': 'chat', 'model': 'second', 'capabilities': {'tools': True}, 'compat': {'reasoning': True, 'contextWindow': 200000}}}}
(root / 'config.json').write_text(json.dumps(config))
log = (root / 'service.log').open('w')
proc = subprocess.Popen([os.environ['R06_SESSION_SERVICE_BIN'], '--config', str(root / 'config.json'), '--bind', '127.0.0.1:0'], env={**os.environ, 'LINGXI_HOME': str(home), 'TMPDIR': str(root), 'LINGXI_PRODUCT_DIR': str(product.parent)}, stdout=subprocess.PIPE, stderr=log, text=True)
ui = None
other_proc = None
other_log = None
trace = []
try:
    url = None
    for line in proc.stdout:
        log.write(line)
        log.flush()
        match = re.search(r'LINGXI_SERVICE_READY addr=(\S+)', line)
        if match:
            url = 'http://' + match[1]
            break
    assert url
    token = json.loads((home / 'lingxi-service/local-token.json').read_text())['token']

    def api(method, path, body=None, status=200, credential=None, target=None):
        request = urllib.request.Request((target or url) + path, data=json.dumps(body).encode() if body is not None else None, method=method, headers={'Authorization': 'Bearer ' + (credential or token), 'Content-Type': 'application/json'})
        try:
            response = urllib.request.urlopen(request, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        raw = response.read()
        data = json.loads(raw) if raw else None
        trace.append({'method': method, 'path': path, 'status': response.status, 'data': ({**data, 'secret': '[omitted]'} if isinstance(data, dict) and 'secret' in data else data)})
        assert response.status == status, (path, response.status, data)
        return data

    for agent in ['focus-a', 'focus-b']:
        api('POST', '/lingxi/v1/agents', {'id': agent, 'name': agent, 'yuan': 'lingxi'})
        api('PUT', f'/lingxi/v1/agents/{agent}/config', {'memory': {'enabled': False}})
    db = sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro', uri=True)
    repository=pathlib.Path(__file__).resolve().parents[2]
    with (root/'ui.log').open('w')as ui_log:
        ui=subprocess.Popen([os.environ.get('R06_NODE_BIN','node'),str(repository/'desktop/src/react/__tests__/session-created-live/launch.mjs')],cwd=repository,env={**os.environ,'R06_SESSION_HOME':str(root),'R06_SESSION_URL':url,'R06_SESSION_TOKEN':token},stdout=ui_log,stderr=subprocess.STDOUT)
        code=ui.wait(timeout=45)
    assert not requests and db.execute('SELECT COUNT(*) FROM model_call_usage').fetchone()[0]==0
    assert code==0,'actual production lifecycle dispatch failed'
    print(root,'PASS',flush=True)

finally:
    if ui and ui.poll() is None:
        ui.terminate()
        ui.wait(timeout=15)
    proc.terminate()
    proc.wait(timeout=15)
    if other_proc:
        other_proc.terminate()
        other_proc.wait(timeout=15)
        other_log.close()
    provider.shutdown()
    log.close()
    (root / 'raw-provider-requests.json').write_text(json.dumps(requests, ensure_ascii=False, indent=2))
