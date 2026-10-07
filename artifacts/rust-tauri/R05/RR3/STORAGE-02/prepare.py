# 只读证明本轮缓存归属；保留所有历史引用和不可确认对象。
from pathlib import Path
import os,sys,json,hashlib,subprocess,datetime,re,stat
ROOT=Path.cwd(); OUT=ROOT/'artifacts/rust-tauri/R05/RR3/STORAGE-02'; G=ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-01'; T=Path('/Users/study_superior/.cache/lingxi-r05-neg-target'); INC=T/'debug/incremental'; COPY=Path('/Users/study_superior/r05t08-work/negcopy.hTXzzN')
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(4*1024*1024),b''):h.update(b)
 return h.hexdigest()
def dump(n,d):(OUT/n).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def command(a):
 r=subprocess.run(a,capture_output=True,text=True);return {'argv':a,'at':now(),'exitCode':r.returncode,'stdout':r.stdout,'stderr':r.stderr}
def row(p):
 s=p.lstat();r={'path':str(p),'bytes':s.st_size,'allocatedBytes':s.st_blocks*512,'inode':s.st_ino,'links':s.st_nlink,'mode':s.st_mode,'mtimeNs':s.st_mtime_ns,'birthTime':s.st_birthtime}
 if p.is_symlink():r['symlink']=os.readlink(p)
 else:r['sha256']=sha(p)
 return r
checks=[command(['ps','-axo','pid,ppid,lstart,command']),command(['/usr/sbin/lsof','+D',str(INC)]),command(['df','-k',str(ROOT)])]
assert checks[1]['exitCode']==1 and not checks[1]['stdout'] and not checks[1]['stderr'], '目标存在打开文件，停止'
# 真实默认执行及已完成补证的时间区间，不用文件时间替代命令原证。
records=[];proof=[]
for p in [G/'default-command.json',*sorted((G/'supplements').glob('*/command.json'))]:
 d=json.loads(p.read_text()); small={k:v for k,v in d.items() if k not in ['inputBefore','inputAfter']}; small['receiptPath']=str(p);small['receiptSHA256']=sha(p);records.append(small)
 if d.get('startUTC') and d.get('endUTC'):proof.append((datetime.datetime.fromisoformat(d['startUTC']).timestamp(),datetime.datetime.fromisoformat(d['endUTC']).timestamp(),str(p)))
# 所有 G 日志中实际出现的编译和执行路径用来核 crate 身份。
compilelines=[]
for p in G.rglob('*.log'):
 for line in p.read_text(errors='replace').splitlines():
  if 'Compiling ' in line or ('Running ' in line and ('deps/' in line or 'tests/' in line)):compilelines.append({'log':str(p),'line':line})
dump('owner-proof.json',{'copy':str(COPY),'commands':records,'compileLines':compilelines,'driver':row(G/'run-supplements.py'),'defaultDriver':row(G/'run-default.py'),'copyScript':row(COPY/'scripts/rust-tauri/r05_t08_negative_gate.sh'),'copyPathReceipt':row(G/'copy-path.json'),'interruption':row(ROOT/'artifacts/rust-tauri/R05/RR3/TASK0/interruption-recovery-20261007.json')})
# 扫全部历史 manifest/digest/sha 原文，精确路径、文件名、SHA任一引用均保留。
refs=set(); hashes=set(); manifests=[]
def walk(v):
 if isinstance(v,dict):
  for k,x in v.items():walk(k);walk(x)
 elif isinstance(v,list):
  for x in v:walk(x)
 elif isinstance(v,str):
  refs.add(v)
  if len(v)<4096 and '/' in v:refs.add(v.rsplit('/',1)[-1])
  hashes.update(re.findall(r'\b[a-fA-F0-9]{64}\b',v.lower()))
for base in [ROOT/'artifacts/rust-tauri',ROOT/'docs/rust-tauri']:
 for p in base.rglob('*'):
  if OUT in p.parents or not p.is_file() or p.is_symlink() or not any(k in p.name.lower() for k in ('manifest','digest','sha256')):continue
  try:s=p.read_text()
  except (UnicodeError,OSError):continue
  manifests.append(row(p))
  try:walk(json.loads(s))
  except ValueError:
   refs.update(s.split());hashes.update(re.findall(r'\b[a-fA-F0-9]{64}\b',s.lower()))
