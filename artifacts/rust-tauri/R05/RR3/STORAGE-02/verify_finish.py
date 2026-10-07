# 总控收紧空间目标后仅核验，不再执行任何删除。
from pathlib import Path
import os,gzip,json,hashlib,subprocess,datetime
R=Path.cwd();O=R/'artifacts/rust-tauri/R05/RR3/STORAGE-02'; T=Path('/Users/study_superior/.cache/lingxi-r05-neg-target');I=T/'debug/incremental'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def free():s=os.statvfs(R);return s.f_bavail*s.f_frsize
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(4*1024*1024),b''):h.update(b)
 return h.hexdigest()
def dump(n,v):(O/n).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
start=now();preFree=free();before=json.load(gzip.open(O/'all-before.json.gz','rt'));deleted=[json.loads(x) for x in (O/'deleted-files.jsonl').read_text().splitlines()];paths={x['path'] for x in deleted};mismatches=[];n=0
with gzip.open(O/'preserved-after.jsonl.gz','wt',compresslevel=6) as f:
 for r in before:
  if r['path'] in paths:continue
  p=Path(r['path']);a={'path':str(p),'beforeSHA256':r.get('sha256')}
  if not p.exists() and not p.is_symlink():a['missing']=True;ok=False
  elif 'symlink' in r:a['symlink']=os.readlink(p);ok=a['symlink']==r['symlink']
  else:a.update(bytes=p.stat().st_size,sha256=sha(p));ok=a['bytes']==r['bytes'] and a['sha256']==r['sha256']
  a['unchanged']=ok;f.write(json.dumps(a,ensure_ascii=False)+'\n');n+=1
  if not ok:mismatches.append(a)
  if n%100000==0:print('verified',n,flush=True)
checks=[]
for argv in [['ps','-axo','pid,ppid,lstart,command'],['/usr/sbin/lsof','+D',str(I)],['df','-k',str(R)]]:
 p=subprocess.run(argv,capture_output=True,text=True);checks.append({'at':now(),'argv':argv,'exitCode':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
dump('final-checks.json',checks)
base=json.loads((O/'before.json').read_text());groups={}
for d in deleted:groups.setdefault(d['inode'],[]).append(d)
result={'verificationStartUTC':start,'verificationEndUTC':now(),'state':'MINIMUM_RECLAIM_COMPLETE_ORIGINAL_6GIB_NOT_REACHED','stopReason':'CONTROLLER_REDUCED_SPACE_REQUIREMENT; deletion had already finished before message; immediately interrupted running verifier then resumed verification-only','firstDeletedAtUTC':deleted[0]['deletedAtUTC'] if deleted else None,'lastDeletedAtUTC':deleted[-1]['deletedAtUTC'] if deleted else None,'deletedFiles':len(deleted),'deletedLogicalBytes':sum(x['bytes'] for x in deleted),'deletedUniqueInodeAllocatedBytes':sum(max(x['allocatedBytes'] for x in g) for g in groups.values()),'allocatedBytesOfFullyUnlinkedInodes':sum(max(x['allocatedBytes'] for x in g) for g in groups.values() if g[0]['links']==len(g)),'preparedSnapshotFreeBytes':base['freeBytes'],'preparedSnapshotAtUTC':base['at'],'freeBytesAtVerificationRestart':preFree,'freeBytesAfterVerification':free(),'observedOverallDiskFreeDeltaBytes':free()-base['freeBytes'],'diskDeltaCaveat':'准备末尾空间快照后压缩了本任务自建大型回执，另有主target并行编译；立即删除前/后数值在中断时尚未落盘，不能把总体净差冒称删除独占物理释放。逻辑长度、st_blocks受硬链接/APFS共享块影响。','originalDesiredFreeBytes':6*1024**3,'originalGoalReached':free()>=6*1024**3,'preservedFiles':n,'preservedMismatches':mismatches,'allDeletedPathsAbsent':all(not Path(x).exists() for x in paths),'verificationOnlyAfterStop':True,'refinedSelection':json.loads((O/'refined-selection.json').read_text())}
dump('after.json',result);print(json.dumps(result,ensure_ascii=False,indent=2),flush=True)
assert not mismatches
