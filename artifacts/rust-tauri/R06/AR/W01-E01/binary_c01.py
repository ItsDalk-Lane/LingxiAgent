import json,os,time,tempfile,threading,subprocess,hashlib,sqlite3,signal,re,urllib.request,http.server,sys
from pathlib import Path
OUT=Path(__file__).resolve().parent;ROOT=OUT.parents[4];BIN=ROOT/'rust/target/debug/lingxi-service'
base=Path(os.environ['LINGXI_HOME']);home=base/'home';ws=base/'workspace';home.mkdir();ws.mkdir();nonce='W01-RUNTIME-'+os.urandom(16).hex();wire=[]
class Handler(http.server.BaseHTTPRequestHandler):
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));wire.append(body)
  msgs=body['messages'];answer='ACK '+nonce if len(wire)==1 else 'followup'
  parts=[{'choices':[{'index':0,'delta':{'role':'assistant','content':answer},'finish_reason':None}]},{'choices':[{'index':0,'delta':{},'finish_reason':'stop'}]},{'choices':[],'usage':{'prompt_tokens':10,'completion_tokens':3}}]
  data=(''.join('data: '+json.dumps(x)+'\n\n' for x in parts)+'data: [DONE]\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
 def log_message(self,*args):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
config={'home':str(home),'workspace':str(ws),'providers':{'fixture':{'endpoint':f'http://127.0.0.1:{server.server_port}/v1','protocol':'openai-completions','auth':{'kind':'none'}}},'models':{'chat':{'provider':'fixture','model':'fixture-model','capabilities':{'tools':True},'compat':{'contextWindow':1000000}}}}
path=base/'config.json';path.write_text(json.dumps(config));cmd=[str(BIN),'--home',str(home),'--config',str(path),'--bind','127.0.0.1:0'];before=hashlib.sha256(BIN.read_bytes()).hexdigest();p=subprocess.Popen(cmd,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True);stderr=[];err=threading.Thread(target=lambda:stderr.extend(p.stderr.readlines()));err.start();record={}
try:
 ready=p.stdout.readline().strip();assert ready.startswith('LINGXI_SERVICE_READY '),ready;addr=re.search(r'addr=([^ ]+)',ready).group(1);token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
 def submit(text):
  req=urllib.request.Request('http://'+addr+'/lingxi/v1/sessions/sess_local_alpha/execute',data=json.dumps({'input':text}).encode(),headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},method='POST')
  with urllib.request.urlopen(req,timeout=30) as res:return json.load(res)
 first='记住合成标记 '+nonce;second='刚才的合成标记是什么？';a=submit(first);assert len(wire)==1;assert sum(x.get('content')==first for x in wire[0]['messages'])==1
 record['positive']='PASS_FIRST_RUN';b=submit(second);assert len(wire)==2
 record.update(nonce=nonce,input_first=first,input_second=second,ready=ready,wire=wire,run_first=a,run_second=b,db=str(home/'lingxi-service/data/runs.db'))
 db=sqlite3.connect('file:'+record['db']+'?mode=ro',uri=True);record['messages']=db.execute('SELECT message_id,role,content_json,entry_type FROM messages ORDER BY seq').fetchall();db.close()
 assert sum(x.get('content')==first and x.get('role')=='user' for x in wire[1]['messages'])==1,'missing first user turn in actual second wire'
 assert sum(x.get('content')=='ACK '+nonce and x.get('role')=='assistant' for x in wire[1]['messages'])==1
 assert sum(x.get('content')==second and x.get('role')=='user' for x in wire[1]['messages'])==1
 record['status']='PASS'
except Exception as e:record.update(status='FAIL',error=repr(e));raise
finally:
 p.send_signal(signal.SIGTERM)
 try:code=p.wait(timeout=20)
 except subprocess.TimeoutExpired:p.kill();code=p.wait()
 server.shutdown();server.server_close();thread.join();err.join();record.update(command=cmd,binary_before=before,binary_after=hashlib.sha256(BIN.read_bytes()).hexdigest(),service_exit=code,child_reaped=p.poll() is not None,stderr=stderr)
 (OUT/(os.environ.get('W01_CASE','c01')+'-observation.json')).write_text(json.dumps(record,ensure_ascii=False,indent=2));print(json.dumps({'status':record['status'],'positive':record.get('positive'),'service_exit':code}))
