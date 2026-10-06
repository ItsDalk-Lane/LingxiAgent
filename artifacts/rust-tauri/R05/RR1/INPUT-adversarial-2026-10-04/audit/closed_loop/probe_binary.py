import http.server, threading, subprocess, pathlib, tempfile, time, json, urllib.request, socket, os, signal, select, sqlite3, hashlib
ROOT=pathlib.Path('/workspace/scratch/9b4397d87a13/audit/closed_loop')
BIN=pathlib.Path('/workspace/scratch/9b4397d87a13/audit/runtime/target/debug/lingxi-service')
run=pathlib.Path(tempfile.mkdtemp(prefix='binary-probe-',dir=ROOT));home=run/'home';ws=run/'workspace';home.mkdir();ws.mkdir()
requests=[]
RAW='literal:\n```\n<think>keep quoted</think>\n```\n<think>PRIVATE_THINK_PROBE</think><mood>PRIVATE_MOOD_PROBE</mood>VISIBLE_FINAL_PROBE'
class H(http.server.BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append({'path':self.path,'body':body})
        parts=[{'id':'probe','choices':[{'index':0,'finish_reason':None,'delta':{'role':'assistant','content':RAW}}]}, {'id':'probe','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]}, {'id':'probe','choices':[],'usage':{'prompt_tokens':3,'completion_tokens':4}}]
        data=''.join('data: '+json.dumps(p)+'\n\n' for p in parts)+'data: [DONE]\n\n';data=data.encode()
        self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.send_header('Connection','close');self.end_headers();self.wfile.write(data)
    def log_message(self,*args): pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),H);threading.Thread(target=server.serve_forever,daemon=True).start()
config={'home':str(home),'workspace':str(ws),'providers':{'text_only':{'protocol':'openai-completions','endpoint':f'http://127.0.0.1:{server.server_port}/v1','auth':{'kind':'none'}}},'models':{'chat':{'provider':'text_only','model':'text-only-no-tools'}}};cfg=run/'config.json';cfg.write_text(json.dumps(config))
def start():
    log=(run/f'stderr-{time.time_ns()}.log').open('w');p=subprocess.Popen([str(BIN),'--home',str(home),'--config',str(cfg),'--bind','127.0.0.1:0'],stdout=subprocess.PIPE,stderr=log,text=True)
    end=time.monotonic()+20
    while time.monotonic()<end:
        if p.poll() is not None:raise RuntimeError(f'service exited {p.returncode}; see {run}')
        if select.select([p.stdout],[],[],.2)[0]:
            line=p.stdout.readline()
            if line.startswith('LINGXI_SERVICE_READY '):
                addr=[w.split('=',1)[1] for w in line.split() if w.startswith('addr=')][0];break
    else: p.kill();raise RuntimeError('ready timeout')
    token=json.loads((home/'lingxi-service/local-token.json').read_text())['token'];return p,addr,token

def http(addr,token,method,path,data=None):
    request=urllib.request.Request('http://'+addr+path,data=json.dumps(data).encode() if data else None,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},method=method)
    with urllib.request.urlopen(request,timeout=20) as r:return json.load(r)
def stop(p):
    if p.poll() is None:p.send_signal(signal.SIGTERM)
    try:p.wait(timeout=20)
    except subprocess.TimeoutExpired:p.kill();p.wait()
p=None
try:
    p,addr,token=start()
    ack=http(addr,token,'POST','/lingxi/v1/sessions/sess_local_alpha/execute',{'input':'Reply with the model response.'})
    events=http(addr,token,'GET','/lingxi/v1/sessions/sess_local_alpha/events')
    payloads=[e.get('payload',{}) for e in events['items']]
    finals=[x for x in payloads if x.get('type')=='final_message_committed']
    deltas=[{'type':x.get('type'),'phase':x.get('phase',x.get('semanticPhase')),'delta':x.get('delta')} for x in payloads if x.get('type') in ('model_call_delta','assistant_segment_delta')]
    lsof=subprocess.run(['lsof','-p',str(p.pid)],capture_output=True,text=True)
    fd_bug_count=sum(bool(l.strip()) and l.strip()[0].isascii() and l.strip()[0].isdigit() for l in lsof.stdout.splitlines())
    proc_fds=len(list(pathlib.Path(f'/proc/{p.pid}/fd').iterdir())) if pathlib.Path(f'/proc/{p.pid}/fd').is_dir() else 'environment /proc pid not visible'
    # Known concrete leak of 40 temporary file descriptors in this helper, to test the default lsof predicate independent of service.
    probes=[(run/f'fd-{i}').open('w') for i in range(40)]
    own_lsof=subprocess.run(['lsof','-p',str(os.getpid())],capture_output=True,text=True)
    own_bug_count=sum(bool(l.strip()) and l.strip()[0].isascii() and l.strip()[0].isdigit() for l in own_lsof.stdout.splitlines())
    own_proc=len(list(pathlib.Path(f'/proc/{os.getpid()}/fd').iterdir())) if pathlib.Path(f'/proc/{os.getpid()}/fd').is_dir() else 'environment /proc pid not visible'
    for f in probes:f.close()
    db=sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro',uri=True)
    row=db.execute('select status, terminal_reason from runs where run_id=?',(ack['runId'],)).fetchone();db.close()
    before=events;stop(p);p,addr,token=start();after=http(addr,token,'GET','/lingxi/v1/sessions/sess_local_alpha/events')
    out={'head':'d80737b6cb9186c8a18c0f35923aac00249d45c3','binary_sha256':hashlib.sha256(BIN.read_bytes()).hexdigest(),'config_has_capability_declarations':False,'actual_network_requests':len(requests),'wire_tools':[t.get('function',{}).get('name') for t in requests[0]['body'].get('tools',[])],'run_status':row,'live_deltas':deltas,'http_final_messages':finals,'restart_history_equal':before==after,'lsof_exit':lsof.returncode,'lsof_first_lines':lsof.stdout.splitlines()[:6],'test_original_fd_counter':fd_bug_count,'linux_proc_fd_count':proc_fds,'helper_with_40_extra_files':{'original_test_counter':own_bug_count,'linux_proc_fd_count':own_proc,'lsof_first_lines':own_lsof.stdout.splitlines()[:4]},'evidence_dir':str(run)}
    (run/'requests.json').write_text(json.dumps(requests,indent=2));(run/'events-before.json').write_text(json.dumps(before,indent=2));(run/'events-after.json').write_text(json.dumps(after,indent=2));(ROOT/'binary-result.json').write_text(json.dumps(out,ensure_ascii=False,indent=2));print(json.dumps(out,ensure_ascii=False,indent=2))
finally:
    if p is not None:stop(p)
    server.shutdown();server.server_close()
