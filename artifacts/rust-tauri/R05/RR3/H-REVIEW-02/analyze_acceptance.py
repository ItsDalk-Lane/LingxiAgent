from capture import *
from collections import Counter
rows=[]
for level in ['warn','info']:
 base=EV/'commands/a01-final-both-levels/levels'/level
 doc=json.loads((base/'a01-leaf-cases.json').read_text());cases=doc['cases'];assert len(cases)==17 and all(c['ok'] and c['expect']==c['actual'] for c in cases)
 sources=[]
 for tag,source in [('a01-newer-data','cli'),('a01-newer-env-data','env'),('a01-newer-config-data','config-file')]:
  original=(base/(tag+'-original-before.sha256')).read_bytes();after=(base/(tag+'-original-after.sha256')).read_bytes();fallback=(base/(tag+'-fallback-before.txt')).read_bytes();fallafter=(base/(tag+'-fallback-after.txt')).read_bytes();err=(base/(tag+'.stderr.log')).read_text();out=(base/(tag+'.stdout.log')).read_text();tree=(base/(tag+'-homes-after.txt')).read_text()
  selected=original.decode().splitlines()[0].split('  ',1)[1].removesuffix('/data-epoch.json')
  checks={'originalBytesPreserved':original==after,'fallbackPreserved':fallback==fallafter,'selectedSourceDiagnostic':f'effective_home={selected}' in err and f'source={source}' in err,'correctEpochDiagnostic':'epoch-downgrade-blocked' in err and 'epoch 2 or newer' in err,'noReady':'LINGXI_SERVICE_READY' not in out,'noStorageOrToken':'lingxi-service/data' not in tree and 'local-token.json' not in tree}
  assert all(checks.values()),checks
  sources.append({'source':source,'checks':checks,'actualExitContract':2,'exitEvidence':'unchanged production shell obtains real wait status then requires equality to 2; exact case green','leafCases':[c for c in cases if c['case'].startswith(tag)]})
 rows.append({'level':level,'count':len(cases),'failed':0,'sources':sources,'health':json.loads((base/'a01-health-body.json').read_text())})
base=EV/'commands/a13-final/A13/redaction-scan';log=(EV/'commands/a13-final/stdout.log').read_text();scan=re.search(r'0 occurrences of 6 preset/minted secrets across (\d+)/(\d+) read evidence files',log);assert scan
assert all('SCAN '+v+': clean (0 files)' in log for v in ['api_key','device_secret','oauth_bearer','query_token','local_token','ws_ticket'])
assert 'CORRELATION PASS' in log
rid=json.loads((base/'p2-me-bad-bearer.body').read_text())['details']['requestId'];assert re.fullmatch(r'req-[0-9a-f]{32}',rid)
assert any(rid in l and 'LINGXI_AUTH_REJECTED' in l for l in (base/'service-p1.err').read_text().splitlines())
manifest=(base/'service-logs/retention-manifest.txt').read_text();retained=[]
for l in manifest.splitlines():
 m=re.fullmatch(r'([0-9a-f]{64})  (.+)  bytes=(\d+)',l)
 if m:
  p=base/'service-logs'/m[2];assert sha(p)==m[1] and p.stat().st_size==int(m[3]);retained.append({'path':str(p),'sha256':sha(p),'bytes':p.stat().st_size})
assert len(retained)==len(list((base/'service-logs').glob('*.log')))==3
inventory=(base/'inventory.txt').read_text();assert all(r['sha256'] in inventory for r in retained)
runid=json.loads((base/'p1-execute-ok.body').read_text())['runId'];assert runid
statuses=(base/'p3-status-codes.txt').read_text().splitlines()
write(EV/'ACCEPTANCE_RECOMPUTATION.json',{'A01':rows,'A13':{'scriptSHA256':sha(ROOT/'scripts/rust-tauri/r02_t07_redaction_scan.sh'),'secretKinds':6,'actualReads':int(scan[1]),'targetFiles':int(scan[2]),'unreadable':0,'occurrences':0,'requestId':rid,'requestIdLength':len(rid),'bareAssignmentLength':len('request_id='+rid),'runId':runid,'retainedSourceBoundFiles':retained,'inventoryVerified':True,'p3ActualStatusCounts':dict(Counter(statuses)),'boundary':'all six secrets were scanned in actual unmodified shell while local token/home existed; no claim of re-reading destroyed token after cleanup'}})
print({'A01':[len(r['sources']) for r in rows],'A13secretReads':int(scan[1]),'A13targetFiles':int(scan[2]),'retained':len(retained),'P3':dict(Counter(statuses))})
