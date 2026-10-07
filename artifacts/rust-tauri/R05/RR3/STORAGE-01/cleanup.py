# 仅按已核验的逐文件清单删除；任何归属或原件变化立即中止。
from pathlib import Path
import json,os,hashlib,datetime,subprocess
ROOT=Path.cwd();OUT=ROOT/'artifacts/rust-tauri/R05/RR3/STORAGE-01'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def free():
 s=os.statvfs(ROOT);return s.f_bavail*s.f_frsize
def dump(n,d):(OUT/n).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
b=json.loads((OUT/'before.json').read_text());checks=[]
for root in b['roots']:
 p=subprocess.run(['/usr/sbin/lsof','+D',root],capture_output=True,text=True)
 checks.append({'argv':['/usr/sbin/lsof','+D',root],'at':now(),'exitCode':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
 assert p.returncode==1 and not p.stdout and not p.stderr,'候选路径仍被打开或查询异常'
ps=subprocess.run(['ps','-axo','pid,ppid,lstart,command'],capture_output=True,text=True,check=True)
checks.append({'argv':['ps','-axo','pid,ppid,lstart,command'],'at':now(),'stdout':ps.stdout,'stderr':ps.stderr,'exitCode':ps.returncode})
for root in b['roots']:
 assert root not in ps.stdout and root.replace('/private/tmp/','/tmp/') not in ps.stdout,'候选路径存在活跃进程'
for row in b['candidates']:
 p=Path(row['path']);s=p.lstat();assert not p.is_symlink() and str(p.resolve())==str(p)
 assert s.st_ino==row['inode'] and s.st_size==row['bytes'] and s.st_mtime_ns==row['mtimeNs'] and sha(p)==row['sha256'],'删除前原件发生变化'
start=now();before=free();deleted=[];stop='ALL_SAFE_CANDIDATES_EXHAUSTED'
dump('pre-delete-checks.json',{'at':start,'freeBytes':before,'checks':checks,'allCandidateHashesMatched':True})
with (OUT/'deleted-files.jsonl').open('x') as log:
 for row in b['candidates']:
  if free()>=b['desiredFreeBytes']:stop='ENOUGH_SPACE';break
  p=Path(row['path']);p.unlink();entry={**row,'deletedAt':now()};log.write(json.dumps(entry,ensure_ascii=False)+'\n');log.flush();deleted.append(entry)
afterDelete=free();verified=[];mismatches=[]
for row in b['preserved']:
 p=Path(row['path'])
 if 'symlink' in row:actual={'symlink':os.readlink(p)} if p.is_symlink() else {'missing':True};matched=actual.get('symlink')==row['symlink']
 else:
  actual={'bytes':p.stat().st_size,'sha256':sha(p)} if p.is_file() else {'missing':True};matched=actual.get('sha256')==row['sha256'] and actual.get('bytes')==row['bytes']
 verified.append({'path':row['path'],**actual,'matchesBefore':matched})
 if not matched:mismatches.append(verified[-1])
dump('preserved-after.json',verified)
allocated={}
for row in deleted:allocated.setdefault(row['inode'],row['allocatedBytes'])
result={'startedAt':start,'finishedAt':now(),'operation':'unlink only prelisted unreferenced non-executable incremental or deps .rlib/.rmeta/.o intermediate files','stopReason':stop,'deletedFileCount':len(deleted),'deletedLogicalBytes':sum(x['bytes'] for x in deleted),'deletedUniqueInodeAllocatedBytes':sum(allocated.values()),'freeBytesImmediatelyBefore':before,'freeBytesImmediatelyAfterDeletion':afterDelete,'observedFreeDeltaBytes':afterDelete-before,'observedFreeDeltaCaveat':'其他并行工作也在同一磁盘写入；净可用空间差值不是缓存独占物理释放量。APFS共享块与硬链接也使文件长度不等于实际释放。','freeBytesAfterVerification':free(),'cloneRequiredBytes':b['cloneRequiredBytes'],'desiredFreeBytes':b['desiredFreeBytes'],'cloneShortfallBytes':max(0,b['cloneRequiredBytes']-free()),'desiredShortfallBytes':max(0,b['desiredFreeBytes']-free()),'safeToStartNextClone':free()>=b['desiredFreeBytes'] and not mismatches,'preservedFileCount':len(verified),'preservedMismatches':mismatches,'excluded':b['excluded'],'allDeletedPathsAbsent':all(not Path(x['path']).exists() for x in deleted),'reportState':'SPACE_INSUFFICIENT' if free()<b['desiredFreeBytes'] else 'SPACE_READY'}
dump('after.json',result)
print(json.dumps(result,ensure_ascii=False,indent=2))
