from pathlib import Path
import json,hashlib,datetime,subprocess,os,sys
ROOT=Path.cwd(); E=ROOT/'artifacts/rust-tauri/R05/RR3/E-02'; D=ROOT/'docs/rust-tauri/R05'; R='artifacts/rust-tauri/R05/RR3/'
OWNED=[str(D.relative_to(ROOT)/n) for n in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_HANDOFF.json','PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_NEGATIVE_GATE_REPORT.md','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json','WORKER_MODEL_BOUNDARY.md','MODEL_USAGE_SEMANTICS.md','R05_INTERFACE_EVOLUTION.md']]+['docs/rust-tauri/ORCHESTRATOR_PROGRESS.json']
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with Path(p).open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
 return h.hexdigest()
def dump(p,d):Path(p).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def load(p):return json.loads(Path(p).read_text())
def source():
 fs=[]
 for dirname,excluded in [('rust',{'target','.git'}),('scripts/rust-tauri',{'__pycache__'})]:
  for directory,dirs,names in os.walk(ROOT/dirname):
   dirs[:]=[n for n in dirs if n not in excluded]
   fs.extend(Path(directory)/n for n in names if (Path(directory)/n).is_file())
 fs+= [ROOT/'rust-toolchain.toml',ROOT/'shared/contract-versions.json']
 entries={str(p.relative_to(ROOT)):sha(p) for p in sorted(set(fs))}
 old=load(ROOT/(R+'E-01/inputhash-before.json'))
 return {'capturedAt':now(),'algorithm':old['algorithm'],'scope':old['scope'],'count':len(entries),'digest':hashlib.sha256(''.join(f'{h}  {p}\n' for p,h in sorted(entries.items())).encode()).hexdigest(),'files':entries}
def run(label,argv):
 out=E/'logs'/f'{label}-stdout.log';err=E/'logs'/f'{label}-stderr.log';out.parent.mkdir(exist_ok=True)
 start=now()
 with out.open('wb') as a,err.open('wb') as b:p=subprocess.run(argv,cwd=ROOT,stdout=a,stderr=b)
 record={'label':label,'argv':argv,'cwd':str(ROOT),'startedAt':start,'endedAt':now(),'exitCode':p.returncode,'stdout':str(out.relative_to(ROOT)),'stderr':str(err.relative_to(ROOT)),'stdoutSha256':sha(out),'stderrSha256':sha(err)}
 cp=E/'commands.json'; records=load(cp) if cp.exists() else [];records.append(record);dump(cp,records)
 print(label,'exit',p.returncode)
 return p.returncode
if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='before':
  assert not (E/'before.json').exists()
  for p in OWNED:
   target=E/'before'/p;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes((ROOT/p).read_bytes())
  dump(E/'before.json',{'utc':now(),'owned':{p:sha(ROOT/p) for p in OWNED},'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()})
  dump(E/'inputhash-before.json',source())
  guards=[str(p.relative_to(ROOT)) for p in (D/'repair-current').glob('*') if p.is_file()]
  guards+=['docs/rust-tauri/R05/'+n for n in ['R05_SCOPE_MATRIX.json','r05_stage_pins.tsv','r05_stage_cids.tsv','r05_required_cids.tsv','r05_leaf_case_map.tsv']]
  dump(E/'protected-before.json',{p:sha(ROOT/p) for p in guards})
  reading=guards+[R+'E-01/REPORT.md',R+'E-REVIEW-01/REVIEW.md']
  reading+=list(OWNED)
  reading+= [str(p.relative_to(ROOT)) for p in (ROOT/'artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/specifications').rglob('*') if p.is_file()]
  dump(E/'READING.json',{'utc':now(),'scope':'完整材料字节读取索引；源码与接口另存source-audit；不冒称重新执行原始测试。','files':{p:{'sha256':sha(ROOT/p),'bytes':(ROOT/p).stat().st_size} for p in reading}})
  print('before',len(OWNED),'source',source()['count'])
 elif mode=='manifests':
  results={}
  for folder,name in [('A-REVIEW-02','manifest.json'),('C-F46-REVIEW-01','MANIFEST.json'),('D-REVIEW-01','evidence-manifest.json'),('E-REVIEW-01','manifest.json')]:
   root=ROOT/R/folder; mp=root/name
   if not mp.exists():
    # 保留真实文件名定位，不猜一个不存在的清单。
    candidates=[p for p in root.glob('*manifest*.json') if p.is_file()]
    assert len(candidates)==1,(folder,candidates);mp=candidates[0]
   d=load(mp); items=d['files'];items=[(x['path'],x) for x in items] if isinstance(items,list) else list(items.items())
   checked=[]
   for rel,meta in items:
    p=root/rel;actual=sha(p);size=p.stat().st_size
    expected=meta.get('bytes',meta.get('size'));assert actual==meta['sha256'],p
    if expected is not None:assert expected==size,p
    checked.append({'path':str(p.relative_to(ROOT)),'sha256':actual,'bytes':size})
   results[folder]={'manifest':str(mp.relative_to(ROOT)),'manifestSha256':sha(mp),'checked':checked,'count':len(checked),'errors':[]}
  dump(E/'manifest-consumption.json',results);print({k:v['count'] for k,v in results.items()})
 elif mode=='after':dump(E/'inputhash-after.json',source());print('after',source()['count'])
