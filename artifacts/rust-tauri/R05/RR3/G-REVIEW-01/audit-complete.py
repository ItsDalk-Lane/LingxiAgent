import pathlib,json,hashlib,re,datetime,collections,subprocess,os
E=pathlib.Path(__file__).resolve().parent;R=pathlib.Path.cwd();C=pathlib.Path(json.loads((E/'copy-path.json').read_text())['path'])
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
assert (E/'extra-complete.json').exists()
b=load(E/'source-copy-boundary.json');initial=load(E/'source-before.json')['files'];final={}
for sub in ['rust','scripts/rust-tauri','contracts','docs/rust-tauri']:
 for root,ds,fs in os.walk(R/sub):
  ds[:]=[x for x in ds if x not in ['target','__pycache__','.git']]
  for f in fs:
   p=pathlib.Path(root)/f
   if p.is_file() and p.suffix!='.pyc':final[str(p.relative_to(R))]=sha(p)
for p in ['rust-toolchain.toml','.gitignore']:final[p]=sha(R/p)
changes=[{'path':p,'before':initial.get(p),'after':final.get(p),'executionInput':p in b['executionInputs'] or p.startswith(('rust/','scripts/rust-tauri/','contracts/'))} for p in sorted(initial.keys()|final.keys()) if final.get(p)!=initial.get(p)]
copy=load(E/'extra-complete.json')['files']
source={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'metadataChanges':changes,'executionInputsCount':len(b['executionInputs']),'mainExecutionInputsEqual':all(final.get(p)==h for p,h in b['executionInputs'].items()) and not any(x['executionInput'] for x in changes),'restoredCopyExecutionInputsEqual':all(copy.get(p)==h for p,h in b['executionInputs'].items()),'wholeScopeCopyInitialEqual':copy==load(E/'copy-initial.json')['files'],'boundary':'913明确文件观察范围，486实际执行输入；不是全树冻结'}
(E/'final-input-boundary.json').write_text(json.dumps(source,ensure_ascii=False,indent=2)+'\n')
d=E/'default16-01';expected=[f'R05-GATE-N{i:02d}' for i in range(1,17)]
if (d/'case-results.json').exists():result=load(d/'case-results.json')
else:
 rows=[]
 for line in (d/'case-results.tsv').read_text().splitlines():
  case,exitcode,named,verdict,note=line.split('\t');rows.append({'case':case,'exitCode':int(exitcode),'gapNamed':named,'verdict':verdict,'note':note})
 result={'schema':'reviewer-observed-partial-default-not-production-summary','requestedScope':'ALL','expectedCases':expected,'cases':rows,'notRecordedCases':[x for x in expected if x not in [r['case'] for r in rows]],'productionCaseResultsJSON':None,'allRefused':False,'controlsGreen':all(int((d/c/'exit-code.txt').read_text())==0 for c in ['control-xtask','control-binwiring','n03-restored']),'boundary':'默认主入口因被改写退出2，仅15真实case行；补证不能伪装成生产汇总16/16'}
logrows=[]
for p in sorted(E.rglob('*.log')):
 if 'dispatch' in p.parts:continue
 s=p.read_text(errors='replace');cnt=re.findall(r'test result: ([^\n]+)',s)
 logrows.append({'path':str(p.relative_to(E)),'sha256':sha(p),'bytes':p.stat().st_size,'actualCounts':cnt,'unrelatedRustCompileErrors':re.findall(r'error\[E[0-9]+\][^\n]*',s),'runningCargoPaths':re.findall(r'Running[^\n]+',s)})
layers=[]
for p in sorted(E.rglob('verify-stage-result.json')):
 x=load(p);cb=x.get('candidateSourceBinding',{});layers.append({'path':str(p.relative_to(E)),'sha256':sha(p),'stage':x.get('stage'),'overall':x.get('overall'),'stable':cb.get('stable'),'beforeDigest':cb.get('before',{}).get('digestSha256'),'afterDigest':cb.get('after',{}).get('digestSha256'),'binding':cb,'runner':x.get('runnerSourceBinding'),'commands':x.get('commands')})
rest=[]
for p in (d/'pristine').rglob('*'):
 if p.is_file():
  rel=p.relative_to(d/'pristine');rest.append({'path':str(rel),'pristineSHA256':sha(p),'currentCopySHA256':sha(C/rel),'equal':sha(p)==sha(C/rel)})
audit={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'default':load(E/'default-command.json'),'uniqueCases':dict(collections.Counter(x['case'] for x in result['cases'])),'result':result,'allDefaultTargetsNonzero':all(x['exitCode']!=0 for x in result['cases']),'logs':logrows,'resultLayers':layers,'restoration':rest,'all12Restored':len(rest)==12 and all(x['equal'] for x in rest),'supplementCommands':load(E/'supplement-commands.json'),'inputBoundary':source}
(E/'actual-evidence-audit.json').write_text(json.dumps(audit,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'defaultExit':audit['default']['exitCode'],'cases':len(result['cases']),'allDefaultTargetsNonzero':audit['allDefaultTargetsNonzero'],'all12Restored':audit['all12Restored'],'inputBoundary':{k:v for k,v in source.items() if k!='metadataChanges'},'layerVerdicts':[{k:v for k,v in a.items() if k not in ['binding','runner','commands']} for a in layers]},ensure_ascii=False))
