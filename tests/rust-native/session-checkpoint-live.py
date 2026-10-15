# 真实CLI、模型供应商替身、批准服务、存储和文件恢复联合实证。
import urllib.parse, signal
import json,pathlib,threading,http.server,socketserver,subprocess,urllib.request,urllib.error,time,re,sqlite3,hashlib,os
root=pathlib.Path(os.environ['R06_SESSION_EVIDENCE']);root.mkdir(parents=True,exist_ok=True);root=root.resolve();home=root/'home';home.mkdir(exist_ok=True)
state={'chat':[],'embeddings':[],'rerank':[],'step':0,'tool':'knowledge_search','args':{},'serial':0};lock=threading.Lock()
def content(m):
 c=m.get('content','');return c if isinstance(c,str)else ''.join(v.get('text','')for v in c or [])
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  b=json.loads(self.rfile.read(int(self.headers['Content-Length'])));path=self.path
  if path.endswith('/embeddings'):
   with lock:state['embeddings'].append(b)
   data=[{'index':i,'embedding':([0.,1.,0.,0.]if'SELECTED'in s else[1.,0.,0.,0.])}for i,s in enumerate(b['input'])];payload=json.dumps({'data':data,'usage':{'prompt_tokens':5,'total_tokens':5}}).encode();ctype='application/json'
  elif path.endswith('/rerank'):
   state['rerank'].append(b);payload=b'{"results":[]}';ctype='application/json'
  else:
   with lock:step=state['step'];state['step']+=1;state['chat'].append(b);tool=state['tool'];args=state['args'].copy()
   sys='\n'.join(content(m)for m in b['messages']if m['role']=='system')
   if step<2:
    if step==0:name='mcp_describe_tool';arg={'name':tool};ident='describe'
    else:name='mcp_call';arg={'tool':tool,'arguments':args};ident='actual-probe-'+str(state['serial'])
    delta={'role':'assistant','tool_calls':[{'index':0,'id':ident,'type':'function','function':{'name':name,'arguments':json.dumps(arg)}}]};finish='tool_calls'
   else:
    if state.get('final_gate'):state['final_gate'].wait(timeout=20)
    delta={'role':'assistant','content':'实际工具调用结束'};finish='stop'
   frames=[{'id':'independent','choices':[{'index':0,'delta':delta,'finish_reason':None}]},{'id':'independent','choices':[{'index':0,'delta':{},'finish_reason':finish}]},{'choices':[],'usage':{'prompt_tokens':20,'completion_tokens':4}}]
   payload=(''.join('data: '+json.dumps(x)+'\n\n'for x in frames)+'data: [DONE]\n\n').encode();ctype='text/event-stream'
  self.send_response(200);self.send_header('Content-Type',ctype);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start();endpoint=f'http://127.0.0.1:{server.server_port}/v1'
product=root/'product/yuan';product.mkdir(parents=True,exist_ok=True);(product/'lingxi.md').write_text('你是{{agentName}}。')
cfg={'home':str(home),'workspace':str(root/'workspace'),'providers':{'chat':{'protocol':'openai-completions','endpoint':endpoint,'auth':{'kind':'apiKey','apiKey':'synthetic-independent'}},'embed':{'protocol':'openai-embeddings','endpoint':endpoint,'auth':{'kind':'apiKey','apiKey':'synthetic-independent'}}},'models':{'chat':{'provider':'chat','model':'chat','capabilities':{'tools':True},'compat':{'contextWindow':200000}},'embedding':{'provider':'embed','model':'embedding'}}};(root/'service.json').write_text(json.dumps(cfg))
(root/'workspace').mkdir()
log=open(root/'service.log','w');proc=subprocess.Popen([os.environ['R06_SESSION_SERVICE_BIN'],'--config',str(root/'service.json'),'--bind','127.0.0.1:0'],stdout=subprocess.PIPE,stderr=log,text=True,env={**os.environ,'LINGXI_HOME':str(home),'TMPDIR':str(root),'LINGXI_PRODUCT_DIR':str(product.parent)})
url=None
for line in proc.stdout:
 log.write(line);log.flush();m=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
 if m:url='http://'+m[1];break
