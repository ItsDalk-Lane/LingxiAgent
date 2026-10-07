import json,os,hashlib,struct,stat,subprocess,datetime,gzip
from pathlib import Path
OUT=Path('/private/tmp/rr3-storage03-20261007');ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent');META=Path('/private/tmp/rr3-g-review-02-20261007');EV=ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-02'
def utc():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def save(n,x):(OUT/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def sha(p):
 h=hashlib.sha256()
 with open(p,'rb') as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def free():
 v=os.statvfs(ROOT);return v.f_bavail*v.f_frsize
def run(a):
 r=subprocess.run(a,capture_output=True,text=True);return dict(argv=a,exit=r.returncode,stdout=r.stdout,stderr=r.stderr)
all_rows=json.loads((OUT/'objects.json').read_text());rows=[x for x in all_rows if x['decision']=='PROPOSE_EXACT_UNLINK_AFTER_ROOT_AUTHORIZATION']
assert len(rows)==33 and sum(x['bytes'] for x in rows)==136063856
assert len(set(x['path'] for x in rows))==33
save('final-candidates.json',{'at':utc(),'authorization':'Root 2026-10-07 explicitly permits the exact final 33 MH_OBJECT paths after verification. Root was notified that its aggregate copied from a prior message was an arithmetic error: actual sum is 136063856 bytes, unchanged 33-path scope. No other deletion authorized.','objects':rows})
commands=json.loads((OUT/'source-commands.json').read_text())
assert sha(Path(commands['buildLog']['path']))==commands['buildLog']['sha256']
protected=json.loads((OUT/'protected-inputs.json').read_text())['protectedIdentities']
kept=json.loads((OUT/'retained-binaries-and-libraries.json').read_text())
protected.extend(x for x in kept['controls']+kept['n02FreshRetained'] if 'sha256' in x)
for p in EV.rglob('*'):
 if p.is_file() and not p.is_symlink():protected.append(dict(path=str(p),sha256=sha(p),bytes=p.stat().st_size))
# 主输入按本轮运行前真实绑定逐项复核，不新造受验来源身份。
baseline=json.loads((META/'source-before.json').read_text());source_before=[]
for x in baseline['files']:
 p=ROOT/x['path'];s=p.lstat();got=dict(path=str(p),sha256=None if p.is_symlink() else sha(p),link=os.readlink(p) if p.is_symlink() else None,bytes=s.st_size,mode=stat.S_IMODE(s.st_mode))
 assert all(got[k]==x[k] for k in ['sha256','link','bytes','mode']),('main source changed',str(p))
 source_before.append(got)
protected_by_path={x['path']:x for x in protected}
for x in protected_by_path.values():assert Path(x['path']).stat().st_size==x['bytes'] and sha(Path(x['path']))==x['sha256'],('protected input changed',x['path'])
save('protected-before-execution.json',{'at':utc(),'sourceBaselineSha256':sha(META/'source-before.json'),'sourceFilesCompared':len(source_before),'protectedObjects':list(protected_by_path.values())})
# 从现状重新扫描正式报告、清单及原始日志引用，保留盘点与原证的区别。
prune={'.git','node_modules','target','debug','incremental','deps','copy','baseline','current','candidate','workspace','reference-copy','worktree'};files=[]
for base in [ROOT/'docs/rust-tauri',ROOT/'artifacts/rust-tauri',META]:
 for d,ds,fs in os.walk(base):
  ds[:]=[x for x in ds if x not in prune and not x.startswith('negcopy.') and not (Path(d)/x/'.git').exists()]
  for f in fs:
   p=Path(d)/f
   if p.is_file() and not p.is_symlink() and p.suffix in {'.json','.jsonl','.md','.txt','.tsv','.log','.gz'}:files.append(p)
matches=[]
for i in range(0,len(files),350):
 r=run(['rg','-l','-F','-a','-z','-f',str(OUT/'reference-patterns.txt'),'--',*map(str,files[i:i+350])]);assert r['exit'] in [0,1],r['stderr'];matches.extend(r['stdout'].splitlines())
for name in matches:
 p=Path(name)
 assert p.name=='owned-cache-boundary.json' and sha(p)=='25dd2c48430e85d063037c33e11191bdd6f19450798d53168143a0ff0923febd',('new formal or unclassified reference',name)
refcheck={'at':utc(),'filesScanned':len(files),'matches':[{'path':p,'sha256':sha(Path(p)),'classification':'CACHE_INVENTORY_ONLY'} for p in matches],'otherReferences':[]}
save('pre-delete-reference-check.json',refcheck)
for x in rows:
 p=Path(x['path']);s=p.lstat();b=p.read_bytes();head=struct.unpack('<4I',b[:16]);cmd=commands['commands'][x['sourceCommandKey']]
 assert p.parent==Path('/Users/study_superior/.cache/lingxi-r05-neg-target/debug/deps')
 assert p.name.startswith(x['sourceCommandKey']+'.') and '--test ' in cmd['command']
 assert head[0]==0xfeedfacf and head[3]==1 and b'/Users/study_superior/r05t08-work/negcopy.HwNE95/rust' in b
 assert not (p.parent/x['crateKey']).exists()
 assert (s.st_size,s.st_ino,s.st_dev,s.st_mode,s.st_mtime_ns,s.st_nlink)==(x['bytes'],x['inode'],x['device'],int(x['mode'],8),x['mtimeNs'],1)
 assert hashlib.sha256(b).hexdigest()==x['sha256']
ps=run(['ps','-axo','pid,ppid,stat,command']);active=[]
for l in ps['stdout'].splitlines():
 if any(s in l for s in ['rustc --crate-name','cargo test','r05_t08_negative_gate.sh','r05_t08_stage_suites.sh','observe-mutations.py','rr3-g-review-02-20261007/driver.py']) and 'cleanup.py' not in l:active.append(l)
ps['stdout']='\n'.join(active)
lsof=run(['/usr/sbin/lsof','-F','pftn','--',*[x['path'] for x in rows]])
save('pre-delete-process-check.json',{'at':utc(),'ps':ps,'lsof':lsof})
assert not active and lsof['exit']==1 and not lsof['stdout'] and not lsof['stderr'],'process holding candidate or build active'
receipt={'beforeAt':utc(),'freeBefore':free(),'finalCandidatesSha256':sha(OUT/'final-candidates.json'),'plannedCount':33,'plannedLogicalBytes':136063856,'actions':[],'exit':None}
save('execution-receipt.json',receipt)
for x in rows:
 p=Path(x['path']);s=p.lstat()
 assert (s.st_ino,s.st_mode,s.st_size,s.st_mtime_ns)==(x['inode'],int(x['mode'],8),x['bytes'],x['mtimeNs'])
 assert sha(p)==x['sha256']
 p.unlink()
 receipt['actions'].append({'at':utc(),'path':str(p),'sha256':x['sha256'],'bytes':x['bytes'],'inode':x['inode'],'operation':'unlink','result':'success','existsAfter':p.exists()})
 save('execution-receipt.json',receipt)
receipt.update(afterUnlinkAt=utc(),freeAfterUnlink=free())
checks=[]
for x in protected_by_path.values():
 p=Path(x['path']);checks.append({'path':str(p),'unchanged':p.is_file() and p.stat().st_size==x['bytes'] and sha(p)==x['sha256']})
for x in source_before:
 p=Path(x['path']);s=p.lstat();now=dict(sha256=None if p.is_symlink() else sha(p),link=os.readlink(p) if p.is_symlink() else None,bytes=s.st_size,mode=stat.S_IMODE(s.st_mode))
 checks.append({'path':str(p),'unchanged':all(now[k]==x[k] for k in now)})
held=[x for x in all_rows if x['decision']!='PROPOSE_EXACT_UNLINK_AFTER_ROOT_AUTHORIZATION']
for x in held:
 p=Path(x['path']);checks.append({'path':str(p),'unchanged':p.is_file() and p.stat().st_ino==x['inode'] and sha(p)==x['sha256']})
changes=[x for x in checks if not x['unchanged']]
save('protected-after-execution.json',{'at':utc(),'objectsChecked':len(checks),'unchanged':len(checks)-len(changes),'changes':changes,'checkScope':'1068 source inputs; G02 evidence; primary HEAD/index/ref; observed control/current binaries; fresh linked libraries; 37 held fragments. No claim of full .git or whole disk snapshot.'})
receipt.update(finalAt=utc(),freeAfterVerification=free(),observedImmediateFreeDelta=receipt['freeAfterUnlink']-receipt['freeBefore'],deletedCount=len(receipt['actions']),deletedLogicalBytes=sum(x['bytes'] for x in receipt['actions']),preservedCount=len(checks),preservedChanges=changes,exit=0 if not changes else 1)
save('execution-receipt.json',receipt)
assert not changes,changes
print(json.dumps({k:v for k,v in receipt.items() if k!='actions'},ensure_ascii=False,indent=2),flush=True)
