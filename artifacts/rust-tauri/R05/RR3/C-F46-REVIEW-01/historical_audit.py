from review_capture import *
from collections import Counter
records=[]
for label in ['C-01','F46-01']:
 base=ROOT/'artifacts/rust-tauri/R05/RR3'/label;d=json.loads((base/'DIGESTS.json').read_text());files=d['files'] if label=='C-01' else d
 results=[];commands=[]
 for name,info in files.items():
  p=base/name;expected=info['sha256'] if isinstance(info,dict) else info
  row={'path':str(p.relative_to(ROOT)),'expectedSha256':expected,'actualSha256':sha(p) if p.is_file() else None};row['equal']=row['expectedSha256']==row['actualSha256'];results.append(row)
  if p.suffix in ('.json','.jsonl','.log','.md','.py','.rs','.diff') and p.is_file():
   content=p.read_text(errors='replace')
   if p.name=='command.json':
    cmd=json.loads(content);commands.append({'path':str(p.relative_to(ROOT)),'command':cmd.get('command'),'exitCode':cmd.get('exitCode'),'summaries':cmd.get('summaries',cmd.get('counts',re.findall(r'test result:.*',content))), 'startedAt':cmd.get('startedAt'),'endedAt':cmd.get('endedAt')})
 raw=base/('measurement-01' if label=='C-01' else 'resources-01')/'f27-resource-series.json';r=json.loads(raw.read_text())
 def lc(point):return len([x for x in point['files']['home'] if x['path'].startswith('lingxi-service/logs/service-') and x['path'].endswith('.log')])
 records.append({'package':label,'fileCount':len(files),'manifestChecks':results,'allManifestEqual':all(x['equal'] for x in results),'commands':commands,'raw':{'sha256':sha(raw),'cycleCounts':dict(Counter(x['phase'] for x in r['cycleResults'])),'binaryPoints':len(r['series']),'ownerPoints':len(r['ownerResourceSeries']),'postRestartLogCount':lc(r['series'][-1]),'steadyLogCounts':sorted(set(lc(x) for x in r['ownerResourceSeries'] if x['phase']=='released-steady'))},'claimBoundary':'C old cargo 2/2 is full resource FAIL if raw exceeds3; implementation data does not replace independent executions'})
(EV/'HISTORICAL_AUDIT.json').write_text(json.dumps({'utc':utc(),'records':records},ensure_ascii=False,indent=2));print([{k:v for k,v in r.items() if k in ('package','fileCount','allManifestEqual','raw')} for r in records])
