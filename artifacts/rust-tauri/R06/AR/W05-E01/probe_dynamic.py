import argparse, concurrent.futures, hashlib, http.server, json, pathlib, queue, signal, sqlite3, subprocess, threading, time, urllib.request, urllib.error, uuid
ROOT=pathlib.Path(__file__).resolve().parents[5]
OUT=ROOT/'artifacts/rust-tauri/R06/AR/W05-E01'
def sha(t): return hashlib.sha256(t.encode()).hexdigest()
class Harness:
    def __init__(self,name):
        self.name=name; self.local=OUT/'local-only'/name; self.local.mkdir(parents=True,exist_ok=False)
        self.home=self.local/'home'; self.home.mkdir(); self.wire=[]; self.release=threading.Event(); self.arrived=threading.Event(); self.hold=False
        self.result={'mock':'external loopback only; production binary/auth/storage/compiler','wire':self.wire,'commands':[],'ready':[],'serviceExits':[],'rssPeak':None}
        outer=self
        class Provider(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                body=json.loads(self.rfile.read(int(self.headers['Content-Length']))); outer.wire.append(body)
                hold=outer.hold; outer.arrived.set()
                if hold: assert outer.release.wait(35), 'provider barrier not released'
                frames=[{'id':'synthetic','choices':[{'index':0,'delta':{'role':'assistant','content':'done'},'finish_reason':None}]},{'id':'synthetic','choices':[{'index':0,'delta':{},'finish_reason':'stop'}]},{'id':'synthetic','choices':[],'usage':{'prompt_tokens':40,'completion_tokens':5}}]
                payload=(''.join('data: '+json.dumps(f)+'\n\n' for f in frames)+'data: [DONE]\n\n').encode()
                self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.send_header('Content-Length',str(len(payload))); self.end_headers(); self.wfile.write(payload)
            def log_message(self,*args): pass
        self.provider=http.server.ThreadingHTTPServer(('127.0.0.1',0),Provider); self.thread=threading.Thread(target=self.provider.serve_forever); self.thread.start()
        self.config=self.local/'service.json'; self.config.write_text(json.dumps({'home':str(self.home),'workspace':str(self.home),'providers':{'main':{'protocol':'openai-completions','endpoint':'http://127.0.0.1:'+str(self.provider.server_port),'auth':{'kind':'apiKey','apiKey':'synthetic-only'}}},'models':{'chat':{'provider':'main','model':'synthetic','capabilities':{'tools':True}}}}))
        self.binary=ROOT/'rust/target-w05-e01/debug/lingxi-service'; self.result['binaryBefore']=hashlib.sha256(self.binary.read_bytes()).hexdigest(); self.proc=None; self.start()
        self.write('agents/lingxi/config.yaml','locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: true\n')
        self.write('user/preferences.json','{"locale":"zh-CN","userName":"USER_INITIAL"}')
    def start(self):
        self.stderr=(self.local/('stderr-'+str(len(self.result['ready']))+'.log')).open('w')
        self.proc=subprocess.Popen([str(self.binary),'--home',str(self.home),'--config',str(self.config),'--bind','127.0.0.1:0'],stdout=subprocess.PIPE,stderr=self.stderr,text=True)
        lines=queue.Queue()
        def drain():
            for line in self.proc.stdout: lines.put(line.rstrip())
        threading.Thread(target=drain,daemon=True).start(); deadline=time.monotonic()+30
        while True:
            line=lines.get(timeout=max(.01,deadline-time.monotonic()))
            if line.startswith('LINGXI_SERVICE_READY '): break
        self.result['ready'].append(line); self.addr=next(x[5:] for x in line.split() if x.startswith('addr=')); self.token=json.loads((self.home/'lingxi-service/local-token.json').read_text())['token']
    def stop(self):
        if self.proc is None: return
        self.proc.send_signal(signal.SIGTERM)
        try: code=self.proc.wait(timeout=15)
        except subprocess.TimeoutExpired: self.proc.kill(); code=self.proc.wait(timeout=10)
        self.result['serviceExits'].append(code); self.stderr.close(); self.proc=None
    def write(self,path,text):
        p=self.home/path; p.parent.mkdir(parents=True,exist_ok=True); tmp=p.with_name(p.name+'.new'); tmp.write_text(text); tmp.replace(p)
    def api(self,method,path,body=None,expect=200):
        time.sleep(.14)  # 按原 HTTP 限流额度发请求，不修改产品限额。
        req=urllib.request.Request('http://'+self.addr+'/lingxi/v1'+path,data=json.dumps(body).encode() if body is not None else None,headers={'Authorization':'Bearer '+self.token,'Content-Type':'application/json'},method=method)
        try:
            with urllib.request.urlopen(req,timeout=45) as res: status=res.status; value=json.load(res)
        except urllib.error.HTTPError as e: status=e.code; value=json.load(e)
        self.result['commands'].append({'method':method,'path':path,'status':status,'response':value})
        if expect is not None: assert status==expect,(method,path,status,value)
        return status,value
    def run(self,session='sess_local_alpha',text='synthetic'):
        self.api('POST','/sessions/'+session+'/execute',{'input':text})
        _,view=self.api('GET','/sessions/'+session+'/context-observation')
        sent=self.wire[-1]['messages'][0]['content']; assert view['renderSha256']==sha(sent),(view,sent)
        assert view['sessionId']==session and view['agentId']=='lingxi'
        assert view['modelCallId'] and view['runId'] and view['snapshotId']
        wire_tools=[(t['function']['name'],t['function']['description'],t['function']['parameters']) for t in self.wire[-1].get('tools',[])]
        snapshot_tools=[(t['wireName'],t['description'],t['inputSchema']['schema']) for t in view['toolDeclarations']]
        assert wire_tools==snapshot_tools, 'actual wire tools must match immutable call observation'
        return view,sent
    def exact(self,view):
        q=urllib.parse.urlencode({k:view[k] for k in ['runId','modelCallId','snapshotId']})
        return self.api('GET','/sessions/'+view['sessionId']+'/context-observation?'+q)[1]
    def finish(self):
        self.release.set(); self.stop(); self.provider.shutdown(); self.provider.server_close(); self.thread.join(timeout=10)
        self.result['binaryAfter']=hashlib.sha256(self.binary.read_bytes()).hexdigest()
        db=sqlite3.connect(self.home/'lingxi-service/data/runs.db'); self.result['sqlSnapshotEvents']=db.execute("SELECT event_id,run_id,event_type,payload_json FROM key_events WHERE event_type IN ('model_context_snapshot','model_call_started') ORDER BY stream_id,seq").fetchall(); self.result['sqlRuns']=db.execute('SELECT run_id,session_id,owner_kind,owner_subject,status FROM runs').fetchall(); db.close()
        self.result['cleanup']='service waited/reaped; external loopback closed/joined; owned synthetic home LOCAL_ONLY_NEVER_GIT'
        (OUT/(self.name+'.json')).write_text(json.dumps(self.result,ensure_ascii=False,indent=2))

def dynamic(h):
    nonce=uuid.uuid4().hex; old='PERSONA_OLD_'+nonce; new='PERSONA_NEW_'+nonce
    h.api('PUT','/agents/lingxi/identity',{'content':old}); h.api('PUT','/memories/compiled/longterm?agentId=lingxi',{'longterm':'MEMORY_OLD_'+nonce})
    package=h.local/'package'; package.mkdir(); (package/'SKILL.md').write_text('---\nname: f05skill\ndescription: SKILL_OLD_'+nonce+'\n---\nSynthetic skill body.\n')
    h.api('POST','/skills/install?agentId=lingxi',{'path':str(package)})
    h.api('PUT','/agents/lingxi/skills',{'enabled':['f05skill']})
    first,sent=h.run(text='first'); assert old in sent and 'MEMORY_OLD_'+nonce in sent and 'SKILL_OLD_'+nonce in sent
    h.api('PUT','/agents/lingxi/identity',{'content':new}); h.api('PUT','/memories/compiled/longterm?agentId=lingxi',{'longterm':'MEMORY_NEW_'+nonce})
    h.api('PATCH','/agents/lingxi/skills/f05skill',{'enabled':False})
    second,sent=h.run(text='second'); assert new in sent and 'MEMORY_NEW_'+nonce in sent and old not in sent and 'SKILL_OLD_'+nonce not in sent
    assert h.exact(first)==first, 'Run1 immutable exact lookup after Run2'
    h.write('agents/lingxi/identity.md','DIRECT_PERSONA_'+nonce); h.write('user/preferences.json','{"userName":"USER_DIRECT"}')
    third,sent=h.run(text='direct writes'); assert 'DIRECT_PERSONA_'+nonce in sent and 'USER_DIRECT' in sent
    h.api('PATCH','/sessions/sess_local_alpha/memory',{'memoryEnabled':False})
    fourth,sent=h.run(text='session memory off'); assert 'MEMORY_NEW_'+nonce not in sent
    h.api('PATCH','/sessions/sess_local_alpha/memory',{'memoryEnabled':True})
    h.api('PATCH','/agents/lingxi/config',{'memory':{'enabled':False}})
    fifth,sent=h.run(text='agent memory off'); assert 'MEMORY_NEW_'+nonce not in sent
    h.api('PATCH','/agents/lingxi/config',{'memory':{'enabled':True}})
    h.api('PATCH','/agents/lingxi/skills/f05skill',{'enabled':True})
    h.write('skills/f05skill/SKILL.md','---\nname: f05skill\ndescription: SKILL_NEW_'+nonce+'\n---\nNew synthetic skill.\n')
    sixth,sent=h.run(text='skill changed'); assert 'SKILL_NEW_'+nonce in sent and 'MEMORY_NEW_'+nonce in sent
    h.api('DELETE','/skills/f05skill?agentId=lingxi'); seventh,sent=h.run(text='uninstalled'); assert 'SKILL_NEW_'+nonce not in sent
    external=h.local/'external'; external.mkdir(); ext_skill=external/'externalf05'; ext_skill.mkdir()
    (ext_skill/'SKILL.md').write_text('---\nname: externalf05\ndescription: EXTERNAL_SOURCE_'+nonce+'\n---\nSynthetic external skill.\n')
    h.api('PUT','/skills/external-paths',{'paths':[str(external)]})
    h.api('PUT','/agents/lingxi/skills',{'enabled':['externalf05']})
    external_view,sent=h.run(text='external source'); assert 'EXTERNAL_SOURCE_'+nonce in sent
    h.write('user/preferences.json',json.dumps({'userName':'USER_DIRECT','external_skill_paths':[str(external)],'hidden_skills':['externalf05']}))
    # 原选择规则中 hidden 仅影响目录展示，显式 enabled 的技能继续参与运行。
    hidden_view,sent=h.run(text='hidden source direct preferences'); assert 'EXTERNAL_SOURCE_'+nonce in sent and 'USER_DIRECT' in sent
    actual_preferences=json.loads((h.home/'user/preferences.json').read_text()); assert actual_preferences['hidden_skills']==['externalf05']
    h.api('PUT','/skills/external-paths',{'paths':[]})
    removed_view,sent=h.run(text='source removed'); assert 'EXTERNAL_SOURCE_'+nonce not in sent
    h.result['externalSourceSwitchAndHiddenPreservesExplicitEnabled']={'external':external_view,'hidden':hidden_view,'removed':removed_view,'actualPreferencesAtHiddenRun':actual_preferences,'explicitEnabledStillSent':True}
    h.api('GET','/sessions/sess_local_alpha/context-observation?runId=not-a-run',expect=404)
    # 损坏来源必须响亮失败；失败之后历史快照仍为实际旧请求。
    count=len(h.wire); h.write('user/preferences.json','{broken')
    _,failed=h.api('POST','/sessions/sess_local_alpha/execute',{'input':'bad source'},expect=None)
    assert len(h.wire)==count, 'bad material must not send stale context'; assert h.exact(first)==first
    h.write('user/preferences.json','{"userName":"RECOVERED"}'); eighth,sent=h.run(text='recovery'); assert 'RECOVERED' in sent
    h.result.update(positiveControl=True,nextRunDynamic=True,oldRunImmutable=True,actualWireObservationSame=True,directFilesVisible=True,sessionAndAgentMemoryToggle=True,skillChangeUninstallVisible=True,loudSourceFailure={'response':failed,'wireCountUnchanged':True},snapshots=[first,second,third,fourth,fifth,sixth,seventh,eighth])

def lifecycle(h):
    first=None; views=[]
    for i in range(145):
        sid='f05-session-'+str(i)
        h.api('POST','/sessions',{'sessionId':sid,'agentId':'lingxi','title':'synthetic '+str(i)},expect=201)
        view,sent=h.run(sid,'bounded '+str(i)); views.append(view)
        if first is None: first=view
    stats=h.api('GET','/sessions/'+views[-1]['sessionId']+'/context-observation?includeCacheStats=true')[1]['cacheStats']
    assert stats['entries']<=stats['capacity'] and stats['bytes']<=stats['byteCapacity'] and stats['evictions']>=17,stats
    assert h.exact(first)==first, 'evicted snapshot durable lookup'
    h.api('POST','/sessions/archive',{'sessionId':views[-1]['sessionId']})
    after=h.api('GET','/sessions/'+first['sessionId']+'/context-observation?includeCacheStats=true')[1]['cacheStats']; assert after['reclaimed']>=1 and after['entries']<stats['entries']
    assert h.exact(views[-1])==views[-1], 'archived history still durable'
    h.api('POST','/sessions/archived/delete',{'sessionId':views[-1]['sessionId']})
    h.api('GET','/sessions/'+views[-1]['sessionId']+'/context-observation',expect=404)
    h.stop(); h.start(); assert h.exact(first)==first, 'restart reads committed snapshot rather than compile'
    fresh=h.api('GET','/sessions/'+first['sessionId']+'/context-observation?includeCacheStats=true')[1]['cacheStats']; assert fresh['entries']==0
    h.result.update(positiveControl=True,bounded=True,evictedHistoryDurable=True,archiveReclaims=True,deleteNotFound=True,restartHistoryDurable=True,cacheBefore=stats,cacheAfterArchive=after,cacheAfterRestart=fresh,firstSnapshot=first,sessionCount=145)

def midrun(h):
    h.api('PUT','/agents/lingxi/identity',{'content':'MIDRUN_OLD_PERSONA'}); h.api('PUT','/memories/compiled/longterm?agentId=lingxi',{'longterm':'MIDRUN_OLD_MEMORY'})
    h.hold=True
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
        task=pool.submit(h.api,'POST','/sessions/sess_local_alpha/execute',{'input':'provider barrier'})
        assert h.arrived.wait(30); oldwire=h.wire[0]['messages'][0]['content']; assert 'MIDRUN_OLD_PERSONA' in oldwire
        h.api('PUT','/agents/lingxi/identity',{'content':'MIDRUN_NEW_PERSONA'}); h.api('PUT','/memories/compiled/longterm?agentId=lingxi',{'longterm':'MIDRUN_NEW_MEMORY'})
        h.api('PATCH','/sessions/sess_local_alpha/memory',{'memoryEnabled':False})
        h.hold=False; h.release.set(); task.result(40)
    first=h.api('GET','/sessions/sess_local_alpha/context-observation')[1]
    assert first['renderSha256']==sha(oldwire) and first['sessionMemoryEnabled']==True
    second,newwire=h.run(text='after barrier'); assert 'MIDRUN_NEW_PERSONA' in newwire and 'MIDRUN_OLD_PERSONA' not in newwire and 'MIDRUN_NEW_MEMORY' not in newwire and second['sessionMemoryEnabled']==False
    assert h.exact(first)==first and first['snapshotId']!=second['snapshotId']
    h.result.update(positiveControl=True,currentRunFrozen=True,nextRunUpdated=True,actualCallImmutable=True,firstSnapshot=first,nextSnapshot=second)

def main(name,case):
    h=Harness(name)
    try:
        {'dynamic':dynamic,'lifecycle':lifecycle,'midrun':midrun}[case](h); h.result['status']='PASS'
    except BaseException as e: h.result['status']='FAIL'; h.result['error']=repr(e); raise
    finally: h.finish()
    print(json.dumps({k:h.result[k] for k in ['status','serviceExits']})); return 0
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('name');p.add_argument('case',choices=['dynamic','lifecycle','midrun']);a=p.parse_args();raise SystemExit(main(a.name,a.case))
