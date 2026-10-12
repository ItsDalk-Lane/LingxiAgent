import pathlib,os,json,hashlib,threading,time,subprocess,signal,urllib.request,sqlite3,concurrent.futures
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
BASE=pathlib.Path('/private/tmp/lingxi-w15-r01-runtime');OUT=pathlib.Path(os.environ['R06_T08_EVAL_OUT']).parent;BIN=BASE/'target/debug/lingxi-service';wire=[]
class Stub(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));response='data: '+json.dumps({'id':'r01-home-collision','choices':[{'index':0,'delta':{'role':'assistant','content':'合成结果'},'finish_reason':None}]})+'\n\ndata: '+json.dumps({'id':'r01-home-collision','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]})+'\n\ndata: '+json.dumps({'choices':[],'usage':{'prompt_tokens':40,'completion_tokens':17}})+'\n\ndata: [DONE]\n\n';wire.append({'endpoint':self.server.server_port,'request':body,'response':response});data=response.encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
def request(world,method,path,body=None):
 data=json.dumps(body).encode() if body is not None else None;headers={'Authorization':'Bearer '+world['token']}
 if data is not None:headers['Content-Type']='application/json'
 with urllib.request.urlopen(urllib.request.Request('http://'+world['addr']+path,data,headers,method=method),timeout=30) as response:return response.status,json.loads(response.read())
worlds=[];attempts=[];collision=None;report={'binaryBefore':hashlib.sha256(BIN.read_bytes()).hexdigest(),'kind':'REAL_SEPARATE_PRODUCTION_HOME_RUN_ID_COLLISION_CONTROL','worlds':[]}
try:
 for i in range(2):
  home=BASE/(OUT.name+'-home-'+str(i));home.mkdir();workspace=BASE/(OUT.name+'-workspace-'+str(i));workspace.mkdir();stub=ThreadingHTTPServer(('127.0.0.1',0),Stub);threading.Thread(target=stub.serve_forever,daemon=True).start();config=BASE/('r01-collision-config-'+str(i)+'.json');config.write_text(json.dumps({'home':str(home),'workspace':str(workspace),'providers':{'main':{'protocol':'openai-completions','endpoint':'http://127.0.0.1:'+str(stub.server_port),'auth':{'kind':'apiKey','apiKey':'sk-synthetic-r01'}}},'models':{'chat':{'provider':'main','model':'r01-collision','capabilities':{'tools':True}}}}));env=dict(os.environ,LINGXI_HOME=str(home));p=subprocess.Popen([str(BIN),'--home',str(home),'--config',str(config),'--bind','127.0.0.1:0'],env=env,stdout=subprocess.PIPE,stderr=(OUT/('service-'+str(i)+'.stderr')).open('w'),text=True);ready=p.stdout.readline().strip();assert ready.startswith('LINGXI_SERVICE_READY '),ready;addr=next(x[5:] for x in ready.split() if x.startswith('addr='));token=json.loads((home/'lingxi-service/local-token.json').read_text())['token'];w={'home':home,'proc':p,'stub':stub,'addr':addr,'token':token};worlds.append(w);report['worlds'].append({'home':str(home),'pid':p.pid,'ready':ready,'providerPort':stub.server_port,'actualLingxiHome':str(home),'osHome':env['HOME']});agent=home/'agents/lingxi';agent.mkdir(parents=True,exist_ok=True);(agent/'config.yaml').write_text('agent:\n  name: 合成助手\nmemory:\n  enabled: false\n');assert request(w,'POST','/lingxi/v1/sessions',{'sessionId':'same_session','agentId':'lingxi','title':'重号控制'})[0]==201
 with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
  for n in range(64):
   barrier=threading.Barrier(2)
   def run(i):
    barrier.wait();status,value=request(worlds[i],'POST','/lingxi/v1/sessions/same_session/execute',{'input':'R01-COLLISION-'+str(n)+'-HOME-'+str(i)});assert status==200;return value['runId']
   futures=[pool.submit(run,i) for i in range(2)];ids=[f.result() for f in futures];row={'round':n,'runIds':ids,'sameId':ids[0]==ids[1]};attempts.append(row)
   for i,w in enumerate(worlds):
    deadline=time.monotonic()+30
    while time.monotonic()<deadline:
     with sqlite3.connect(w['home']/'lingxi-service/data/runs.db') as db:r=db.execute('SELECT status FROM runs WHERE run_id=?',(ids[i],)).fetchone()
     if r and r[0] in ('completed','failed','cancelled'):break
     time.sleep(.005)
    assert r==('completed',),r
   if row['sameId']:collision=row;break
 report.update(attempts=attempts,collision=collision,wire=wire,sql=[])
 for w in worlds:
  with sqlite3.connect('file:'+str(w['home']/'lingxi-service/data/runs.db')+'?mode=ro',uri=True) as db:
   db.row_factory=sqlite3.Row;report['sql'].append({'home':str(w['home']),'runs':[dict(x) for x in db.execute('SELECT * FROM runs ORDER BY rowid')],'usage':[dict(x) for x in db.execute('SELECT * FROM model_call_usage ORDER BY rowid')]})
 report['assertedHomeRunIdentity']='Each accepted run bound to its database home; two databases were never grouped by runId alone';report['collisionStatus']='PASS_REAL_COLLISION_OBSERVED' if collision else 'NOT_OBSERVED_AFTER_BOUNDED_64_ATTEMPTS'
 print(json.dumps({'rounds':len(attempts),'collision':collision,'homeCount':len(worlds),'usageRows':[len(x['usage']) for x in report['sql']]}))
finally:
 for w in worlds:
  w['proc'].send_signal(signal.SIGTERM);code=w['proc'].wait(timeout=30);report['worlds'][worlds.index(w)]['serviceExitCode']=code;w['stub'].shutdown();w['stub'].server_close()
 report['binaryAfter']=hashlib.sha256(BIN.read_bytes()).hexdigest();(OUT/'collision.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
