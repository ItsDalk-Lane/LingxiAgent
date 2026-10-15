# 双Electron窗口、真正启动的Rust CLI与真实HTTP模型替身；只使用合成目录。
import http.server,json,os,pathlib,re,sqlite3,subprocess,threading,time,urllib.request,urllib.error
root=pathlib.Path(os.environ['R06_QUICKCHAT_EVIDENCE']);root.mkdir(parents=True);home=root/'home';workspace=root/'workspace';home.mkdir();workspace.mkdir();requests=[]
class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
        frames=[{'choices':[{'index':0,'delta':{'role':'assistant','content':'快捷真实回复'},'finish_reason':None}]},{'choices':[{'index':0,'delta':{},'finish_reason':'stop'}]},{'choices':[],'usage':{'prompt_tokens':5,'completion_tokens':3}}]
        data=(''.join('data: '+json.dumps(x)+'\n\n'for x in frames)+'data: [DONE]\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
provider=http.server.ThreadingHTTPServer(('127.0.0.1',0),Provider);threading.Thread(target=provider.serve_forever,daemon=True).start()
product=root/'product/yuan';product.mkdir(parents=True);(product/'lingxi.md').write_text('你是测试助手。')
config={'home':str(home),'workspace':str(workspace),'providers':{'chat':{'protocol':'openai-completions','endpoint':f'http://127.0.0.1:{provider.server_port}/v1','auth':{'kind':'apiKey','apiKey':'synthetic-quickchat'}}},'models':{'chat':{'provider':'chat','model':'default','capabilities':{'tools':True}}}}
(root/'config.json').write_text(json.dumps(config));log=(root/'service.log').open('w');proc=subprocess.Popen([os.environ['R06_QUICKCHAT_SERVICE_BIN'],'--config',str(root/'config.json'),'--bind','127.0.0.1:0'],env={**os.environ,'LINGXI_HOME':str(home),'LINGXI_PRODUCT_DIR':str(product.parent)},stdout=subprocess.PIPE,stderr=log,text=True)
trace=[]
try:
    url=None
    for line in proc.stdout:
        log.write(line);log.flush();match=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
        if match:url='http://'+match[1];break
    assert url
    token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
    def api(method,path,body=None):
        request=urllib.request.Request(url+path,data=json.dumps(body).encode()if body is not None else None,method=method,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
        try:response=urllib.request.urlopen(request,timeout=30)
        except urllib.error.HTTPError as error:response=error
        value=json.loads(response.read());trace.append({'method':method,'path':path,'status':response.status});assert response.status in [200,201],(path,response.status,value);return value
    api('POST','/lingxi/v1/agents',{'id':'quick-a','name':'快捷测试助手','yuan':'lingxi'})
    api('PUT','/lingxi/v1/agents/quick-a/config',{'memory':{'enabled':False}})
    api('POST','/lingxi/v1/sessions',{'sessionId':'quick-existing','agentId':'quick-a','title':'原主窗口会话','permissionMode':'read_only'})
    modes=api('GET','/lingxi/v1/preferences/session-permission-default');assert modes['permissionMode']=='auto'
    preferences=home/'user/preferences.json';preferences.parent.mkdir(parents=True,exist_ok=True);value=json.loads(preferences.read_text())if preferences.exists()else{};value['session_permission_mode_default']='read_only';preferences.write_text(json.dumps(value));assert api('GET','/lingxi/v1/preferences/session-permission-default')['permissionMode']=='read_only'
    repository=pathlib.Path(__file__).resolve().parents[2]
    ledger_errors=[]
    def read_ledger():
        try:
            deadline=time.monotonic()+85
            while not(root/'ledger-phase.json').exists():
                assert time.monotonic()<deadline,'真实窗口未到达用量核验阶段';time.sleep(.03)
            phase=json.loads((root/'ledger-phase.json').read_text());db=sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro',uri=True)
            rows=db.execute('SELECT * FROM model_call_usage WHERE session_id=?',(phase['sessionId'],)).fetchall();assert len(rows)==len(requests)and len(rows)>0
            columns=[r[1]for r in db.execute('PRAGMA table_info(model_call_usage)')];receipt={'sessionId':phase['sessionId'],'modelCalls':len(rows),'columns':columns,'rows':rows};(root/'ledger-receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2));db.close()
        except Exception as error:ledger_errors.append(str(error))
    reader=threading.Thread(target=read_ledger,daemon=True);reader.start()
    output=subprocess.run(['node',str(repository/'desktop/src/react/__tests__/quickchat-live/launch.mjs')],cwd=repository,env={**os.environ,'R06_QUICKCHAT_URL':url,'R06_QUICKCHAT_TOKEN':token},stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=115)
    (root/'electron.log').write_bytes(output.stdout);assert output.returncode==0,output.stdout.decode(errors='replace')
    result=json.loads((root/'result.json').read_text());assert result['status']=='PASS'and len(result['checks'])==9
    assert len(requests)>=1
    reader.join(timeout=2);assert not ledger_errors,ledger_errors;count=json.loads((root/'ledger-receipt.json').read_text())['modelCalls'];assert count==len(requests)
    result['modelCalls']=count;(root/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2));print(root,'PASS',flush=True)
finally:
    proc.terminate();proc.wait(timeout=15);provider.shutdown();log.close();(root/'raw-provider-requests.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2));(root/'http-trace.json').write_text(json.dumps(trace,indent=2))
