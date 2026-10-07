# 只删除已证明归属且无历史引用的增量中间对象；达到实际空间目标立即停止。
from pathlib import Path
import os,json,hashlib,subprocess,datetime,stat,time,struct,gzip
ROOT=Path.cwd(); OUT=ROOT/'artifacts/rust-tauri/R05/RR3/STORAGE-02'; INC=Path('/Users/study_superior/.cache/lingxi-r05-neg-target/debug/incremental'); TARGET=INC.parent.parent; GOAL=6*1024**3
COPY=b'/Users/study_superior/r05t08-work/negcopy.hTXzzN/rust'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(4*1024*1024),b''):h.update(b)
 return h.hexdigest()
def dump(n,d):(OUT/n).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def free():s=os.statvfs(ROOT);return s.f_bavail*s.f_frsize
def run(a):
 p=subprocess.run(a,capture_output=True,text=True);return {'at':now(),'argv':a,'exitCode':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
checks=[]
def active():
 a=run(['/usr/sbin/lsof','+D',str(INC)]); b=run(['ps','-axo','pid,ppid,lstart,command']);checks.extend([a,b]);dump('active-checks.json',checks)
 assert a['exitCode']==1 and not a['stdout'] and not a['stderr'],'打开文件复核失败'
 forbidden=[x for x in b['stdout'].splitlines() if str(TARGET) in x and any(y in x for y in ['/rustc ','/cargo ','r05_t08_negative_gate.sh'])]
 assert not forbidden,forbidden
 return a['at']
candidates=json.loads((OUT/'candidates.json').read_text()); before=json.load(gzip.open(OUT/'all-before.json.gz','rt')); sessions={x['session']:x for x in json.loads((OUT/'sessions.json').read_text())}
# 同inode的两个会话硬链接一起处理；不删除无归属的其他链接。
groups={}
for r in candidates:groups.setdefault(r['inode'],[]).append(r)
groups=sorted(groups.values(),key=lambda a:sum(r['allocatedBytes'] for r in a),reverse=True)
start=now();f0=free();lastCheck=active();lastCheckTime=time.monotonic();lastFree=f0;deleted=[];skipped=[]
with (OUT/'deleted-files.jsonl').open('x') as log:
 for group in groups:
  if free()>=GOAL:break
  if time.monotonic()-lastCheckTime>15 or free()-lastFree>512*1024**2:lastCheck=active();lastCheckTime=time.monotonic();lastFree=free()
  for r in group:
   if free()>=GOAL:break
   p=Path(r['path']); assert INC in p.parents and not p.is_symlink() and p.resolve()==p
   s=p.stat(); assert stat.S_ISREG(s.st_mode) and not s.st_mode&0o111
   assert s.st_ino==r['inode'] and s.st_size==r['bytes'] and s.st_mtime_ns==r['mtimeNs'] and sha(p)==r['sha256']
   assert not r['referenceMatches'] and r['completedCommandReferences'] and sessions[r['ownerSession']]['ownershipEstablished']
   if p.suffix=='.o':
    data=p.read_bytes()
    assert sessions[r['ownerSession']]['embeddedSourceExamples']
    if r.get('containsExactRr3Copy'):assert COPY in data
    # MH_OBJECT，明确拒绝实际可执行 Mach-O 及其他格式。
    if len(data)<16 or data[:4]!=b'\xcf\xfa\xed\xfe' or struct.unpack('<I',data[12:16])[0]!=1:
     skipped.append({'path':str(p),'reason':'not-confirmed-MH_OBJECT'});continue
   else:assert p.name in ['query-cache.bin','dep-graph.bin','work-products.bin']
   entry=r|{'deletedAtUTC':now(),'activeCheckAtUTC':lastCheck,'operation':'unlink-only-this-file','fileType':'MH_OBJECT' if p.suffix=='.o' else 'rust-incremental-data'}
   p.unlink(); log.write(json.dumps(entry,ensure_ascii=False)+'\n');log.flush();os.fsync(log.fileno());deleted.append(entry)
  if len(deleted)%250==0:print('deleted',len(deleted),'freeGiB',free()/1024**3,flush=True)
f1=free();active(); paths={r['path'] for r in deleted};preserved=[];mismatches=[]
for r in before:
 if r['path'] in paths:continue
 p=Path(r['path'])
 if not p.exists() and not p.is_symlink():a={'path':str(p),'missing':True}
 elif 'symlink' in r:a={'path':str(p),'symlink':os.readlink(p)}
 else:a={'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)}
 ok=(a.get('symlink')==r['symlink']) if 'symlink' in r else a.get('bytes')==r['bytes'] and a.get('sha256')==r['sha256']
 a['unchanged']=ok;preserved.append(a)
 if not ok:mismatches.append(a)
with gzip.open(OUT/'preserved-after.json.gz','wt',compresslevel=6) as f:json.dump(preserved,f,ensure_ascii=False)
f2=free();dump('after.json',{'startedAtUTC':start,'completedAtUTC':now(),'state':'SPACE_READY' if f2>=GOAL and not mismatches else 'SPACE_INSUFFICIENT_OR_MISMATCH','stopReason':'ACTUAL_FREE_TARGET_REACHED' if f1>=GOAL else 'ALL_PROVEN_UNREFERENCED_CANDIDATES_EXHAUSTED','freeBytesBefore':f0,'freeBytesImmediatelyAfter':f1,'freeBytesAfterVerification':f2,'observedDiskNetDeltaBytes':f1-f0,'desiredFreeBytes':GOAL,'deletedFiles':len(deleted),'deletedLogicalBytes':sum(r['bytes'] for r in deleted),'deletedUniqueInodeAllocatedBytes':sum(max(r['allocatedBytes'] for r in group) for group in groups if all(r['path'] in paths for r in group) and group[0]['links']==len(group)),'preservedFiles':len(preserved),'preservedMismatches':mismatches,'allDeletedPathsAbsent':all(not Path(x).exists() for x in paths),'skipped':skipped,'remainingCandidateFiles':len(candidates)-len(deleted),'note':'APFS共享块/硬链接与同盘主target并行构建会影响净差；逻辑长度及st_blocks不等于独占物理释放。6GiB是clone3.5GiB加编译余量的准备目标，不是实测峰值。'})
print((OUT/'after.json').read_text(),flush=True)
assert not mismatches
