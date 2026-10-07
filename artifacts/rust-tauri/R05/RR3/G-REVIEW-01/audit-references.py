import pathlib,json,hashlib,datetime,collections
R=pathlib.Path.cwd(); B=R/'artifacts/rust-tauri/R05/RR3';E=B/'G-REVIEW-01'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
def check_inputs(label,files):
 changes=[]
 for p,h in files.items():
  if not (R/p).is_file() or sha(R/p)!=h: changes.append({'path':p,'expected':h,'actual':sha(R/p) if (R/p).is_file() else None})
 return {'label':label,'count':len(files),'changes':changes,'equal':not changes}
rows=[]
a=load(B/'A-REVIEW-02/input-manifest.json');rows.append(check_inputs('A19',{x['path']:x['sha256'] for x in a['files']}))
c=load(B/'C-F46-REVIEW-01/FINAL_SOURCE_BINDING.json');rows.append(check_inputs('C/F46 actual321',c['executionInputComparison']['actualHashes']))
bfiles=['scripts/rust-tauri/r05_t08_negative_gate.sh','scripts/rust-tauri/r05_t08_mutate_pin.py','scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py','docs/rust-tauri/R05/r05_stage_pins.tsv']
b=load(B/'B-REVIEW-01/source-after-production.json')['files']; bmap=b if isinstance(b,dict) else {x['path']:x['sha256'] for x in b}
rows.append(check_inputs('B4',{p:bmap[p]['sha256'] if isinstance(bmap[p],dict) else bmap[p] for p in bfiles}))
manifests=[]
for directory,name in [('A-REVIEW-02','manifest.json'),('B-REVIEW-01','evidence-sha256.json'),('C-F46-REVIEW-01','MANIFEST.json')]:
 root=B/directory; data=load(root/name); entries=data['files'] if isinstance(data,dict) else data
 if isinstance(entries,dict): entries=[{'path':p,**v} for p,v in entries.items()]
 bad=[]
 for entry in entries:
  p=root/entry['path']
  if not p.is_file() or sha(p)!=entry['sha256']:bad.append(entry['path'])
 manifests.append({'manifest':str((root/name).relative_to(R)),'sha256':sha(root/name),'fileCount':len(entries),'mismatches':bad})
# 所有JSON完整解析后才重算层级绑定；历史失败不会被文字报告遮盖。
F=R/'artifacts/rust-tauri/R05/RR2/FINAL-01/verify-r05-2'; historical=[]
for p in F.rglob('verify-stage-result.json'):
 d=load(p);b=d.get('candidateSourceBinding',{});check=b.get('checkpointAfterEveryCommand',[])
 historical.append({'path':str(p.relative_to(R)),'sha256':sha(p),'stage':d.get('stage'),'overall':d.get('overall'),'commands':[{'key':x.get('key',x.get('commandKey')),'status':x.get('status'),'exitCode':x.get('exitCode')} for x in d.get('commands',[])],'stable':b.get('stable'),'checkpoints':check,'runnerSourceBinding':d.get('runnerSourceBinding',{}).get('status')})
raw=load(B/'C-F46-REVIEW-01/resources-01/f27-resource-series.json')
resource={'sha256':sha(B/'C-F46-REVIEW-01/resources-01/f27-resource-series.json'),'load':raw['load'],'counts':{'series':len(raw['series']),'cycles':len(raw['cycleResults']),'owners':len(raw['ownerResourceSeries'])},'binaryPhases':dict(collections.Counter(x['phase'] for x in raw['series'])),'ownerPhases':dict(collections.Counter(x['phase'] for x in raw['ownerResourceSeries'])),'cleanup':raw['cleanup']}
result={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'inputs':rows,'manifests':manifests,'historicalLayers':historical,'newCResourceRaw':resource,'boundary':'input subset equality and evidence hash audit; no current whole-tree freeze or new business-gate PASS implied'}
(E/'reference-audit.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'inputs':rows,'manifests':[{k:v for k,v in x.items() if k!='sha256'} for x in manifests],'layers':len(historical),'resources':resource['counts']},ensure_ascii=False))