assert url,'actual service did not start';token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
trace=[]
def api(method,path,body=None,status=200):
 req=urllib.request.Request(url+path,data=json.dumps(body).encode()if body is not None else None,method=method,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
 try:r=urllib.request.urlopen(req,timeout=30)
 except urllib.error.HTTPError as e:r=e
 data=r.read();j=json.loads(data)if data else None;trace.append({'method':method,'path':path,'status':r.status,'result':j});assert r.status==status,(method,path,r.status,j);return j

results=[]
ui=None
ui_log=None
other_proc=None
other_log=None
try:
 api('POST','/lingxi/v1/agents',{'id':'lingxi','name':'会话实证','yuan':'lingxi'})
 api('PUT','/lingxi/v1/agents/lingxi/config',{'memory':{'enabled':False}})
 work=root/'workspace';first=work/'tracked.txt';first.write_text('v1');outside=work/'external.txt';outside.write_text('初始外部文件')
 for sid,mode in [('tool-a','operate'),('tool-b','operate'),('readonly','read_only')]:api('POST','/lingxi/v1/sessions',{'sessionId':sid,'agentId':'lingxi','title':sid,'permissionMode':mode,'authorizedFolders':[str(work)]},201)
 db=sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro',uri=True)
 requests={}
 def begin(tool,args,sid='tool-a',hold=False):
  state.update(step=0,tool=tool,args=args,serial=state['serial']+1,final_gate=threading.Event()if hold else None);before=len(state['chat']);seen={r[0]for r in db.execute('SELECT run_id FROM runs WHERE session_id=?',(sid,))};reply={}
  def request():
   try:reply['response']=api('POST',f'/lingxi/v1/sessions/{sid}/execute',{'input':'实际模型工具 '+tool})
   except BaseException as e:reply['error']=str(e)
  thread=threading.Thread(target=request);thread.start()
  for _ in range(500):
   fresh=[r[0]for r in db.execute('SELECT run_id FROM runs WHERE session_id=?',(sid,))if r[0]not in seen]
   if fresh:
    assert len(fresh)==1;requests[fresh[0]]=(thread,reply);return fresh[0],before
   time.sleep(.02)
  raise AssertionError(('no actual run',reply))
 def wait_run(run):
  for _ in range(600):
   row=db.execute('SELECT status,terminal_reason FROM runs WHERE run_id=?',(run,)).fetchone()
   if row and row[0]in['completed','failed','cancelled']:
    assert row[0]=='completed',row;thread,reply=requests[run];thread.join(timeout=2);assert 'error'not in reply,reply;assert reply['response']['runId']==run;return
   time.sleep(.025)
  raise AssertionError(('run did not settle',run,row))
 def toolrun(tool,args,sid='tool-a'):
  run,before=begin(tool,args,sid);wait_run(run);chat=state['chat'][before:];frozen=api('GET',f'/lingxi/v1/sessions/{sid}/prompt-snapshot');assert frozen['runId']==run;last=chat[-1];system='\n'.join(content(m)for m in last['messages']if m['role']=='system');assert frozen['promptSnapshot']['systemPrompt']==system;assert frozen['toolNames']==[t['function']['name']for t in last['tools']];history=api('GET',f'/lingxi/v1/sessions/{sid}/history?all=1');calls=[i for i in history['items']if i['kind']=='tool_call'and i['target'] in ['tool:first-party:'+tool,'tool:first-party:mcp_call'] and i['run'].get('runId')==run];refusal='\n'.join(content(m)for m in chat[-1]['messages']if m['role']=='tool');assert len(calls)==1 or(len(calls)==0 and 'ACTION_BLOCKED_BY_READ_ONLY' in refusal),calls;result={'run':run,'tool':tool,'args':args,'call':calls[0]if calls else None,'refusalText':refusal,'chatCount':len(chat)};results.append(result);return result
 created=toolrun('checkpoint',{'action':'create','name':'safe','snapshot':True});assert created['call']['status']=='success',created
 assert 'workspace snapshot degraded' not in str(created), created
 assert db.execute("SELECT COUNT(*) FROM checkpoint_file_versions WHERE checkpoint_id=?",('tool-a:safe',)).fetchone()[0]==2
 cp=api('GET','/lingxi/v1/sessions/tool-a/checkpoints');assert len(cp['checkpoints'])==1 and cp['checkpoints'][0]['name']=='safe';assert api('GET','/lingxi/v1/sessions/tool-b/checkpoints')['checkpoints']==[]
 extra=toolrun('checkpoint',{'action':'create','name':'drop-me','snapshot':False});assert extra['call']['status']=='success'
 removed=toolrun('checkpoint',{'action':'drop','name':'drop-me'});assert removed['call']['status']=='success';assert api('GET','/lingxi/v1/sessions/tool-a/checkpoints')==cp
 listed=toolrun('checkpoint',{'action':'list'});assert listed['call']['status']=='success';assert'safe'in json.dumps(listed)
 invalid=toolrun('checkpoint',{'action':'create','name':'   ','snapshot':True});assert invalid['call']['status']=='failed';assert api('GET','/lingxi/v1/sessions/tool-a/checkpoints')==cp
 wrong=toolrun('checkpoint',{'action':'drop','name':'missing'});assert wrong['call']['status']=='failed';assert api('GET','/lingxi/v1/sessions/tool-a/checkpoints')==cp
 readonly=toolrun('checkpoint',{'action':'list'},'readonly');assert readonly['call']['status']=='success',readonly
 blocked=toolrun('checkpoint',{'action':'create','name':'denied','snapshot':True},'readonly');assert blocked['call'] is None and 'ACTION_BLOCKED_BY_READ_ONLY' in blocked['refusalText'],blocked;assert api('GET','/lingxi/v1/sessions/readonly/checkpoints')['checkpoints']==[]
 written=toolrun('write',{'path':str(first),'content':'v2'});assert written['call']['status']=='success',written;assert first.read_text()=='v2';outside.write_text('用户外部改动')
 # 按钮来自生产 React 确认面板，实际请求直接使用同一真实服务。
 if os.environ.get('R06_SESSION_UI')=='1':
  other_home=root/'other-home';other_home.mkdir();other_cfg={**cfg,'home':str(other_home),'workspace':str(root/'other-workspace')};pathlib.Path(other_cfg['workspace']).mkdir();(root/'other-service.json').write_text(json.dumps(other_cfg));other_log=open(root/'other-service.log','w')
  other_proc=subprocess.Popen([os.environ['R06_SESSION_SERVICE_BIN'],'--config',str(root/'other-service.json'),'--bind','127.0.0.1:0'],stdout=subprocess.PIPE,stderr=other_log,text=True,env={**os.environ,'LINGXI_HOME':str(other_home),'TMPDIR':str(root),'LINGXI_PRODUCT_DIR':str(product.parent)})
  other_url=None
  for line in other_proc.stdout:
   other_log.write(line);other_log.flush();m=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
   if m:other_url='http://'+m[1];break
  assert other_url and other_url!=url
  other_token=json.loads((other_home/'lingxi-service/local-token.json').read_text())['token']
  for route,body in [('/lingxi/v1/agents',{'id':'lingxi','name':'另一处同名助手','yuan':'lingxi'}),('/lingxi/v1/sessions',{'sessionId':'tool-a','agentId':'lingxi','title':'另一处同ID会话'})]:
   request=urllib.request.Request(other_url+route,data=json.dumps(body).encode(),method='POST',headers={'Authorization':'Bearer '+other_token,'Content-Type':'application/json'})
   with urllib.request.urlopen(request)as response:assert response.status in [200,201]
  repo=pathlib.Path(__file__).resolve().parents[2];ui_log=open(root/'review-ui-child.log','w')
  ui=subprocess.Popen([os.environ.get('R06_NODE_BIN','node'),str(repo/'desktop/src/react/__tests__/session-review-live/launch.mjs')],stdout=ui_log,stderr=ui_log,start_new_session=True,env={**os.environ,'R06_SESSION_HOME':str(root),'R06_SESSION_URL':url,'R06_SESSION_TOKEN':token,'R06_SESSION_OTHER_URL':other_url,'R06_SESSION_OTHER_TOKEN':other_token})
 def wait_ui(mark):
  for _ in range(2000):
   if (root/('review-ui-'+mark)).exists():return
   if ui.poll()is not None:raise AssertionError(('UI exited early',ui.returncode,(root/'review-ui-child.log').read_text()))
   time.sleep(.025)
  raise AssertionError(('UI mark missing',mark,(root/'review-ui-child.log').read_text()))
 if ui:wait_ui('ready')
 # 查看用户确认：risk_accepted不能绕过。拒绝保持确认时的真实分支与两份文件。
 run,_=begin('rewind',{'checkpoint':'safe','restoreFiles':True,'risk_accepted':True})
 for _ in range(400):
  pending=api('GET','/lingxi/v1/sessions/tool-a/approvals')['approvals']
  if pending:break
  time.sleep(.025)
 assert len(pending)==1,pending;approval=pending[0];assert'tracked.txt'in approval['summary'];branch=api('GET','/lingxi/v1/sessions/tool-a/branch');assert first.read_text()=='v2'and outside.read_text()=='用户外部改动'
 if ui:(root/'review-ui-reject-ready').write_text('actual branch verified before user rejection')
 (wait_ui('rejected') if ui else api('POST','/lingxi/v1/sessions/tool-a/approvals/'+urllib.parse.quote(approval['approvalId'],safe=''),{'action':'rejected'}));wait_run(run);branch_after=api('GET','/lingxi/v1/sessions/tool-a/branch');assert all(m in branch_after['messages']for m in branch['messages']);assert first.read_text()=='v2'and outside.read_text()=='用户外部改动';assert api('GET','/lingxi/v1/sessions/tool-a/deferred-rewind')['deferredRewind']is None
 # 确认后保持模型本轮继续占门，必须只排队；放行真正结束后才回退。
 api('PUT','/lingxi/v1/sessions/workspace-rollback',{'enabled':True})
 run,_=begin('rewind',{'checkpoint':'safe','restoreFiles':True},hold=True)
 for _ in range(400):
  pending=api('GET','/lingxi/v1/sessions/tool-a/approvals')['approvals']
  if pending:break
  time.sleep(.025)
 assert len(pending)==1;approval=pending[0]
 if ui:(root/'review-ui-confirm-ready').write_text('actual new approval verified')
 (wait_ui('confirmed') if ui else api('POST','/lingxi/v1/sessions/tool-a/approvals/'+urllib.parse.quote(approval['approvalId'],safe=''),{'action':'confirmed'}))
 for _ in range(400):
  deferred=api('GET','/lingxi/v1/sessions/tool-a/deferred-rewind')['deferredRewind']
  if deferred:break
  time.sleep(.025)
 assert deferred['status']=='scheduled',deferred;busy_branch=api('GET','/lingxi/v1/sessions/tool-a/branch');assert any(m['messageId']=='user:'+run for m in busy_branch['messages']);assert first.read_text()=='v2'
 if ui:wait_ui('scheduled')
 state['final_gate'].set();wait_run(run)
 for _ in range(400):
  deferred=api('GET','/lingxi/v1/sessions/tool-a/deferred-rewind')['deferredRewind']
  if deferred['status']!='scheduled':break
  time.sleep(.025)
 assert deferred['status']=='completed',deferred;assert first.read_text()=='v1'and outside.read_text()=='用户外部改动';assert api('GET','/lingxi/v1/sessions/tool-b/checkpoints')['checkpoints']==[]
 actual_branch=api('GET','/lingxi/v1/sessions/tool-a/branch');assert [m['messageId']for m in actual_branch['messages']]==[deferred['receipt']['resetMarkerId']],actual_branch
 assert deferred['receipt']['restored']==[str(first)]and deferred['receipt']['conflicted']==[str(outside)]
 assert deferred['receipt']['turnInputMessageId']==cp['checkpoints'][0]['targetMessageId']
 if ui:
  wait_ui('completed')
  isolated_run,_=begin('rewind',{'checkpoint':'safe','restoreFiles':False})
  wait_ui('isolated')
  pending=api('GET','/lingxi/v1/sessions/tool-a/approvals')['approvals'];assert len(pending)==1
  api('POST','/lingxi/v1/sessions/tool-a/approvals/'+urllib.parse.quote(pending[0]['approvalId'],safe=''),{'action':'rejected'});wait_run(isolated_run)
  anchor_b=toolrun('checkpoint',{'action':'create','name':'safe','snapshot':False},'tool-b');assert anchor_b['call']['status']=='success'
  delayed_a,_=begin('rewind',{'checkpoint':'safe','restoreFiles':False})
  for _ in range(500):
   pending=api('GET','/lingxi/v1/sessions/tool-a/approvals')['approvals']
   if pending:break
   time.sleep(.02)
  assert len(pending)==1;(root/'review-ui-delay-a-ready').write_text(json.dumps(pending[0]));wait_ui('delay-a-held');wait_run(delayed_a)
  delayed_b,_=begin('rewind',{'checkpoint':'safe','restoreFiles':False},'tool-b')
  for _ in range(500):
   pending=api('GET','/lingxi/v1/sessions/tool-b/approvals')['approvals']
   if pending:break
   time.sleep(.02)
  assert len(pending)==1;(root/'review-ui-delay-b-ready').write_text(json.dumps(pending[0]));wait_ui('session-delay-isolated');wait_run(delayed_b)
  assert ui.wait(timeout=15)==0
  proof=json.loads((root/'review-ui-result.json').read_text());assert proof['status']=='PASS'and len(proof['checks'])==8;assert proof['otherServicePosts']==[];ui_log.close()
  other_db=sqlite3.connect(f'file:{other_home}/lingxi-service/data/runs.db?mode=ro',uri=True);assert other_db.execute('SELECT COUNT(*) FROM runs').fetchone()[0]==0;other_db.close()
 usages=db.execute('SELECT COUNT(*) FROM model_call_usage').fetchone()[0];assert usages==len(state['chat']),(usages,len(state['chat']))
 (root/'result.json').write_text(json.dumps({'status':'PASS','results':results,'rejectedApproval':approval,'deferred':deferred,'trace':trace,'chatCount':len(state['chat']),'embeddingCount':len(state['embeddings']),'persistedModelCalls':usages},ensure_ascii=False,indent=2));print(root,'PASS',flush=True)
finally:
 if ui and ui.poll()is None:os.killpg(ui.pid,signal.SIGTERM);ui.wait(timeout=15)
 if other_proc:other_proc.terminate();other_proc.wait(timeout=15);other_log.close()
 if state.get('final_gate'):state['final_gate'].set()
 proc.terminate();proc.wait(timeout=15);server.shutdown();log.close();(root/'raw-provider-requests.json').write_text(json.dumps({k:state[k]for k in['chat','embeddings','rerank']},ensure_ascii=False,indent=2))
