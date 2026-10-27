# 真服务、真实供应商 HTTP 替身、实际模型工具与编辑器联合验证。
import hashlib
import http.server
import json
import os
import pathlib
import re
import sqlite3
import subprocess
import sys
import threading
import time
import signal
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
        latest_user = next((message for message in reversed(body['messages']) if message['role']=='user'), {})
        if step == 2 and 'MAP-HOLD' in json.dumps(latest_user) and (root/'model-hold').exists():
            (root/'model-hold-reached').write_text('actual provider waiting')
            while (root/'model-hold').exists():time.sleep(0.01)
        frames = [{'choices': [{'index': 0, 'delta': delta, 'finish_reason': None}]}, {'choices': [{'index': 0, 'delta': {}, 'finish_reason': finish}]}, {'choices': [], 'usage': {'prompt_tokens': 20, 'completion_tokens': 4}}]
        payload = (''.join('data: ' + json.dumps(frame) + '\n\n' for frame in frames) + 'data: [DONE]\n\n').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)



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
    first = api('POST', '/lingxi/v1/sessions/new', {'agentId':'focus-b','cwd':str(selected),'memoryEnabled':False,'projectId':'map-project','model':['chat','second'],'thinkingLevel':'high'})
    sid = first['sessionId']
    run_proofs = []
    def execute(session, text, expected_model="second", expected_thinking="high"):
        before = len(requests)
        response = api('POST', f'/lingxi/v1/sessions/{session}/execute', {'input':text})
        assert db.execute('SELECT status FROM runs WHERE run_id=?',(response['runId'],)).fetchone() == ('completed',)
        assert len(requests)-before == 3
        assert all(request['model']==expected_model and request.get('reasoning_effort','off')==expected_thinking for request in requests[before:])
        run_proofs.append({'sessionId':session,'runId':response['runId'],'requests':3,'model':expected_model,'thinking':expected_thinking})
        return response
    execute(sid,'MAP-SOURCE-ONE')
    execute(sid,'MAP-SOURCE-TWO')
    other = api('POST', '/lingxi/v1/sessions/new', {'agentId':'focus-b','cwd':str(selected),'memoryEnabled':False,'projectId':'map-project','model':['chat','second'],'thinkingLevel':'high'})['sessionId']
    execute(other,'MAP-TARGET-ONE')
    turns = api('GET',f'/lingxi/v1/sessions/{sid}/conversation-map/turns')['turns']
    assert len(turns)==2 and turns[0]['question']=='MAP-SOURCE-ONE' and turns[1]['question']=='MAP-SOURCE-TWO'
    assert turns[0]['answerEntryId'] and turns[0]['questionEntryId'] and turns[0]['answer']=='真实焦点会话完成'
    layout = api('GET','/lingxi/v1/conversation-map/layout')
    assert layout=={'version':1,'positions':{},'collapsed':[],'updatedAt':None}
    api('PUT','/lingxi/v1/conversation-map/layout',{'positions':{'keep':{'x':10,'y':20}}})
    api('PUT','/lingxi/v1/conversation-map/layout',{'positions':{'second':{'x':30,'y':40}}})
    merged=api('GET','/lingxi/v1/conversation-map/layout')
    assert merged['positions']=={'keep':{'x':10,'y':20},'second':{'x':30,'y':40}}
    import concurrent.futures
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        list(pool.map(lambda i:api('PUT','/lingxi/v1/conversation-map/layout',{'positions':{'concurrent-'+str(i):{'x':i,'y':i+1}}}),range(16)))
    merged=api('GET','/lingxi/v1/conversation-map/layout')
    assert len(merged['positions'])==18 and all(merged['positions']['concurrent-'+str(i)]=={'x':i,'y':i+1} for i in range(16))
    api('PUT','/lingxi/v1/conversation-map/layout',{'collapsed':['real-collapse']})
    assert api('GET','/lingxi/v1/conversation-map/layout')['collapsed']==['real-collapse']
    api('PUT','/lingxi/v1/conversation-map/layout',{'positions':{},'replacePositions':True,'collapsed':[]})
    assert api('GET','/lingxi/v1/conversation-map/layout')['positions']=={}
    layout_file=home/'conversation-map.json'
    valid_layout=layout_file.read_bytes()
    layout_file.write_text('{persisted corruption')
    degraded_layout=api('GET','/lingxi/v1/conversation-map/layout')
    assert degraded_layout['degraded'] is True and degraded_layout['readError']['code']=='layout_corrupt' and degraded_layout['positions']=={}
    api('PUT','/lingxi/v1/conversation-map/layout',{'positions':{'do-not-overwrite':{'x':1,'y':1}}},500)
    assert layout_file.read_text()=='{persisted corruption'
    layout_file.write_bytes(valid_layout)
    foreign=api('POST','/lingxi/v1/devices/credentials',{'userId':'foreign-map-owner','scopes':['chat']},201)['secret']
    api('GET',f'/lingxi/v1/sessions/{sid}/conversation-map/turns',status=403,credential=foreign)
    api('PUT','/lingxi/v1/conversation-map/layout',{'positions':{'foreign-only':{'x':1,'y':2}}},credential=foreign)
    assert api('GET','/lingxi/v1/conversation-map/layout')['positions']=={}
    assert api('GET','/lingxi/v1/conversation-map/layout',credential=foreign)['positions']=={'foreign-only':{'x':1,'y':2}}
    no_chat=api('POST','/lingxi/v1/devices/credentials',{'userId':'no-chat-map-owner','scopes':['settings.read']},201)['secret']
    api('PUT','/lingxi/v1/conversation-map/layout',{'positions':{}},403,credential=no_chat)
    choice_path=home/'chat-input-settings.json';metadata_dir=home/'lingxi-service/data/session-ui'
    original_choice=json.loads(choice_path.read_text())['sessions'][sid]
    count=db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0]
    api('POST',f'/lingxi/v1/sessions/{sid}/fork',{'newSessionId':'map-invalid-anchor','boundaryMessageId':'missing'},400)
    assert db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0]==count
    assert not (metadata_dir/(hashlib.sha256(b'map-invalid-anchor').hexdigest()+'.json')).exists()
    if sys.platform=='darwin':
        old_choices=choice_path.read_bytes();old_metadata={p.name:p.read_bytes() for p in metadata_dir.iterdir()}
        subprocess.run(['/usr/bin/chflags','uchg',str(choice_path)],check=True)
        try:api('POST',f'/lingxi/v1/sessions/{sid}/fork',{'newSessionId':'map-settings-failure','boundaryMessageId':turns[0]['answerEntryId']},500)
        finally:subprocess.run(['/usr/bin/chflags','nouchg',str(choice_path)],check=True)
        assert choice_path.read_bytes()==old_choices and {p.name:p.read_bytes()for p in metadata_dir.iterdir()}==old_metadata
        assert db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0]==count
    child=api('POST',f'/lingxi/v1/sessions/{sid}/fork',{'newSessionId':'map-child','boundaryMessageId':turns[0]['answerEntryId']})['sessionId']
    assert db.execute('SELECT parent_session_id,memory_enabled,project_id FROM sessions WHERE session_id=?',(child,)).fetchone()==(sid,0,'map-project')
    assert json.loads(choice_path.read_text())['sessions'][child]==original_choice
    assert json.loads((metadata_dir/(hashlib.sha256(child.encode()).hexdigest()+'.json')).read_text())['cwd']==str(selected)
    execute(child,'MAP-CHILD-ACTUAL-WORKSPACE')
    assert 'SELECTED SESSION WORKSPACE' in json.dumps([m for m in requests[-1]['messages']if m['role']=='tool'])
    projected=api('GET','/lingxi/v1/sessions/studio-list')['sessions']
    child_projection=next(row for row in projected if row['sessionId']==child)
    assert child_projection['hasDisplayMessages'] is True and child_projection['forkedFrom']=={'sessionId':sid,'entryId':turns[0]['answerEntryId']}
    default_source='map-default-source'
    api('POST','/lingxi/v1/sessions',{'sessionId':default_source,'agentId':'focus-b','title':'MAP-DEFAULT-SOURCE','memoryEnabled':False},201)
    execute(default_source,'MAP-DEFAULT-SOURCE','default','off')
    default_turn=api('GET',f'/lingxi/v1/sessions/{default_source}/conversation-map/turns')['turns'][0]
    default_child=api('POST',f'/lingxi/v1/sessions/{default_source}/fork',{'newSessionId':'map-default-child','boundaryMessageId':default_turn['answerEntryId']})['sessionId']
    default_choice=json.loads(choice_path.read_text())['sessions'][default_child]
    (root/'default-fork-choice-observed.json').write_text(json.dumps(default_choice))
    assert default_choice=={'model':['chat','default'],'thinkingLevel':'off'},default_choice
    execute(default_child,'MAP-DEFAULT-CHILD','default','off')
    no_workspace='map-no-workspace'
    api('POST','/lingxi/v1/sessions',{'sessionId':no_workspace,'agentId':'focus-b','title':'MAP-NO-WORKSPACE','memoryEnabled':False},201)
    api('POST','/lingxi/v1/models/switch',{'sessionId':no_workspace,'provider':'chat','modelId':'second'})
    api('POST','/lingxi/v1/session-thinking-level',{'sessionPath':no_workspace,'level':'high'})
    execute(no_workspace,'MAP-NO-WORKSPACE')
    ui_result=None
    if os.environ.get('R06_SESSION_UI')=='1':
        other_home=root/'other-home';other_home.mkdir();other_workspace=root/'other-workspace';other_workspace.mkdir()
        other_config={**config,'home':str(other_home),'workspace':str(other_workspace)};(root/'other-config.json').write_text(json.dumps(other_config))
        other_log=(root/'other-service.log').open('w')
        other_proc=subprocess.Popen([os.environ['R06_SESSION_SERVICE_BIN'],'--config',str(root/'other-config.json'),'--bind','127.0.0.1:0'],env={**os.environ,'LINGXI_HOME':str(other_home),'TMPDIR':str(root),'LINGXI_PRODUCT_DIR':str(product.parent)},stdout=subprocess.PIPE,stderr=other_log,text=True)
        other_url=None
        for line in other_proc.stdout:
            other_log.write(line);other_log.flush();match=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
            if match:other_url='http://'+match[1];break
        assert other_url
        other_token=json.loads((other_home/'lingxi-service/local-token.json').read_text())['token']
        repository=pathlib.Path(__file__).resolve().parents[2]
        with (root/'map-ui.log').open('w') as ui_log:
            ui=subprocess.Popen([os.environ.get('R06_NODE_BIN','node'),str(repository/'desktop/src/react/__tests__/session-map-live/launch.mjs')],cwd=repository,env={**os.environ,'R06_SESSION_HOME':str(root),'R06_SESSION_URL':url,'R06_SESSION_TOKEN':token,'R06_SESSION_ID':sid,'R06_SESSION_OTHER_ID':other,'R06_SESSION_CHILD_ID':child,'R06_SESSION_NO_WORKSPACE_ID':no_workspace,'R06_SESSION_SERVICE_PID':str(proc.pid),'R06_SESSION_OTHER_URL':other_url,'R06_SESSION_OTHER_TOKEN':other_token},stdout=ui_log,stderr=subprocess.STDOUT)
            assert ui.wait(timeout=120)==0,'actual map production window failed'
        ui_result=json.loads((root/'map-ui-result.json').read_text());assert ui_result['status']=='PASS'
    usages=db.execute('SELECT COUNT(*) FROM model_call_usage').fetchone()[0]
    assert usages==len(requests)==(36 if ui_result else 21)
    assert sum(r['model']=='default' for r in requests)==6
    assert all(r.get('reasoning_effort','off')==('off' if r['model']=='default' else 'high') for r in requests)
    usage_proofs=db.execute('SELECT run_id,provider,model,purpose,origin,usage_state,input_tokens,output_tokens,outcome,COUNT(*) FROM model_call_usage GROUP BY run_id,provider,model,purpose,origin,usage_state,input_tokens,output_tokens,outcome').fetchall()
    assert len(usage_proofs)==(12 if ui_result else 7)
    assert all(row[1]=='chat' and row[3:9]==('chat','user','reported',20,4,'succeeded') and row[9]==3 for row in usage_proofs)
    assert sum(row[2]=='default'for row in usage_proofs)==2
    assert sum(proof['requests'] for proof in run_proofs)<=len(requests)
    for proof in run_proofs:
        rows=db.execute('SELECT COUNT(*) FROM model_call_usage WHERE run_id=?',(proof['runId'],)).fetchone()[0]
        assert rows==proof['requests'],(proof,rows)
    (root/'result.json').write_text(json.dumps({'status':'PASS','trace':trace,'requests':requests,'runProofs':run_proofs,'persistedModelCalls':usages,'usageProofs':usage_proofs,'ui':ui_result},ensure_ascii=False,indent=2))
    print(root,'PASS',flush=True)

finally:
    if ui and ui.poll() is None:
        ui.terminate()
        ui.wait(timeout=15)
    if hasattr(signal,'SIGCONT'):
        try:os.kill(proc.pid,signal.SIGCONT)
        except ProcessLookupError:pass
    proc.terminate()
    proc.wait(timeout=15)
    if other_proc:
        other_proc.terminate()
        other_proc.wait(timeout=15)
        other_log.close()
    provider.shutdown()
    log.close()
    (root / 'raw-provider-requests.json').write_text(json.dumps(requests, ensure_ascii=False, indent=2))
