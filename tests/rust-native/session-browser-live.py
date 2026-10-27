# 当前真实服务和 Electron 浏览器宿主；所有数据均在合成根内。
import json,os,pathlib,re,subprocess,urllib.request,urllib.error
root=pathlib.Path(os.environ['R06_SESSION_EVIDENCE']);root.mkdir(parents=True);root=root.resolve();home=root/'home';home.mkdir();workspace=root/'workspace';workspace.mkdir();product=root/'product/yuan';product.mkdir(parents=True);(product/'lingxi.md').write_text('你是{{agentName}}。')
(root/'config.json').write_text(json.dumps({'home':str(home),'workspace':str(workspace)}));log=(root/'service.log').open('w');proc=subprocess.Popen([os.environ['R06_SESSION_SERVICE_BIN'],'--config',str(root/'config.json'),'--bind','127.0.0.1:0'],env={**os.environ,'LINGXI_HOME':str(home),'TMPDIR':str(root),'LINGXI_PRODUCT_DIR':str(product.parent)},stdout=subprocess.PIPE,stderr=log,text=True)
try:
 url=None
 for line in proc.stdout:
  log.write(line);log.flush();m=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
  if m:url='http://'+m[1];break
 assert url
 token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
 def api(method,p,body=None,status=200):
  req=urllib.request.Request(url+p,data=json.dumps(body).encode()if body is not None else None,method=method,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
  try:r=urllib.request.urlopen(req,timeout=20)
  except urllib.error.HTTPError as e:r=e
  data=json.loads(r.read());assert r.status==status,(r.status,data);return data
 api('POST','/lingxi/v1/agents',{'id':'browser-agent','name':'browser-agent','yuan':'lingxi'})
 for sid in ['browser-a','browser-b','browser-c']:api('POST','/lingxi/v1/sessions',{'sessionId':sid,'agentId':'browser-agent','title':sid,'memoryEnabled':False},201)
 foreign=api('POST','/lingxi/v1/devices/credentials',{'userId':'foreign-browser','scopes':['chat']},201)['secret']
 repo=pathlib.Path(__file__).resolve().parents[2];electron=repo/'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'
 result=subprocess.run([str(electron),str(repo/'desktop/src/react/__tests__/session-browser-live/main.cjs')],cwd=repo,env={**os.environ,'R06_SESSION_EVIDENCE':str(root),'R06_SESSION_URL':url,'R06_SESSION_TOKEN':token,'R06_FOREIGN_TOKEN':foreign},stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=60);(root/'browser-window.log').write_text(result.stdout);assert result.returncode==0,result.stdout
 result=json.loads((root/'browser-result.json').read_text());assert result['status']=='PASS';print(root,'PASS')
finally:proc.terminate();proc.wait(timeout=15);log.close()
