import pathlib,tempfile,threading,http.server,subprocess,os,json,re,http.client,hashlib,time,sqlite3
ROOT=pathlib.Path(__file__).resolve().parent
BIN=pathlib.Path(os.environ['W06_BINARY'])
class Stub(http.server.BaseHTTPRequestHandler):
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));self.server.captured.append(body)
  s='data: '+json.dumps({'id':'probe','choices':[{'index':0,'delta':{'role':'assistant','content':'离线'},'finish_reason':None}]})+'\n\ndata: '+json.dumps({'id':'probe','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]})+'\n\ndata: [DONE]\n\n'
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(s.encode())));self.end_headers();self.wfile.write(s.encode())
 def log_message(self,*a):pass
cases=[('orphan-block-control', 'memory: {enabled: false}\nskills: {enabled: [shared-review]}\n', False), ('orphan-complex-peer', 'memory: {enabled: false}\nskills: {enabled: [shared-review]}\n', False), ('project-block-control', 'memory: {enabled: false}\nworkspace_context: {discover_project_skills: false}\n', False), ('project-complex-key', 'memory: {enabled: false}\nworkspace_context: {? [discover_project_skills]: false}\n', None), ('project-literal-merge', 'memory: {enabled: false}\nworkspace_context: {"<<": {discover_project_skills: false}}\n', False), ('orphan-literal-peer', 'memory: {enabled: false}\nskills: {enabled: [shared-review]}\n', False)]
records=[];start=time.time()
for name,yaml,expected in cases:
 with tempfile.TemporaryDirectory(prefix='lingxi-review-'+name+'-') as temp:
  root=pathlib.Path(temp);home=root/'home';ws=root/'ws';home.mkdir();ws.mkdir()
  stub=http.server.ThreadingHTTPServer(('127.0.0.1',0),Stub);stub.captured=[];thread=threading.Thread(target=stub.serve_forever);thread.start()
  cfg=root/'service.json';cfg.write_text(json.dumps({'home':str(home),'workspace':str(ws),'providers':{'main':{'protocol':'openai-completions','endpoint':f'http://127.0.0.1:{stub.server_port}','auth':{'kind':'apiKey','apiKey':'synthetic-local-probe'}}},'models':{'chat':{'provider':'main','model':'probe','capabilities':{'tools':True}}}}))
  env=os.environ.copy();env.update(LINGXI_HOME=str(home),TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',LINGXI_TEST_GUARD='1',TEST_GUARD_VERBOSE='1')
  case_start=time.time();command=[str(BIN),'--home',str(home),'--config',str(cfg),'--bind','127.0.0.1:0'];stderr=open(ROOT/'logs'/f'v2-{name}-stderr.log','w');child=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=stderr,text=True)
  try:
   ready=child.stdout.readline();assert 'LINGXI_SERVICE_READY' in ready,ready;assert 'home='+str(home.resolve())+' ' in ready;assert 'source=cli' in ready;hostport=re.search(r'addr=([^ ]+)',ready).group(1).strip();host,port=hostport.rsplit(':',1)
   token=json.loads((home/'lingxi-service/local-token.json').read_text())['token'];agent=home/'agents/lingxi';agent.mkdir(parents=True,exist_ok=True);path=agent/'config.yaml';path.write_text(yaml);(agent/'memory').mkdir(exist_ok=True);(agent/'memory/memory.md').write_text('REVIEWER-ONLY-NONCE')
   package=home/'skills/shared-review';package.mkdir(parents=True);(package/'SKILL.md').write_text('---\nname: shared-review\ndescription: review shared skill\n---\nSynthetic skill body')
   peer=home/'agents/other-review';peer.mkdir();(peer/'config.yaml').write_text('skills:\n  ? [enabled]\n  : [shared-review]\n' if name=='orphan-complex-peer' else ('skills: {"<<": {enabled: [shared-review]}}\n' if name=='orphan-literal-peer' else 'skills: {enabled: [shared-review]}\n'))
   project=ws/'.agents/skills/project-review';project.mkdir(parents=True);(project/'SKILL.md').write_text('---\nname: project-review\ndescription: PROJECT-ONLY-REVIEW-NONCE\n---\nSynthetic workspace material')
   def req(method,url,data=None):
    conn=http.client.HTTPConnection(host,int(port),timeout=60);conn.request(method,url,json.dumps(data) if data else None,{'Authorization':'Bearer '+token,'Content-Type':'application/json'});r=conn.getresponse();raw=r.read();conn.close();return r.status,json.loads(raw)
   cleanup=req('GET','/lingxi/v1/agents/lingxi/skills/cleanup-preview');flags=req('GET','/lingxi/v1/agents/lingxi/config-flags');execute=req('POST','/lingxi/v1/sessions/sess_local_alpha/execute',{'input':'回答'});sent=stub.captured[0]['messages'][0]['content'] if stub.captured else None
   db=sqlite3.connect('file:'+str(home/'lingxi-service/data/runs.db')+'?mode=ro',uri=True);terminal=db.execute('SELECT status,terminal_reason FROM runs WHERE run_id=?',(execute[1]['runId'],)).fetchone();db.close()
   ok=(flags[0]==400 and sent is None and terminal[0]=='failed') if expected is None else (flags[0]==200 and flags[1]['memoryEnabled']==expected and sent is not None and ('REVIEWER-ONLY-NONCE' in sent)==expected)
   expected_preview_status=400 if name=='orphan-complex-peer' or expected is None else 200
   expected_orphans=['shared-review'] if name=='orphan-literal-peer' else []
   actual_orphans=[v['name'] for v in cleanup[1].get('skills',[])]
   preview_ok=cleanup[0]==expected_preview_status and (cleanup[0]!=200 or actual_orphans==expected_orphans)
   expected_project=name=='project-literal-merge' or name.startswith('orphan')
   project_ok=(bool(sent and 'PROJECT-ONLY-REVIEW-NONCE' in sent)==expected_project) if expected is not None else sent is None
   if name=='orphan-complex-peer':ok=flags[0]==200 and sent is None and terminal[0]=='failed';project_ok=sent is None
   record={'preview_matches':preview_ok,'expected_orphans':expected_orphans,'actual_orphans':actual_orphans,'project_matches':project_ok,'expected_project':expected_project,'peer_yaml':(peer/'config.yaml').read_text(),'shared_exists':(package/'SKILL.md').is_file(),'name':name,'yaml':yaml,'expected_memory':expected,'actual_flags':flags,'execute':execute,'terminal':terminal,'cleanup_preview':cleanup,'project_in_wire': bool(sent and 'PROJECT-ONLY-REVIEW-NONCE' in sent),'provider_request':stub.captured,'matches_expectation':ok,'home':str(home.resolve()),'home_source':'CLI and env both synthetic','config_unchanged':path.read_text()==yaml,'binary_sha256':hashlib.sha256(BIN.read_bytes()).hexdigest(),'ready':ready.strip()};records.append(record);print(name,ok,flags[0],terminal)
  finally:
   child.terminate();exitcode=child.wait(timeout=30);record['child_exit_code']=exitcode;record['command']=command;record['start_time']=case_start;record['end_time']=time.time();record['pid']=child.pid;stderr.close();stub.shutdown();thread.join();stub.server_close()
ROOT.joinpath('effects-probe-results-v2.json').write_text(json.dumps({'start':start,'end':time.time(),'binary':str(BIN),'records':records},ensure_ascii=False,indent=2))

for r in records:r['fixture_removed']=not pathlib.Path(r['home']).exists();r['guard_armed']=('mem_limit=8192 MB, time_limit=600 s' in (ROOT/'logs'/('v2-'+r['name']+'-stderr.log')).read_text())
ROOT.joinpath('effects-probe-results-v2.json').write_text(json.dumps({'start':start,'end':time.time(),'binary':str(BIN),'records':records},ensure_ascii=False,indent=2))
