# 仅对已完成阶段二进制做合成数据与回环取证，不实施修复。
import os, json, hashlib, subprocess, datetime, time, re, signal, urllib.request, urllib.error
from pathlib import Path
BASE=Path(__file__).resolve().parents[1]
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
BINARY=ROOT/'artifacts/rust-tauri/R05/RR3/C-F46-REVIEW-01/isolated/build/lingxi-service'
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def write(p,x): p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(x,ensure_ascii=False,indent=2))
def snapshot(p): return {str(f.relative_to(p)):sha(f) for f in p.rglob('*') if f.is_file()}
identity=json.loads((BASE/'manifest/probe-binary-before.json').read_text())
assert sha(BINARY)==identity['sha256']
# 精确限制环境；不读取或使用真实供应商密钥。
def env_for(home,level):
 e={'PATH':'/usr/bin:/bin','HOME':str(home),'TMPDIR':str(BASE/'input/probe-tmp')}
 (BASE/'input/probe-tmp').mkdir(exist_ok=True)
 if level is not None:e['RUST_LOG']=level
 return e
rows=[]
for source,level in [('cli','warn'),('cli','info'),('cli',None),('env','info'),('config-file','info')]:
 tag=f'epoch-{source}-{level or "unset"}';d=BASE/'input'/tag;d.mkdir(exist_ok=False)
 home=d/'selected';fallback=d/'fallback';(home/'app-data').mkdir(parents=True);fallback.mkdir()
 (home/'app-data/existing.bin').write_text('preserve-original-data\n')
 stamp={'schemaVersion':2,'epoch':2,'minimumReaderEpoch':2,'committedDataEpoch':2,'lastVersion':'9.9.9','updatedAt':'2026-09-26T08:00:00.000Z'}
 write(home/'data-epoch.json',stamp)
 e=env_for(fallback,level);e['LINGXI_HOME']=str(fallback)
 argv=[str(BINARY),'--bind','127.0.0.1:0']
 if source=='cli':argv+=['--home',str(home)]
 elif source=='env':e['LINGXI_HOME']=str(home)
 else:
  e.pop('LINGXI_HOME');write(d/'config.json',{'home':str(home)});argv+=['--config',str(d/'config.json')]
 before=snapshot(home);fb=snapshot(fallback);start=now();r=subprocess.run(argv,capture_output=True,env=e,timeout=15);end=now()
 (BASE/'commands'/f'{tag}.stdout.log').write_bytes(r.stdout);(BASE/'commands'/f'{tag}.stderr.log').write_bytes(r.stderr)
 write(BASE/'commands'/f'{tag}.json',{'argv':argv,'env':e,'start':start,'end':end,'binarySHA256':sha(BINARY)})
 write(BASE/'exit'/f'{tag}.json',{'exit':r.returncode})
 err=r.stderr.decode();after=snapshot(home)
 row={'case':tag,'source':source,'RUST_LOG':level,'exit':r.returncode,'ready':b'LINGXI_SERVICE_READY ' in r.stdout,'epochRefused':r.returncode==2 and b'LINGXI_SERVICE_READY ' not in r.stdout,'explicitError':'LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked' in err and 'epoch 2 or newer' in err,'selectedDataPreserved':all(after.get(k)==v for k,v in before.items()),'fallbackPreserved':fb==snapshot(fallback),'effectiveHomeDiagnostic':f'effective_home={home}' in err,'sourceDiagnostic':f'source={source}' in err,'databaseCreated':(home/'lingxi-service/data').exists(),'localTokenCreated':(home/'lingxi-service/local-token.json').exists(),'before':before,'after':after}
 row['originalNoSwitchCheck']=row['selectedDataPreserved'] and row['fallbackPreserved'] and row['effectiveHomeDiagnostic'] and row['sourceDiagnostic'] and not row['databaseCreated'] and not row['localTokenCreated'] and not row['ready']; rows.append(row)
 print(tag,'exit',r.returncode,'refused',row['epochRefused'],'preserved',row['selectedDataPreserved'],row['fallbackPreserved'],'diagnostics',row['effectiveHomeDiagnostic'],row['sourceDiagnostic'],'original-no-switch',row['originalNoSwitchCheck'])
write(BASE/'input/epoch-probe-results.json',rows)
# 正式程序的真实认证错误，只有回环、合成错误认证，无模型调用。
tag='request-correlation';d=BASE/'input'/tag;d.mkdir(exist_ok=False);home=d/'home';home.mkdir();env=env_for(home,'info')
argv=[str(BINARY),'--home',str(home),'--bind','127.0.0.1:0'];start=now()
out=open(BASE/'commands'/f'{tag}.stdout.log','wb');err=open(BASE/'commands'/f'{tag}.stderr.log','wb');p=subprocess.Popen(argv,stdout=out,stderr=err,env=env)
try:
 addr=None
 for _ in range(200):
  txt=(BASE/'commands'/f'{tag}.stdout.log').read_text();match=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',txt)
  if match:addr=match[1];break
  if p.poll() is not None:raise RuntimeError('服务未达到READY，保留准备失败')
  time.sleep(.05)
 if not addr:raise RuntimeError('等待READY超时，保留准备失败')
 request={'method':'GET','url':f'http://{addr}/lingxi/v1/me','headers':{'Authorization':'Bearer a13-triage-synthetic-invalid-credential'}};write(d/'request.json',request)
 req=urllib.request.Request(request['url'],headers=request['headers']);opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
 try:
  response=opener.open(req,timeout=5);status=response.status;body=response.read()
 except urllib.error.HTTPError as ex:status=ex.code;body=ex.read()
 (d/'response.body').write_bytes(body)
finally:
 if p.poll() is None:p.send_signal(signal.SIGTERM)
 try:rc=p.wait(timeout=10)
 except subprocess.TimeoutExpired:p.kill();rc=p.wait(timeout=5)
 out.close();err.close()
 write(BASE/'commands'/f'{tag}.json',{'argv':argv,'env':env,'pid':p.pid,'start':start,'end':now(),'binarySHA256':sha(BINARY)})
 write(BASE/'exit'/f'{tag}.json',{'exit':rc,'reaped':p.poll() is not None})
request_id=json.loads(body)['details']['requestId'];stderr=(BASE/'commands'/f'{tag}.stderr.log').read_text();markers=[l for l in stderr.splitlines() if 'LINGXI_AUTH_REJECTED' in l];logtext=''.join(f.read_text() for f in (home/'lingxi-service/logs').glob('*.log'))
result={'httpStatus':status,'requestId':request_id,'requestIdBytes':len(request_id),'assignmentBytes':len('request_id='+request_id),'stderrMarkers':markers,'correlationPresent':any(request_id in l for l in markers),'traceCorrelationPresent':request_id in logtext,'exit':rc,'binarySHA256':sha(BINARY),'noProviderConfigured':not (home/'config.json').exists()}
write(d/'result.json',result);print(json.dumps(result,ensure_ascii=False))
write(BASE/'manifest/probe-binary-after.json',{'observedUTC':now(),'binary':str(BINARY),'sha256':sha(BINARY),'same':sha(BINARY)==identity['sha256']})
