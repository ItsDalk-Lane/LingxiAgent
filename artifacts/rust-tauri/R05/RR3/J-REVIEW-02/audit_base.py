import os,json,subprocess,hashlib,stat,datetime
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent');EV=ROOT/'artifacts/rust-tauri/R05/RR3/J-REVIEW-02';p=EV/'legacy-directed/legacy-entry/e0-base-materialization';r=json.loads((p/'result.json').read_text());base=Path(r['copy']);candidate=Path(r['source']);env=dict(os.environ,GIT_OPTIONAL_LOCKS='0');start=datetime.datetime.now(datetime.timezone.utc).isoformat()
def git(root,*args):return subprocess.check_output(['git','-C',str(root),*args],env=env)
expected={}
for row in git(ROOT,'ls-tree','-rlz',r['commit']).split(b'\0'):
 if row:
  fields,path=row.split(b'\t',1);mode,kind,oid,size=fields.split();expected[os.fsdecode(path)]=(mode.decode(),oid.decode(),int(size))
rows=json.loads((p/'tracked-files.json').read_text());assert len(rows)==len(expected)
for row in rows:assert (row['mode'],row['oid'],row['size'])==expected[row['path']]
result={'startedUtc':start,'base':str(base),'candidate':str(candidate),'baseCommit':r['commit'],'entries':len(expected),'bytes':sum(v[2] for v in expected.values()),'methods':r['methods'],'manifestMatchesRealHistoricalTree':True}
# 在原进程清理临时基线前独立读取实物；若已清理，不能冒称亲验实物。
result['physicalBaseAvailable']=base.exists()
if base.exists():
 count=0
 for name,(mode,oid,size) in expected.items():
  f=base/name;st=f.lstat();h=hashlib.sha1();h.update(f'blob {st.st_size}\0'.encode())
  with f.open('rb') as stream:
   while block:=stream.read(1024*1024):h.update(block)
  assert h.hexdigest()==oid,name;assert stat.S_IMODE(st.st_mode)==(0o755 if mode=='100755' else 0o644),name
  source=candidate/name
  if source.exists():
   a=source.stat();assert (a.st_dev,a.st_ino)!=(st.st_dev,st.st_ino),name
  count+=1
 assert git(base,'rev-parse','HEAD').decode().strip()==r['commit']
 assert git(base,'diff','--cached','--exit-code')==b''
 assert git(base,'diff','--exit-code',r['commit'])==b''
 assert git(base,'status','--porcelain=v1','-z','--untracked-files=all')==b''
 result.update(physicalFilesVerified=count,exactHead=True,indexClean=True,worktreeDiffEmpty=True,residueZero=True)
result['finishedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();(EV/'historical-base-independent.json').write_text(json.dumps(result,indent=2)+'\n');print(result)
