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
    before_count = db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0]
    preferences_path = home / 'user/preferences.json'
    before_preferences = preferences_path.read_bytes() if preferences_path.exists() else None
    invalid = [({'agentId': 'missing'}, 404), ({'agentId': 'focus-b', 'cwd': 'relative'}, 400), ({'agentId': 'focus-b', 'model': ['chat', 'missing']}, 400), ({'agentId': 'focus-b', 'thinkingLevel': 'invalid'}, 400)]
    for body, status in invalid:
        api('POST', '/lingxi/v1/sessions/new', body, status)
        assert db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0] == before_count
        assert (preferences_path.read_bytes() if preferences_path.exists() else None) == before_preferences
    restricted = api('POST', '/lingxi/v1/devices/credentials', {'userId': 'read-only-owner', 'scopes': ['settings.read']}, 201)['secret']
    api('POST', '/lingxi/v1/sessions/new', {'agentId': 'focus-b'}, 403, credential=restricted)
    assert db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0] == before_count
    assert (preferences_path.read_bytes() if preferences_path.exists() else None) == before_preferences
    foreign = api('POST', '/lingxi/v1/devices/credentials', {'userId': 'foreign-owner', 'scopes': ['chat']}, 201)['secret']
    api('PATCH','/lingxi/v1/experiments/subagent.proactive_delegation',{'value':True})
    local_wire = Wire(url, token)
    foreign_wire = Wire(url, foreign)
    data = api('POST', '/lingxi/v1/sessions/new', {'agentId': 'focus-b', 'cwd': str(selected), 'memoryEnabled': False, 'thinkingLevel': 'high', 'model': ['chat', 'second'], 'projectId': 'actual-project'})
    sid = data['sessionId']
    broadcast = local_wire.receive()
    assert broadcast['type'] == 'session_created' and broadcast['sessionId'] == sid and broadcast['sessionPath'] == 'native:' + sid
    assert broadcast['session'] == data and not any(key in broadcast for key in ['runId', 'streamId', 'seq'])
    foreign_wire.socket.settimeout(0.25)
    try:
        unexpected = foreign_wire.receive()
        raise AssertionError(('foreign owner received private creation', unexpected))
    except TimeoutError:
        pass
    finally:
        local_wire.close()
        foreign_wire.close()

    assert data['path'] == 'native:' + sid
    assert db.execute('SELECT proactive_delegation FROM sessions WHERE session_id=?',(sid,)).fetchone()==(1,)
    api('PATCH','/lingxi/v1/experiments/subagent.proactive_delegation',{'value':False})
    assert db.execute('SELECT proactive_delegation FROM sessions WHERE session_id=?',(sid,)).fetchone()==(1,)
    row = db.execute('SELECT agent_id,memory_enabled,authorized_folders_json,project_id FROM sessions WHERE session_id=?', (sid,)).fetchone()
    assert row == ('focus-b', 0, json.dumps([str(selected)], separators=(',', ':')), 'actual-project'), row
    preferences = json.loads((home / 'user/preferences.json').read_text())
    assert preferences['current_agent_id'] == 'focus-b' and preferences['current_session_id'] == sid
    choice = json.loads((home / 'chat-input-settings.json').read_text())['sessions'][sid]
    assert choice['model'] == ['chat', 'second'] and choice['thinkingLevel'] == 'high'
    metadata = json.loads((home / 'lingxi-service/data/session-ui' / (hashlib.sha256(sid.encode()).hexdigest() + '.json')).read_text())
    assert metadata['cwd'] == str(selected) and metadata['sessionId'] == sid
    files = api('GET', f'/lingxi/v1/sessions/{sid}/workspace-files')
    assert files['basePath'] == str(selected) and files['files'][0]['name'] == 'owned.txt'
    api('GET', f'/lingxi/v1/sessions/{sid}/workspace-files?subdir=..', status=400)
    result = api('POST', f'/lingxi/v1/sessions/{sid}/execute', {'input': '读取本会话工作区文件'})
    assert db.execute('SELECT status FROM runs WHERE run_id=?', (result['runId'],)).fetchone() == ('completed',)
    assert len(requests) == 3
    boundary=db.execute('SELECT message_id FROM messages WHERE session_id=? ORDER BY seq DESC LIMIT 1',(sid,)).fetchone()[0]
    for fork_id,enabled in [('experiment-fork-off',False),('experiment-fork-on',True)]:
        api('PATCH','/lingxi/v1/experiments/subagent.proactive_delegation',{'value':enabled})
        api('POST',f'/lingxi/v1/sessions/{sid}/fork',{'newSessionId':fork_id,'boundaryMessageId':boundary})
        assert db.execute('SELECT proactive_delegation FROM sessions WHERE session_id=?',(fork_id,)).fetchone()==(int(enabled),)
        assert db.execute('SELECT proactive_delegation FROM sessions WHERE session_id=?',(sid,)).fetchone()==(1,)
    api('PATCH','/lingxi/v1/experiments/subagent.proactive_delegation',{'value':False})
    assert all('简单任务直接做' in json.dumps(request['messages'],ensure_ascii=False) or 'delegate independent research with access=' in json.dumps(request['messages'],ensure_ascii=False) for request in requests), 'new session lost its frozen delegation hint after global toggle off'
    assert all(request['model'] == 'second' and request.get('reasoning_effort') == 'high' for request in requests), requests
    tools = [message for message in requests[-1]['messages'] if message['role'] == 'tool']
    assert 'SELECTED SESSION WORKSPACE' in json.dumps(tools) and 'WRONG GLOBAL WORKSPACE' not in json.dumps(tools), tools
    run_proofs = [{'tool': 'read', 'runId': result['runId'], 'requestCount': 3}]
    for tool, arguments, marker, forbidden in [('ls', {'path': '.'}, 'selected-only.txt', 'global-only.txt'), ('exec_command', {'cmd': 'pwd'}, str(selected), str(workspace))]:
        active_tool, active_arguments = tool, arguments
        before = len(requests)
        extra = api('POST', f'/lingxi/v1/sessions/{sid}/execute', {'input': '验证本会话目录：' + tool})
        assert db.execute('SELECT status FROM runs WHERE run_id=?', (extra['runId'],)).fetchone() == ('completed',)
        assert len(requests) - before == 3
        tool_results = json.dumps([message for message in requests[-1]['messages'] if message['role'] == 'tool'])
        assert marker in tool_results and forbidden not in tool_results, (tool, tool_results)
        run_proofs.append({'tool': tool, 'runId': extra['runId'], 'requestCount': 3})
    assert len(requests) == 9 and all(request['model'] == 'second' and request.get('reasoning_effort') == 'high' for request in requests)
    usages = db.execute('SELECT COUNT(*) FROM model_call_usage').fetchone()[0]
    assert usages == len(requests)
    for proof in run_proofs:
        ledger = db.execute('SELECT purpose,origin,provider,model,COUNT(*) FROM model_call_usage WHERE run_id=? GROUP BY purpose,origin,provider,model', (proof['runId'],)).fetchall()
        assert ledger == [('chat', 'user', 'chat', 'second', 3)], ledger
        proof['usage'] = ledger

    events = api('GET', '/lingxi/v1/app-events?since=0')['events']
    assert any(event['type'] == 'session-created' and event['payload'].get('sessionId') == sid for event in events)
    assert any(event['type'] == 'session-focus-changed' and event['payload'].get('sessionId') == sid for event in events)
    # 真实下游偏好写失败：已准备资料必须撤回，不能留下假焦点或假成功。
    row_count = db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0]
    old_preferences = preferences_path.read_bytes()
    old_choices = (home / 'chat-input-settings.json').read_bytes()
    metadata_dir = home / 'lingxi-service/data/session-ui'
    old_metadata = {path.name: path.read_bytes() for path in metadata_dir.iterdir()}
    if sys.platform == 'darwin':
        subprocess.run(['/usr/bin/chflags', 'uchg', str(preferences_path)], check=True)
        try:
            failed = api('POST', '/lingxi/v1/sessions/new', {'agentId': 'focus-a', 'cwd': str(workspace)}, 500)
            assert '创建焦点会话失败' in failed['message']
        finally:
            subprocess.run(['/usr/bin/chflags', 'nouchg', str(preferences_path)], check=True)
        assert db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0] == row_count
        assert preferences_path.read_bytes() == old_preferences
        assert json.loads((home / 'chat-input-settings.json').read_text()) == json.loads(old_choices)
        assert {path.name: path.read_bytes() for path in metadata_dir.iterdir()} == old_metadata
        assert len(requests) == usages
    other = api('POST', '/lingxi/v1/sessions/new', {'agentId': 'focus-b', 'memoryEnabled': False})['sessionId']
    api('POST', '/lingxi/v1/sessions/switch', {'sessionId': sid})
    ui_result = None
    if os.environ.get('R06_SESSION_UI') == '1':
        other_home = root / 'other-home'
        other_home.mkdir()
        other_workspace = root / 'other-workspace'
        other_workspace.mkdir()
        other_config = {**config, 'home': str(other_home), 'workspace': str(other_workspace)}
        (root / 'other-config.json').write_text(json.dumps(other_config))
        other_log = (root / 'other-service.log').open('w')
        other_proc = subprocess.Popen([os.environ['R06_SESSION_SERVICE_BIN'], '--config', str(root / 'other-config.json'), '--bind', '127.0.0.1:0'], env={**os.environ, 'LINGXI_HOME': str(other_home), 'TMPDIR': str(root), 'LINGXI_PRODUCT_DIR': str(product.parent)}, stdout=subprocess.PIPE, stderr=other_log, text=True)
        other_url = None
        for line in other_proc.stdout:
            other_log.write(line)
            other_log.flush()
            match = re.search(r'LINGXI_SERVICE_READY addr=(\S+)', line)
            if match:
                other_url = 'http://' + match[1]
                break
        assert other_url
        other_token = json.loads((other_home / 'lingxi-service/local-token.json').read_text())['token']
        api('POST', '/lingxi/v1/agents', {'id': 'focus-b', 'name': '另一个真实服务', 'yuan': 'lingxi'}, credential=other_token, target=other_url)
        api('POST', '/lingxi/v1/sessions', {'sessionId': sid, 'agentId': 'focus-b', 'title': '相同会话ID的另一个来源'}, 201, credential=other_token, target=other_url)
        api('PUT', '/lingxi/v1/input-drafts', {'surface': 'electron', 'sessionId': sid, 'text': '连接B原草稿'}, credential=other_token, target=other_url)
        repository = pathlib.Path(__file__).resolve().parents[2]
        with (root / 'ui.log').open('w') as ui_log:
            ui = subprocess.Popen([os.environ.get('R06_NODE_BIN', 'node'), str(repository / 'desktop/src/react/__tests__/session-focus-live/launch.mjs')], cwd=repository, env={**os.environ, 'R06_SESSION_HOME': str(root), 'R06_SESSION_URL': url, 'R06_SESSION_TOKEN': token, 'R06_SESSION_ID': sid, 'R06_SESSION_OTHER_ID': other, 'R06_SESSION_OTHER_URL': other_url, 'R06_SESSION_OTHER_TOKEN': other_token}, stdout=ui_log, stderr=subprocess.STDOUT)
            assert ui.wait(timeout=85) == 0, 'actual native input failed'
        ui_result = json.loads((root / 'focus-ui-result.json').read_text())
        assert ui_result['status'] == 'PASS' and len(ui_result['checks']) == 6
    # 独立创建共用资料初始化，保持主焦点、默认模型和工作区历史原样。
    detached_preferences = preferences_path.read_bytes()
    detached_agent_config = (home / 'agents/focus-b/config.yaml').read_bytes()
    detached = api('POST', '/lingxi/v1/sessions/new-detached-context', {'agentId': 'focus-b', 'cwd': str(selected), 'memoryEnabled': False, 'recordWorkspaceHistory': False, 'forkedFromSessionId': sid})
    assert detached['path'] == 'native:' + detached['sessionId'] and detached['lineageDepth'] == 1
    assert db.execute('SELECT proactive_delegation FROM sessions WHERE session_id=?',(detached['sessionId'],)).fetchone()==(0,)
    assert db.execute('SELECT proactive_delegation FROM sessions WHERE session_id=?',(sid,)).fetchone()==(1,)
    assert db.execute('SELECT parent_session_id,lineage_depth FROM sessions WHERE session_id=?', (detached['sessionId'],)).fetchone() == (sid, 1)
    assert preferences_path.read_bytes() == detached_preferences
    assert (home / 'agents/focus-b/config.yaml').read_bytes() == detached_agent_config
    detached_events = api('GET', '/lingxi/v1/app-events?since=0')['events']
    assert any(event['type'] == 'session-created' and event['payload'].get('sessionId') == detached['sessionId'] and event['payload'].get('detached') is True for event in detached_events)
    assert not any(event['type'] == 'session-focus-changed' and event['payload'].get('sessionId') == detached['sessionId'] for event in detached_events)
    # 已授权根后来被替换成链接，不可重新授权或产生新的模型费用。
    preserved = root / 'selected-preserved'
    selected.rename(preserved)
    selected.symlink_to(workspace, target_is_directory=True)
    try:
        api('GET', f'/lingxi/v1/sessions/{sid}/workspace-files', status=403)
        refused_run = api('POST', f'/lingxi/v1/sessions/{sid}/execute', {'input': '根替换后不得读取默认目录'})
        assert db.execute('SELECT status FROM runs WHERE run_id=?', (refused_run['runId'],)).fetchone() == ('failed',)
        assert len(requests) == 9 and db.execute('SELECT COUNT(*) FROM model_call_usage').fetchone()[0] == 9
        assert (preserved / 'owned.txt').read_text() == 'SELECTED SESSION WORKSPACE'
    finally:
        selected.unlink()
        preserved.rename(selected)
    (root / 'result.json').write_text(json.dumps({'status': 'PASS', 'trace': trace, 'requests': requests, 'runProofs': run_proofs, 'broadcast': broadcast, 'persistedModelCalls': usages, 'ui': ui_result}, ensure_ascii=False, indent=2))
    print(root, 'PASS', flush=True)
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
