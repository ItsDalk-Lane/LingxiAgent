import http.server, threading, subprocess, pathlib, tempfile, time, json, urllib.request, signal, select, sqlite3
ROOT=pathlib.Path('/workspace/scratch/9b4397d87a13/audit/closed_loop');BIN=pathlib.Path('/workspace/scratch/9b4397d87a13/audit/runtime/target/debug/lingxi-service')
def probe(name,calls):
 run=pathlib.Path(tempfile.mkdtemp(prefix=name+'-',dir=ROOT));home=run/'home';ws=run/'workspace';home.mkdir();ws.mkdir();requests=[]
 class H(http.server.BaseHTTPRequestHandler):
  protocol_version='HTTP/1.1'
  def do_POST(self):
   body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
   if len(requests)==1:
    wire_calls=[{'index':i,'id':c[0],'type':'function','function':{'name':c[1],'arguments':json.dumps(c[2])}} for i,c in enumerate(calls)]
    parts=[{'id':'probe','choices':[{'index':0,'finish_reason':None,'delta':{'role':'assistant','tool_calls':wire_calls}}]},{'id':'probe','choices':[{'index':0,'delta':{},'finish_reason':'tool_calls'}]}]
   else:parts=[{'id':'probe','choices':[{'index':0,'finish_reason':None,'delta':{'role':'assistant','content':'DONE'}}]},{'id':'probe','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]}]
   data=(''.join('data: '+json.dumps(x)+'\n\n' for x in parts)+'data: [DONE]\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.send_header('Connection','close');self.end_headers();self.wfile.write(data)
  def log_message(self,*a):pass
 server=http.server.ThreadingHTTPServer(('127.0.0.1',0),H);threading.Thread(target=server.serve_forever,daemon=True).start()
 cfg=run/'config.json';cfg.write_text(json.dumps({'home':str(home),'workspace':str(ws),'providers':{'main':{'protocol':'openai-completions','endpoint':f'http://127.0.0.1:{server.server_port}/v1','auth':{'kind':'none'}}},'models':{'chat':{'provider':'main','model':'batch-probe'}}}))
 p=subprocess.Popen([str(BIN),'--home',str(home),'--config',str(cfg),'--bind','127.0.0.1:0'],stdout=subprocess.PIPE,stderr=(run/'stderr.log').open('w'),text=True)
 try:
  end=time.monotonic()+20;addr=None
  while time.monotonic()<end:
   if p.poll()!=None:raise RuntimeError('early exit')
   if select.select([p.stdout],[],[],.2)[0]:
    line=p.stdout.readline()
    if line.startswith('LINGXI_SERVICE_READY '):addr=[x.split('=',1)[1] for x in line.split() if x.startswith('addr=')][0];break
  assert addr
  token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
  def call_api(method,path,data=None):
   req=urllib.request.Request('http://'+addr+path,data=json.dumps(data).encode() if data else None,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},method=method)
   with urllib.request.urlopen(req,timeout=25) as r:return json.load(r)
  ack=call_api('POST','/lingxi/v1/sessions/sess_local_alpha/execute',{'input':'Execute model tool batch.'});events=call_api('GET','/lingxi/v1/sessions/sess_local_alpha/events')
  db=sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro',uri=True);row=db.execute('select status,terminal_reason from runs where run_id=?',(ack['runId'],)).fetchone();db.close()
  files={x.name:x.read_text() for x in ws.iterdir() if x.is_file()}
  out={'case':name,'configured_calls':calls,'requests':len(requests),'files_written':files,'run':row,'tool_outcomes':[e['payload'] for e in events['items'] if e['payload'].get('type') in ['tool_call_completed','tool_call_started']],'second_request_messages':requests[1].get('messages') if len(requests)>1 else None,'evidence_dir':str(run)}
  (run/'events.json').write_text(json.dumps(events,indent=2));(run/'requests.json').write_text(json.dumps(requests,indent=2));return out
 finally:
  if p.poll() is None:p.send_signal(signal.SIGTERM)
  try:p.wait(timeout=20)
  except subprocess.TimeoutExpired:p.kill();p.wait()
  server.shutdown();server.server_close()
results=[probe('batch-invalid-second',[('call_good','write',{'path':'output.txt','content':'BAD_BATCH_WROTE_FILE'}),('call_bad','read',{'path':123})]),probe('batch-duplicate-id',[('dup','write',{'path':'a.txt','content':'A'}),('dup','write',{'path':'b.txt','content':'B'})])]
(ROOT/'batch-result.json').write_text(json.dumps(results,indent=2));print(json.dumps(results,indent=2))
