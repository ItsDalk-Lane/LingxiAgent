import os, re, json, hashlib, stat, struct, datetime, subprocess, gzip
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
OUT=Path('/private/tmp/rr3-storage03-20261007')
META=Path('/private/tmp/rr3-g-review-02-20261007')
TARGET=Path('/Users/study_superior/.cache/lingxi-r05-neg-target')
DEPS=TARGET/'debug/deps'
EV=ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-02'
LOG=EV/'default16-01/n02-zero-match/suites/build.log'
SOURCE=b'/Users/study_superior/r05t08-work/negcopy.HwNE95/rust'
START=1791347271.0
END=1791347315.0
def utc():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with open(p,'rb') as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def save(n,x):
 (OUT/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def ident(p):
 s=p.lstat()
 return dict(path=str(p),bytes=s.st_size,sha256=sha(p),mode=oct(s.st_mode),inode=s.st_ino,device=s.st_dev,nlink=s.st_nlink,mtimeNs=s.st_mtime_ns,birthUtc=datetime.datetime.fromtimestamp(s.st_birthtime,datetime.timezone.utc).isoformat(),mtimeUtc=datetime.datetime.fromtimestamp(s.st_mtime,datetime.timezone.utc).isoformat(),stBlocksBytes=s.st_blocks*512)
def run(argv):
 r=subprocess.run(argv,capture_output=True,text=True)
 return dict(argv=argv,exit=r.returncode,stdout=r.stdout,stderr=r.stderr)
log=LOG.read_text(); commands={}
for line_no,line in enumerate(log.splitlines(),1):
 m=re.search(r'process didn.t exit successfully: `([^`]+)`',line)
 if not m:continue
 cmd=m[1];a=cmd.split(); key=a[a.index('--crate-name')+1]+re.search(r'extra-filename=([^ ]+)',cmd)[1]
 commands[key]=dict(sourceLog=str(LOG),line=line_no,command=cmd,observedExit=101,crateSource=re.search(r'crates/[^ ]+\.rs',cmd)[0])
rows=[]
for key,cmd in commands.items():
 for p in sorted(DEPS.glob(key+'.*.o')):
  s=p.lstat()
  if not (START<=s.st_birthtime<=END and START<=s.st_mtime<=END):continue
  b=p.read_bytes(); header=struct.unpack('<4I',b[:16]) if len(b)>=16 else None
  object_type=bool(header and header[0]==0xfeedfacf and header[3]==1)
  source=SOURCE in b
  row=ident(p);row.update(crateKey=key,sourceCommandKey=key,header=b[:16].hex(),machOType='MH_OBJECT' if object_type else 'TRUNCATED_OR_EMPTY_UNKNOWN',sourceMarker=SOURCE.decode() if source else None,pairedBinaryExists=(DEPS/key).exists(),holderProcesses=[],referenceHits=[],decision='HOLD_UNPROVEN')
  if object_type and source and not s.st_mode&0o111 and s.st_nlink==1 and not row['pairedBinaryExists']:row['decision']='CANDIDATE_PENDING_REFERENCES'
  rows.append(row)
save('objects.json',rows)
save('source-commands.json',dict(at=utc(),buildLog=ident(LOG),commands=commands,sourceMarker=SOURCE.decode(),timeWindow={'startUtc':'2026-10-07T04:27:51Z','endUtc':'2026-10-07T04:28:35Z'},windowBasis='N02 producer observed at 04:27:51.503766; console final write 04:28:34.544201; exact failed rustc invocations and embedded unique source jointly required.'))
candidates=[x for x in rows if x['decision']=='CANDIDATE_PENDING_REFERENCES']
patterns=[]
for r in candidates:patterns.extend([Path(r['path']).name,r['sha256']])
(OUT/'reference-patterns.txt').write_text('\n'.join(patterns)+'\n')
# 仅扫描证据正文，跳过源副本、依赖与构建树；不新建全盘清单。
prune={'.git','node_modules','target','debug','incremental','deps','copy','baseline','current','candidate','workspace','reference-copy','worktree'}
refs=[];skipped=set();scan_bytes=0
for base in [ROOT/'docs/rust-tauri',ROOT/'artifacts/rust-tauri',META]:
 for d,ds,fs in os.walk(base):
  kept=[]
  for x in ds:
   p=Path(d)/x
   if x in prune or x.startswith('negcopy.') or (p/'.git').exists():skipped.add(x)
   else:kept.append(x)
  ds[:]=kept
  for f in fs:
   p=Path(d)/f
   if not p.is_symlink() and p.is_file() and p.suffix in {'.json','.jsonl','.md','.txt','.tsv','.log','.gz'}:
    refs.append(p);scan_bytes+=p.stat().st_size
matches=[];errors=[]
for i in range(0,len(refs),350):
 argv=['rg','-l','-F','-a','-z','-f',str(OUT/'reference-patterns.txt'),'--',*map(str,refs[i:i+350])]
 r=subprocess.run(argv,capture_output=True,text=True)
 if r.returncode not in [0,1]:errors.append(dict(exit=r.returncode,stderr=r.stderr))
 matches.extend(Path(x) for x in r.stdout.splitlines())
matched=[]
for p in matches:
 data=(gzip.open(p,'rt',errors='replace').read() if p.suffix=='.gz' else p.read_text(errors='replace'))
 record=dict(path=str(p),sha256=sha(p),hits=[])
 # STORAGE 的前后缓存盘点只证明存在，不是执行原件保留指令；其他引用一律先保留。
 inventory=('/STORAGE-02/' in str(p) and p.name in ['all-before.json.gz','preserved-after.jsonl.gz','before.json','candidates.json','candidates-initial-strict.json','protected-candidates.json'])
 for row in candidates:
  reasons=[]
  if Path(row['path']).name in data:reasons.append('exact_filename')
  if row['sha256'] in data:reasons.append('exact_sha256')
  if reasons:
   hit=dict(path=str(p),reason=reasons,classification='CACHE_INVENTORY_ONLY' if inventory else 'FORMAL_OR_UNCLASSIFIED_HOLD')
   row['referenceHits'].append(hit);record['hits'].append(dict(object=row['path'],**hit))
 matched.append(record)
ps=run(['ps','-axo','pid,ppid,stat,command'])
ps_lines=[]
for l in ps['stdout'].splitlines():
 if any(x in l for x in ['rustc --crate-name','cargo test','r05_t08_negative_gate.sh','r05_t08_stage_suites.sh','observe-mutations.py','rr3-g-review-02-20261007/driver.py']):
  if 'prepare.py' not in l:ps_lines.append(l)
ps['stdout']='\n'.join(ps_lines)
lsof=run(['/usr/sbin/lsof','-F','pftn','--',*[x['path'] for x in candidates]])
for row in candidates:
 if errors:row['decision']='HOLD_REFERENCE_SCAN_ERROR'
 elif ps_lines or lsof['exit']!=1 or lsof['stdout'] or lsof['stderr']:row['decision']='HOLD_PROCESS_CHECK'
 elif any(x['classification']!='CACHE_INVENTORY_ONLY' for x in row['referenceHits']):row['decision']='HOLD_REFERENCED'
 else:row['decision']='PROPOSE_EXACT_UNLINK_AFTER_ROOT_AUTHORIZATION'
save('objects.json',rows)
save('reference-check.json',dict(at=utc(),filesScanned=len(refs),physicalInputBytes=scan_bytes,scope=['docs/rust-tauri','artifacts/rust-tauri',str(META)],prunedDirectoryKinds=sorted(skipped),patternsSha256=sha(OUT/'reference-patterns.txt'),matchFiles=matched,errors=errors))
save('process-check.json',dict(at=utc(),ps=ps,lsof=lsof))
# 保留范围只作身份快照，不复制程序或树。
retained=[]
for p in DEPS.iterdir():
 if not p.is_file() or p.is_symlink():continue
 s=p.stat()
 if START<=s.st_birthtime<=END and START<=s.st_mtime<=END and (s.st_mode&0o111 or p.suffix in ['.rlib','.rmeta']):retained.append(ident(p))
control_paths={Path(json.loads(x)['path']) for x in (META/'binary-observations.jsonl').read_text().splitlines() if json.loads(x)['mtime_ns']/1e9>=1791347074}
controls=[]
for p in sorted(control_paths):controls.append(ident(p) if p.exists() else dict(path=str(p),exists=False,decision='PRESERVE_ABSENCE_AS_OBSERVED_AFTER_LINK_FAILURE'))
save('retained-binaries-and-libraries.json',dict(at=utc(),controls=controls,n02FreshRetained=retained,allOtherTargetObjects='OUT_OF_SCOPE_PRESERVE',temporaryRustcDirectories=[str(x) for x in DEPS.glob('rustc*') if x.is_dir()]))
receipt=json.loads((META/'cache-single-object-cleanup.json').read_text())
safe=[x for x in rows if x['decision']=='PROPOSE_EXACT_UNLINK_AFTER_ROOT_AUTHORIZATION']
summary=dict(at=utc(),status='READ_ONLY_READY_WAITING_ROOT',deletedByThisTask=0,previousAuthorizedReceipt=ident(META/'cache-single-object-cleanup.json'),previousAuthorizedPathExists=Path(receipt['path']).exists(),remainingFailedUnlinkedObjects=len(rows),remainingFailedUnlinkedLogicalBytes=sum(x['bytes'] for x in rows),proposedObjectCount=len(safe),proposedLogicalBytes=sum(x['bytes'] for x in safe),proposedStBlocksBytes=sum(x['stBlocksBytes'] for x in safe),actualPhysicalRelease='UNKNOWN_NOT_EXECUTED',heldObjectCount=len(rows)-len(safe),heldLogicalBytes=sum(x['bytes'] for x in rows if x not in safe),freeBytes=os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize,retainedFreshBinaryCount=sum(bool(int(x['mode'],8)&0o111) for x in retained),retainedFreshBinaryLogicalBytes=sum(x['bytes'] for x in retained if int(x['mode'],8)&0o111),retainedFreshLibraryLogicalBytes=sum(x['bytes'] for x in retained if not int(x['mode'],8)&0o111),sourceInputs='READ_ONLY',git='READ_ONLY',buildsExecuted=0)
save('summary.json',summary)
print(json.dumps(summary,ensure_ascii=False,indent=2),flush=True)
