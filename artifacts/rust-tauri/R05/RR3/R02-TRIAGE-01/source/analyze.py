# 从原始现场逐项复算；历史与活跃输入均保留摘要边界。
import json,hashlib,re,subprocess,importlib.util,datetime
from pathlib import Path
BASE=Path(__file__).resolve().parents[1];ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
s=importlib.util.spec_from_file_location('collect',BASE/'source/collect.py');c=importlib.util.module_from_spec(s);s.loader.exec_module(c)
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def load(p):return json.loads(Path(p).read_text())
def save(p,x):p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(x,ensure_ascii=False,indent=2))
N=ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-01/default16-01/n06-midgate-mutation';E=N/'evidence'
paths=[str(N/'exit-code.txt'),str(N/'named-check.txt'),str(E/'verify-stage-result.json')]
paths += [str(p) for p in (E/'A07_A08').glob('*.json')]
paths += [str(p) for p in (E/'A11/backup-restore').glob('*json')]
paths += [str(p) for p in (E/'CLI_RUST').glob('*json')]
paths += [str(ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-01'/p) for p in ['default-console.log','default16-01/summary.txt','default16-01/case-results.tsv','mutation-state-008.json','source-midrun-observation.json']]
paths += [str(ROOT/'artifacts/rust-tauri/R02/audit-r17-g5/a13/redaction-scan'/p) for p in ['summary.txt','service-p1.err','p2-me-bad-bearer.body','build.log','inventory.txt','service-logs/retention-manifest.txt']]
paths += [str(p) for p in (ROOT/'artifacts/rust-tauri/R02/audit-r17-g5/a13/redaction-scan/service-logs').glob('*.log')]
paths += [str(ROOT/'artifacts/rust-tauri/R05/RR2/FINAL-01/verify-r05-2'/p) for p in ['verify-stage-result.json','R04_REGRESSION/verify-stage-result.json','R04_REGRESSION/R03_REGRESSION/verify-stage-result.json','R04_REGRESSION/R03_REGRESSION/R02/A07_A08/s4-migrations-round1.json','R05_SUITES/f27-resource-series.json']]
print('capture',len(c.capture('late-raw-before',paths)))
a01=E/'A01';err=(a01/'a01-newer-data.stderr.log').read_text();leaf=load(a01/'a01-leaf-cases.json')
no=(a01/'a01-newer-data-homes-after.txt').read_text();original_a01={'leaf':leaf,'stampAndOriginalEqual':(a01/'a01-newer-data-original-before.sha256').read_bytes()==(a01/'a01-newer-data-original-after.sha256').read_bytes(),'fallbackEqual':(a01/'a01-newer-data-fallback-before.txt').read_bytes()==(a01/'a01-newer-data-fallback-after.txt').read_bytes(),'noDatabaseSnapshot':'lingxi-service/data' not in no,'noTokenSnapshot':'local-token.json' not in no,'effectiveHomeLogged':'effective_home=' in err,'sourceLogged':'source=cli' in err,'epochRefusal':'LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked' in err,'readerEpoch1':'this kernel is epoch 1' in err,'requiresEpoch2':'epoch 2 or newer' in err,'regularStartupStderrBytes':(a01/'a01-service-stderr.log').stat().st_size,'missingLaterCases':[s for s in ['env','config'] if not (a01/f'a01-newer-{s}-data.stderr.log').exists()]}
save(BASE/'input/a01-recomputed.json',original_a01)
a13=E/'A13/redaction-scan';rid=load(a13/'p2-me-bad-bearer.body')['details']['requestId'];text=(a13/'service-p1.err').read_text();markers=[l for l in text.splitlines() if 'LINGXI_AUTH_REJECTED' in l];ret=(a13/'service-logs/retention-manifest.txt').read_text();retrows=[]
for l in ret.splitlines():
 m=re.match(r'([0-9a-f]{64})\s+(service-\d+\.log)\s+bytes=(\d+)',l)
 if m:
  p=a13/'service-logs'/m[2];retrows.append({'name':m[2],'manifestSHA256':m[1],'actualSHA256':sha(p),'bytes':p.stat().st_size,'manifestBytes':int(m[3]),'same':sha(p)==m[1] and p.stat().st_size==int(m[3])})
excluded={'p1-execute-request.json','p1-issue-request.json','p1-ticket-request.json','p1-issue-credential.body','p1-ticket.body','summary.txt'}
targets=[p for p in a13.rglob('*') if p.is_file() and p.name not in excluded]
# 已删除的临时凭证文件不作重新读取；可独立复算已有4种预置值和票据，第6种保留原执行的读证据。
script=(ROOT/'scripts/rust-tauri/r02_t07_redaction_scan.sh').read_text();vals={}
for label,key in [('api_key','API_KEY'),('device_secret','DEVICE_SECRET'),('oauth_bearer','OAUTH_TOKEN'),('query_token','QUERY_TOKEN')]:vals[label]=re.search(r'^'+key+r'="([^"]+)"',script,re.M)[1]
vals['ws_ticket']=load(a13/'p1-ticket.body')['ticket']
scan={name:{'valueSHA256':hashlib.sha256(value.encode()).hexdigest(),'hits':[str(p.relative_to(a13)) for p in targets if value.encode() in p.read_bytes()]} for name,value in vals.items()}
original_a13={'requestId':rid,'idBytes':len(rid),'assignmentBytes':len('request_id='+rid),'markerLines':markers,'matchingMarkers':[l for l in markers if rid in l],'matchingRetainedLines':[l for p in (a13/'service-logs').glob('*.log') for l in p.read_text().splitlines() if rid in l],'retention':retrows,'retainedCount':len(retrows),'inventoryExists':(a13/'inventory.txt').exists(),'scanTargets':len(targets),'recomputedSecrets':scan,'localTokenRescan':'NOT_EXECUTED: original synthetic home cleaned; original SCAN 246/41 retained','bodies':{p.name:load(p) for p in a13.glob('p2-*.body') if p.read_text().startswith('{')},'inputBodies':{p.name:load(p) for p in a13.glob('*request.json')},'p3StatusCodes':(a13/'p3-status-codes.txt').read_text(),'wsTranscriptSHA256':sha(a13/'p2-ws-transcript.txt')}
save(BASE/'input/a13-recomputed.json',original_a13)
j=load(E/'verify-stage-result.json');bind=j['candidateSourceBinding'];save(BASE/'input/n06-observed-binding.json',{'observedUTC':c.now(),'exitFile':(N/'exit-code.txt').read_text(),'targetCommands':[r for r in j['commands'] if r['key'] in ['a01_smoke','a13_redaction_scan']],'targetScenarios':[r for r in j['scenarios'] if r['id'] in ['R02-A01','R02-A13']],'stable':bind['stable'],'changedPaths':[bytes.fromhex(v).decode() for v in bind['finalChangedPathBytesHex']],'checkpoints':bind['checkpointAfterEveryCommand'],'runnerSourceBinding':j['runnerSourceBinding'],'beforeDigest':bind['before']['digestSha256'],'afterDigest':bind['after']['digestSha256'],'note':'仅N06读取截点，非G完整结论；业务FAIL独立于绑定变异'})
versions={'authority':load(a01/'a01-health-body.json'),'snapshots':{}}
for p in paths:
 p=Path(p)
 if 'migrations-round' in p.name and p.is_file():
  v=load(p);versions['snapshots'][str(p.relative_to(ROOT))]={'supportedVersion':v.get('supportedVersion'),'userVersion':v.get('userVersion'),'compiledIn':v.get('compiledIn'),'receipts':v.get('receipts'),'sha256':sha(p)}
save(BASE/'input/versions-recomputed.json',versions)
source_rows=[]
for f in ['scripts/rust-tauri/r02_t01_service_smoke.sh','scripts/rust-tauri/r02_t07_redaction_scan.sh','rust/crates/lingxi-service/src/redaction.rs','rust/crates/lingxi-service/src/inject.rs','rust/crates/lingxi-service/src/main.rs','rust/crates/lingxi-service/src/logging.rs','rust/crates/lingxi-service/src/epoch.rs','rust/crates/lingxi-adapters/src/storage/migrations.rs','rust/crates/lingxi-protocol/src/lib.rs']:
 r=subprocess.run(['git','show','HEAD:'+f],cwd=ROOT,capture_output=True)
 source_rows.append({'path':f,'HEADSHA256':hashlib.sha256(r.stdout).hexdigest(),'workingSHA256':sha(ROOT/f),'copySHA256':sha(Path('/Users/study_superior/r05t08-work/negcopy.hTXzzN')/f),'gitShowExit':r.returncode})
save(BASE/'source/head-working-copy-comparison.json',source_rows)
for ref in ['cdd213078','d80737b6c']:
 for f in ['rust/crates/lingxi-service/src/redaction.rs','rust/crates/lingxi-service/src/inject.rs']:
  r=subprocess.run(['git','show',ref+':'+f],cwd=ROOT,capture_output=True);tag=ref+'-'+Path(f).stem
  (BASE/'source'/f'{tag}.rs').write_bytes(r.stdout);save(BASE/'commands'/f'{tag}.json',{'argv':['git','show',ref+':'+f]});save(BASE/'exit'/f'{tag}.json',{'exit':r.returncode})
print('a01 preserved',original_a01['stampAndOriginalEqual'],original_a01['fallbackEqual'],'diagnostic flags',original_a01['effectiveHomeLogged'],original_a01['sourceLogged'])
print('a13',rid,'assignmentBytes',len('request_id='+rid),'markers',len(markers),'matched',len(original_a13['matchingMarkers']),'retention',len(retrows),'targetcount',len(targets),'5secretLeaks',sum(len(v['hits']) for v in scan.values()))
print('N06 stable',bind['stable'],'changed',[bytes.fromhex(v).decode() for v in bind['finalChangedPathBytesHex']],'checkpointcount',len(bind['checkpointAfterEveryCommand']))
print('source differences',[r['path'] for r in source_rows if r['HEADSHA256']!=r['workingSHA256']])
