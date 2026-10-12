import os,sys,json,time,sqlite3,hashlib,threading,subprocess,signal,urllib.request,urllib.error
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
base=Path(__file__).parent;root=base/'own.git';out=Path(os.environ['R06_T08_EVAL_OUT']).parent
home=base/'resource-probe-home';home.mkdir();workspace=base/'resource-probe-workspace';workspace.mkdir();wire=[]
class Provider(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  frames=[{'id':'persona-fixture','choices':[{'index':0,'delta':{'role':'assistant','content':'合成检查完成'},'finish_reason':None}]},{'id':'persona-fixture','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]},{'id':'persona-fixture','choices':[],'usage':{'prompt_tokens':40,'completion_tokens':17}}]
  response=''.join('data: '+json.dumps(frame,ensure_ascii=False)+'\n\n' for frame in frames)+'data: [DONE]\n\n'
  wire.append(dict(path=self.path,request=body,responseSse=response,capturedAtUnixMs=int(time.time()*1000)))
  data=response.encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=ThreadingHTTPServer(('127.0.0.1',0),Provider);threading.Thread(target=server.serve_forever,daemon=True).start()
config=base/'resource-probe-service.json';config.write_text(json.dumps({'home':str(home),'workspace':str(workspace),'providers':{'main':{'protocol':'openai-completions','endpoint':'http://127.0.0.1:'+str(server.server_port),'auth':{'kind':'apiKey','apiKey':'sk-synthetic-local-only'}}},'models':{'chat':{'provider':'main','model':'persona-fixture','capabilities':{'tools':True}}}}))
binary=base/'target/debug/lingxi-service';env=dict(os.environ,LINGXI_HOME=str(home));argv=[str(binary),'--home',str(home),'--config',str(config),'--bind','127.0.0.1:0'];proc=subprocess.Popen(argv,env=env,stdout=subprocess.PIPE,stderr=(out/'service.stderr').open('w'),text=True)
report=dict(status='SCOPED_ACTUAL_OBSERVATIONS_NOT_FULL_SEMANTIC_CLOSURE',argv=argv,pid=proc.pid,binarySha256Before=hashlib.sha256(binary.read_bytes()).hexdigest(),effectiveLingxiHome=str(home),osHome=env['HOME'],productDir=env['LINGXI_PRODUCT_DIR'],captured=[])
try:
 ready=proc.stdout.readline().strip();assert ready.startswith('LINGXI_SERVICE_READY '),ready;report['ready']=ready
 addr=next(x[5:] for x in ready.split() if x.startswith('addr='));token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
 agent=home/'agents/lingxi';agent.mkdir(parents=True,exist_ok=True)
 (home/'user').mkdir(exist_ok=True);(home/'user/preferences.json').write_text('{"userName":"合成用户","locale":"zh-CN"}')
 (agent/'config.yaml').write_text('locale: zh-CN\nagent:\n  name: 合成助手\nmemory:\n  enabled: true\n')
 (agent/'identity.md').write_text('PERSONA-OLD-IDENTITY');(agent/'AGENTS.md').write_text('PERSONA-OLD-AGENTS');(agent/'AGENTS.public.md').write_text('PERSONA-OLD-PUBLIC');(agent/'memory').mkdir(exist_ok=True);(agent/'memory/memory.md').write_text('PERSONA-MEMORY-BEFORE')
 def req(method,path,body=None):
  data=None if body is None else json.dumps(body).encode();headers={'Authorization':'Bearer '+token}
  if data is not None:headers['Content-Type']='application/json'
  try:r=urllib.request.urlopen(urllib.request.Request('http://'+addr+path,data,headers,method=method),timeout=30)
  except urllib.error.HTTPError as e:r=e
  payload=r.read();value=json.loads(payload)
  receipt=dict(method=method,path=path,status=r.status,request=body,response=value,capturedAtUnixMs=int(time.time()*1000));report['captured'].append(receipt);return receipt
 def run(session,marker,create=False):
  if create:assert req('POST','/lingxi/v1/sessions',{'sessionId':session,'agentId':'lingxi','title':'独立合成人格检查'})['status']==201
  accepted=req('POST','/lingxi/v1/sessions/'+session+'/execute',{'input':marker});assert accepted['status']==200,accepted
  run_id=accepted['response']['runId'];deadline=time.monotonic()+30
  while time.monotonic()<deadline:
   with sqlite3.connect(home/'lingxi-service/data/runs.db') as db:
    row=db.execute('SELECT status,terminal_reason FROM runs WHERE run_id=?',(run_id,)).fetchone()
   if row and row[0] in ('completed','failed','cancelled'):break
   time.sleep(.01)
  else:raise AssertionError('run did not settle')
  report.setdefault('runs',[]).append(dict(databaseHome=str(home),sessionId=session,runId=run_id,input=marker,status=row[0],terminalReason=row[1]));assert row[0]=='completed',row
 run('persona_same','PERSONA-BASELINE',True)
 for route,content in [('identity','PERSONA-NEW-IDENTITY'),('agents-md','PERSONA-NEW-AGENTS'),('public-agents-md','PERSONA-NEW-PUBLIC')]:
  saved=req('PUT','/lingxi/v1/agents/lingxi/'+route,{'content':content});assert saved['status']==200,saved
  read=req('GET','/lingxi/v1/agents/lingxi/'+route);assert read['status']==200 and content in json.dumps(read['response']),read
 assert req('PATCH','/lingxi/v1/agents/lingxi/config',{'memory':{'enabled':False}})['status']==200
 run('persona_same','PERSONA-NEXT-SAME-SESSION')
 run('persona_fresh','PERSONA-NEXT-FRESH-SESSION',True)
 (workspace/'sentinel.txt').write_text('RESOURCE-SYNTHETIC-BYTES')
 resource_checks=[]
 for kind,resource in [('local-file',{'kind':'local-file','path':str(workspace/'sentinel.txt')}),('mount',{'kind':'mount','mountId':'synthetic_mount','path':'sentinel.txt'}),('url',{'kind':'url','url':'http://127.0.0.1:'+str(server.server_port)+'/synthetic-resource'})]:
  for action in ['stat','read']:
   observation=req('POST','/lingxi/v1/resource-io/'+action,{'resource':resource})
   if kind=='local-file':assert observation['status']==200,observation
   resource_checks.append(dict(providerKind=kind,action=action,observation=observation,fullOriginalSuccessContractMet=observation['status']==200,decision='Actual local-file positive control; mount/url original success remains OPEN if current provider rejects. Observer exit0 means capture succeeded, not original leaf PASS.'))
 report['resourceChecks']=resource_checks
 report['wire']=wire
 checks=[]
 for item in wire:
  messages=item['request']['messages'];marker=next(m['content'] for m in reversed(messages) if m['role']=='user');system='\n'.join(m.get('content','') for m in messages if m['role']=='system')
  checks.append(dict(marker=marker,systemSha256=hashlib.sha256(system.encode()).hexdigest(),identityNewPresent='PERSONA-NEW-IDENTITY' in system,agentsNewPresent='PERSONA-NEW-AGENTS' in system,publicNewPresent='PERSONA-NEW-PUBLIC' in system,memoryBeforePresent='PERSONA-MEMORY-BEFORE' in system,identityOldPresent='PERSONA-OLD-IDENTITY' in system,source='actual next wire system message, not saved-file/route echo'))
 report['nextRunActualReadback']=checks
 with sqlite3.connect(home/'lingxi-service/data/runs.db') as db:
  db.row_factory=sqlite3.Row;report['sql']=[dict(row) for row in db.execute('SELECT * FROM model_call_usage ORDER BY rowid')];report['runsSql']=[dict(row) for row in db.execute('SELECT run_id,status,terminal_reason FROM runs ORDER BY rowid')]
 report['productionFiles']=[dict(path=str(p.relative_to(home)),sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in agent.rglob('*') if p.is_file()]
 report['original28StillOpen']=True;report['decision']='Only measured save/read/next-wire shares. Agent creation/deletion/primary/order/switch/user-avatar/provider persistence/ETag/ticker/restart remain unresolved under original R06 deadline; no R07 waiver.'
 print(json.dumps(resource_checks,ensure_ascii=False))
finally:
 proc.send_signal(signal.SIGTERM);report['serviceExitCode']=proc.wait(timeout=30);server.shutdown();server.server_close();report['binarySha256After']=hashlib.sha256(binary.read_bytes()).hexdigest();report['homeClassification']='LOCAL_ONLY: synthetic credentials/database/config';(out/'resource-production-observations.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