dump('reference-scan.json',{'files':manifests,'uniqueReferenceTokens':len(refs),'uniqueHashes':len(hashes),'rule':'精确路径、完整incremental相对路径、文件名、内容SHA任一命中即保留'})
# 每个会话需要：真实copy内嵌路径 + 对应crate实际编译/执行原日志 + 真实已完成命令时间。
incrows=[]; sessions=[]; candidates=[]
for crate in sorted(INC.iterdir()):
 if not crate.is_dir():continue
 cname=crate.name.rsplit('-',1)[0]; matches=[x for x in compilelines if ('Compiling '+cname.replace('_','-')+' ') in x['line'] or ('tests/'+cname+'.rs') in x['line'] or ('deps/'+cname+'-') in x['line']]
 for session in sorted(crate.iterdir()):
  if not session.is_dir():
   if session.is_file() or session.is_symlink():incrows.append(row(session))
   continue
  sr=[]; embedded=[]
  for p in sorted(session.rglob('*')):
   if not p.is_file() and not p.is_symlink():continue
   r=row(p); sr.append(r);incrows.append(r)
   if p.suffix=='.o' and not p.is_symlink():
    data=p.read_bytes()
    if str(COPY).encode()+b'/rust' in data:embedded.append(str(p)); r['containsExactRr3Copy']=True
  ownership=bool(embedded and matches)
  detail={'session':str(session),'crate':cname,'ownershipEstablished':ownership,'embeddedSourceExamples':embedded[:3],'compileReferences':matches[:4],'totalFiles':len(sr),'totalBytes':sum(r['bytes'] for r in sr)}
  sessions.append(detail)
  for r in sr:
   p=Path(r['path']); intervals=[x[2] for x in proof if x[0]<=r['birthTime']<=x[1] and x[0]<=r['mtimeNs']/1e9<=x[1]]
   references=[]
   if str(p) in refs:references.append('absolute-path')
   if str(p.relative_to(T)) in refs:references.append('target-relative-path')
   if p.name in refs:references.append('filename')
   if r.get('sha256') in hashes:references.append('content-sha256')
   r['referenceMatches']=references
   eligible=ownership and intervals and not references and not (r['mode']&0o111) and 'symlink' not in r and (p.suffix=='.o' or p.name in ['query-cache.bin','dep-graph.bin','work-products.bin'])
   # .o 必须自己携带本次唯一copy路径，避免删会话中硬链接复用的RR2对象。
   if p.suffix=='.o' and not r.get('containsExactRr3Copy'):eligible=False
   if eligible:r['ownerSession']=str(session);r['completedCommandReferences']=intervals;candidates.append(r)
  print('checked',crate.name,session.name,flush=True)
# 全NEG_TARGET保留对象与旧G原证均前后摘要核对；不把正在变动主target纳入。
seen={r['path'] for r in incrows};preservedBase=list(incrows)
for base in [T,G,ROOT/'artifacts/rust-tauri/R05/RR3/G-INTERRUPTION-01',ROOT/'artifacts/rust-tauri/R05/RR3/STORAGE-01']:
 for p in sorted(base.rglob('*')):
  if (p.is_file() or p.is_symlink()) and str(p) not in seen:preservedBase.append(row(p));seen.add(str(p))
dump('all-before.json',preservedBase);dump('sessions.json',sessions);dump('candidates.json',candidates)
dump('before.json',{'at':now(),'freeBytes':os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize,'desiredFreeBytes':6*1024**3,'scope':str(INC),'checks':checks,'candidateFiles':len(candidates),'candidateLogicalBytes':sum(r['bytes'] for r in candidates),'allPreservationFiles':len(preservedBase),'manifestFiles':len(manifests)})
print(json.dumps(json.loads((OUT/'before.json').read_text())|{'checks':'see before.json'},ensure_ascii=False),flush=True)
