import argparse, hashlib, http.server, json, pathlib, queue, shutil, signal, sqlite3, subprocess, threading, time, urllib.request, urllib.error, uuid

ROOT=pathlib.Path(__file__).resolve().parents[5]
OUT=ROOT/'artifacts/rust-tauri/R06/AR/W05-E01'

def sha(text): return hashlib.sha256(text.encode()).hexdigest()

def main(name):
    local=OUT/'local-only'/name; local.mkdir(parents=True, exist_ok=False)
    home=local/'home'; home.mkdir(); captures=[]
    class Provider(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            captures.append(body)
            frames=[{'id':'stub','choices':[{'index':0,'delta':{'role':'assistant','content':'synthetic done'},'finish_reason':None}]},
                    {'id':'stub','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]},
                    {'id':'stub','choices':[],'usage':{'prompt_tokens':40,'completion_tokens':5}}]
            payload=(''.join('data: '+json.dumps(f)+'\n\n' for f in frames)+'data: [DONE]\n\n').encode()
            self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.send_header('Content-Length',str(len(payload))); self.end_headers(); self.wfile.write(payload)
        def log_message(self,*args): pass
    provider=http.server.ThreadingHTTPServer(('127.0.0.1',0),Provider)
    thread=threading.Thread(target=provider.serve_forever); thread.start()
    config=local/'service.json'; config.write_text(json.dumps({'home':str(home),'providers':{'main':{'protocol':'openai-completions','endpoint':'http://127.0.0.1:'+str(provider.server_port),'auth':{'kind':'apiKey','apiKey':'synthetic-only'}}},'models':{'chat':{'provider':'main','model':'synthetic','capabilities':{'tools':True}}}}))
    binary=ROOT/'rust/target-w05-e01/debug/lingxi-service'; before=hashlib.sha256(binary.read_bytes()).hexdigest()
    stderr=(local/'stderr.log').open('w'); proc=subprocess.Popen([str(binary),'--home',str(home),'--config',str(config),'--bind','127.0.0.1:0'],stdout=subprocess.PIPE,stderr=stderr,text=True)
    result={'binaryBefore':before,'mock':'external loopback provider only','rssPeak':None,'commands':[]}
    lines=queue.Queue()
    def drain():
        for line in proc.stdout: lines.put(line.rstrip())
    threading.Thread(target=drain,daemon=True).start()
    try:
        deadline=time.monotonic()+30
        while True:
            line=lines.get(timeout=max(.01,deadline-time.monotonic()))
            if line.startswith('LINGXI_SERVICE_READY '): break
        result['ready']=line; addr=next(x[5:] for x in line.split() if x.startswith('addr=')); token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
        def api(method,path,body=None):
            req=urllib.request.Request('http://'+addr+path,data=json.dumps(body).encode() if body is not None else None,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},method=method)
            try:
                with urllib.request.urlopen(req,timeout=40) as res: status=res.status; value=json.load(res)
            except urllib.error.HTTPError as e: status=e.code; value=json.load(e)
            result['commands'].append({'method':method,'path':path,'status':status,'response':value}); return status,value
        def write(path,text):
            p=home/path; p.parent.mkdir(parents=True,exist_ok=True); p.write_text(text)
        nonce=uuid.uuid4().hex
        write('agents/lingxi/config.yaml','locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: true\n')
        write('user/preferences.json','{"locale":"zh-CN","userName":"synthetic"}')
        old='PERSONA_OLD_'+nonce; new='PERSONA_NEW_'+nonce
        assert api('PUT','/lingxi/v1/agents/lingxi/identity',{'content':old})[0]==200
        assert api('PUT','/lingxi/v1/memories/compiled/longterm?agentId=lingxi',{'longterm':'MEMORY_OLD_'+nonce})[0]==200
        assert api('POST','/lingxi/v1/sessions/sess_local_alpha/execute',{'input':'synthetic first'})[0]==200
        status,obs1=api('GET','/lingxi/v1/sessions/sess_local_alpha/context-observation'); assert status==200
        assert len(captures)==1 and old in captures[0]['messages'][0]['content']
        result['positiveControl']=True
        assert api('PUT','/lingxi/v1/agents/lingxi/identity',{'content':new})[0]==200
        assert api('PUT','/lingxi/v1/memories/compiled/longterm?agentId=lingxi',{'longterm':'MEMORY_NEW_'+nonce})[0]==200
        assert api('POST','/lingxi/v1/sessions/sess_local_alpha/execute',{'input':'synthetic next'})[0]==200
        status,obs2=api('GET','/lingxi/v1/sessions/sess_local_alpha/context-observation'); assert status==200
        assert len(captures)==2
        result.update(wire=captures,observationRun1=obs1,observationRun2=obs2)
        sent=captures[1]['messages'][0]['content']
        result['nextRunDynamic']=new in sent and 'MEMORY_NEW_'+nonce in sent and old not in sent
        result['actualWireObservationSame']=obs2['renderSha256']==sha(sent)
        result['oldRunSnapshotFrozen']=obs1['renderSha256']==sha(captures[0]['messages'][0]['content'])
        db=sqlite3.connect(home/'lingxi-service/data/runs.db')
        result['sqlEvents']=db.execute('SELECT event_id,run_id,event_type,payload_json FROM key_events ORDER BY stream_id,seq').fetchall(); db.close()
        result['status']='PASS' if result['nextRunDynamic'] and result['actualWireObservationSame'] else 'FAIL'
    finally:
        proc.send_signal(signal.SIGTERM)
        try: result['serviceExit']=proc.wait(timeout=15)
        except subprocess.TimeoutExpired: proc.kill(); result['serviceExit']=proc.wait(timeout=10)
        stderr.close(); provider.shutdown(); provider.server_close(); thread.join(timeout=10)
        result['binaryAfter']=hashlib.sha256(binary.read_bytes()).hexdigest(); result['cleanup']='service wait/reap; external loopback stopped; own synthetic home remains LOCAL_ONLY_NEVER_GIT'
        (OUT/(name+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2))
    print(json.dumps({k:result.get(k) for k in ['status','positiveControl','nextRunDynamic','actualWireObservationSame','oldRunSnapshotFrozen','serviceExit']}))
    return 0 if result['status']=='PASS' else 1

if __name__=='__main__':
    p=argparse.ArgumentParser(); p.add_argument('name'); a=p.parse_args(); raise SystemExit(main(a.name))
