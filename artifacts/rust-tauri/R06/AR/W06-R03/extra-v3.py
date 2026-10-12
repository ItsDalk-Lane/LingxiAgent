import pathlib,tempfile,threading,http.server,subprocess,os,json,re,http.client,hashlib,time,sqlite3
ROOT=pathlib.Path(__file__).resolve().parent
BIN=pathlib.Path(os.environ['W06_BINARY'])
class Stub(http.server.BaseHTTPRequestHandler):
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));self.server.captured.append(body)
  s='data: '+json.dumps({'id':'probe','choices':[{'index':0,'delta':{'role':'assistant','content':'离线'},'finish_reason':None}]})+'\n\ndata: '+json.dumps({'id':'probe','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]})+'\n\ndata: [DONE]\n\n'
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(s.encode())));self.end_headers();self.wfile.write(s.encode())
 def log_message(self,*a):pass
cases=[('upper-prefix-family', 'memory: {enabled: false}\nfuture:\n  v0: 0B10\n  v1: 0O17\n  v2: 0Xff\n  v3: +0B10\n  v4: -0B10\n  v5: +0O17\n  v6: -0O17\n  v7: +0Xff\n  v8: -0Xff\n  v9: 0xFF\n  v10: 0o17\n  v11: 0b10\n  v12: 0b102\n  v13: 0XGG\n  v14: 0b\n  v15: 1:20\n  v16: True\n  v17: TRUE\n  v18: False\n  v19: FALSE\n  v20: Null\n  v21: NULL\n', False), ('tag-scalar-0', 'memory: {enabled: false}\nfuture: !!null wrong\n', None), ('tag-scalar-1', 'memory: {enabled: false}\nfuture: !!null false\n', None), ('tag-scalar-2', 'memory: {enabled: false}\nfuture: !!null 7\n', None), ('tag-scalar-3', 'memory: {enabled: false}\nfuture: !!null "~"\n', False), ('tag-scalar-4', 'memory: {enabled: false}\nfuture: !!int 0B10\n', None), ('tag-scalar-5', 'memory: {enabled: false}\nfuture: !!int 0O17\n', None), ('tag-scalar-6', 'memory: {enabled: false}\nfuture: !!int 0Xff\n', None), ('tag-scalar-7', 'memory: {enabled: false}\nfuture: !!float 0xFF\n', None), ('tag-scalar-8', 'memory: {enabled: false}\nfuture: !!float -0B10\n', None), ('tag-scalar-9', 'memory: {enabled: false}\nfuture: !!float .NaN\n', False), ('tag-scalar-10', 'memory: {enabled: false}\nfuture: !!int 12.0\n', None)]
records=[];start=time.time()
for name,yaml,expected in cases:
 with tempfile.TemporaryDirectory(prefix='lingxi-review-'+name+'-') as temp:
  root=pathlib.Path(temp);home=root/'home';ws=root/'ws';home.mkdir();ws.mkdir()
  stub=http.server.ThreadingHTTPServer(('127.0.0.1',0),Stub);stub.captured=[];thread=threading.Thread(target=stub.serve_forever);thread.start()
  cfg=root/'service.json';cfg.write_text(json.dumps({'home':str(home),'workspace':str(ws),'providers':{'main':{'protocol':'openai-completions','endpoint':f'http://127.0.0.1:{stub.server_port}','auth':{'kind':'apiKey','apiKey':'synthetic-local-probe'}}},'models':{'chat':{'provider':'main','model':'probe','capabilities':{'tools':True}}}}))
  env=os.environ.copy();env.update(LINGXI_HOME=str(home),TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',LINGXI_TEST_GUARD='1',TEST_GUARD_VERBOSE='1')
  case_start=time.time();command=[str(BIN),'--home',str(home),'--config',str(cfg),'--bind','127.0.0.1:0'];stderr=open(ROOT/'logs'/f'probe-{name}-stderr.log','w');child=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=stderr,text=True)
  try:
   ready=child.stdout.readline();assert 'LINGXI_SERVICE_READY' in ready,ready;assert 'home='+str(home.resolve())+' ' in ready;assert 'source=cli' in ready;hostport=re.search(r'addr=([^ ]+)',ready).group(1).strip();host,port=hostport.rsplit(':',1)
   token=json.loads((home/'lingxi-service/local-token.json').read_text())['token'];agent=home/'agents/lingxi';agent.mkdir(parents=True,exist_ok=True);path=agent/'config.yaml';path.write_text(yaml);(agent/'memory').mkdir(exist_ok=True);(agent/'memory/memory.md').write_text('REVIEWER-ONLY-NONCE')
   def req(method,url,data=None):
    conn=http.client.HTTPConnection(host,int(port),timeout=60);conn.request(method,url,json.dumps(data) if data else None,{'Authorization':'Bearer '+token,'Content-Type':'application/json'});r=conn.getresponse();raw=r.read();conn.close();return r.status,json.loads(raw)
   flags=req('GET','/lingxi/v1/agents/lingxi/config-flags');execute=req('POST','/lingxi/v1/sessions/sess_local_alpha/execute',{'input':'回答'});sent=stub.captured[0]['messages'][0]['content'] if stub.captured else None
   db=sqlite3.connect('file:'+str(home/'lingxi-service/data/runs.db')+'?mode=ro',uri=True);terminal=db.execute('SELECT status,terminal_reason FROM runs WHERE run_id=?',(execute[1]['runId'],)).fetchone();db.close()
   ok=(flags[0]==400 and sent is None and terminal[0]=='failed') if expected is None else (flags[0]==200 and flags[1]['memoryEnabled']==expected and sent is not None and ('REVIEWER-ONLY-NONCE' in sent)==expected)
   patch=req('PATCH','/lingxi/v1/agents/lingxi/config',{'memory':{'enabled':True}})
   after=path.read_text()
   record={'before':yaml,'after':after,'patch':patch,'name':name,'yaml':yaml,'expected_memory':expected,'actual_flags':flags,'execute':execute,'terminal':terminal,'provider_request':stub.captured,'matches_expectation':ok,'home':str(home.resolve()),'home_source':'CLI and env both synthetic','config_unchanged_before_patch':sent is not None or expected is None,'config_unchanged':path.read_text()==yaml,'binary_sha256':hashlib.sha256(BIN.read_bytes()).hexdigest(),'ready':ready.strip()};records.append(record);print(name,ok,flags[0],terminal)
  finally:
   record=locals().get('record',{})
   child.terminate();exitcode=child.wait(timeout=30);record.update(child_exit_code=exitcode,pid=child.pid,command=command,start_time=case_start,end_time=time.time(),input_sha256=hashlib.sha256(yaml.encode()).hexdigest());stderr.close();stub.shutdown();thread.join();stub.server_close()
ROOT.joinpath('extra-v3-results.json').write_text(json.dumps({'start':start,'end':time.time(),'binary':str(BIN),'records':records},ensure_ascii=False,indent=2))

for r in records:r["fixture_removed"]=not pathlib.Path(r["home"]).exists();r["guard_armed"]=("mem_limit=8192 MB, time_limit=600 s" in (ROOT/"logs"/("probe-"+r["name"]+"-stderr.log")).read_text())
ROOT.joinpath("extra-v3-results.json").write_text(json.dumps({"start":start,"end":time.time(),"binary":str(BIN),"records":records},ensure_ascii=False,indent=2))
