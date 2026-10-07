# 仅清点已结束 RR3 包的可重建缓存，逐文件保留原证。
from pathlib import Path
import os,json,hashlib,subprocess,datetime,stat,re
ROOT=Path.cwd(); OUT=ROOT/'artifacts/rust-tauri/R05/RR3/STORAGE-01'; BASE=ROOT/'artifacts/rust-tauri/R05/RR3'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def dump(n,d):(OUT/n).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def command(a):
 p=subprocess.run(a,capture_output=True,text=True);return {'argv':a,'at':now(),'exitCode':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
roots=[Path('/tmp/lingxi-rr3-a-review-01-target').resolve(),BASE/'I-REVIEW-01/permanent-final/own-target']
# 全部历史 manifest/digest 中的原路径和摘要均作为保留条件。
refs=set();hashes=set();manifests=[]
def walk(v,parent):
 if isinstance(v,dict):
  for k,x in v.items():walk(k,parent);walk(x,parent)
 elif isinstance(v,list):
  for x in v:walk(x,parent)
 elif isinstance(v,str):
  refs.add(v)
  if re.fullmatch('[a-fA-F0-9]{64}',v):hashes.add(v.lower())
  if '/' in v and len(v)<4096:
   try:refs.add(str((parent/v).resolve()))
   except (OSError,ValueError):pass
for p in (ROOT/'artifacts/rust-tauri').rglob('*'):
 if not p.is_file() or p.is_symlink() or OUT in p.parents:continue
 if not any(k in p.name.lower() for k in ('manifest','digest','sha256')):continue
 try:s=p.read_text()
 except (UnicodeError,OSError):continue
 manifests.append({'path':str(p.relative_to(ROOT)),'sha256':sha(p),'bytes':p.stat().st_size})
 try:walk(json.loads(s),p.parent)
 except ValueError:
  refs.update(s.split());hashes.update(re.findall(r'\b[a-fA-F0-9]{64}\b',s))
candidates=[];protected=[]
for root in roots:
 for p in root.rglob('*'):
  if not p.is_file() or p.is_symlink():continue
  rel=p.relative_to(root);st=p.stat()
  eligible=('incremental' in rel.parts or (len(rel.parts)==3 and rel.parts[:2]==('debug','deps') and p.suffix in ('.rlib','.rmeta','.o'))) and not (st.st_mode&0o111)
  if not eligible:continue
  digest=sha(p)
  row={'path':str(p),'ownerRoot':str(root),'bytes':st.st_size,'allocatedBytes':st.st_blocks*512,'inode':st.st_ino,'links':st.st_nlink,'sha256':digest,'mtimeNs':st.st_mtime_ns}
  if str(p) in refs or str(p).replace('/private/tmp/','/tmp/') in refs or p.name in refs or digest in hashes:protected.append(row)
  else:candidates.append(row)
commands= [command(['ps','-axo','pid,ppid,lstart,command']),command(['df','-k',str(ROOT)])]
for root in roots:commands.append(command(['/usr/sbin/lsof','+D',str(root)]))
proof=[]
for name in ['A-REVIEW-01','I-REVIEW-01']:
 p=BASE/name/('commands.json' if name.startswith('A') else 'permanent-final/commands.json')
 d=json.loads(p.read_text());rows=d if isinstance(d,list) else d.get('commands',[])
 proof.append({'owner':name,'commandFile':str(p.relative_to(ROOT)),'commandFileSha256':sha(p),'completedReport':str((BASE/name/'REVIEW.md').relative_to(ROOT)),'reportSha256':sha(BASE/name/'REVIEW.md'),'targetCommands':[x for x in rows if any(str(r) in json.dumps(x) or str(r).replace('/private/tmp/','/tmp/') in json.dumps(x) for r in roots)]})
# 原证目录和外部 target 中所有保留文件的完整摘要。
preserved=[]
for root in [BASE/'A-REVIEW-01',BASE/'I-REVIEW-01',roots[0]]:
 for p in root.rglob('*'):
  if p.is_symlink():preserved.append({'path':str(p),'symlink':os.readlink(p)});continue
  if p.is_file() and str(p) not in {x['path'] for x in candidates}:preserved.append({'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)})
usage=os.statvfs(ROOT)
r={'at':now(),'freeBytes':usage.f_bavail*usage.f_frsize,'cloneRequiredBytes':int(3.5*1024**3),'desiredFreeBytes':int(4.5*1024**3),'desiredBasis':'3.5 GiB independent clone plus 1 GiB conservative working margin; cannot promise future build peak','roots':[str(x) for x in roots],'ownerProof':proof,'manifestScan':manifests,'protectedCandidates':protected,'candidates':candidates,'commands':commands,'preserved':preserved,'excluded':['rust/target','/Users/study_superior/.cache/lingxi-r05-neg-target','all G packages','all H-REVIEW-02','all I-01 cache files named in I-01/manifest.json','all RR1/RR2/user/system data']}
dump('before.json',r)
print(json.dumps({'freeGiB':r['freeBytes']/1024**3,'candidateFiles':len(candidates),'candidateLogicalMiB':sum(x['bytes'] for x in candidates)/1024**2,'protectedCandidates':len(protected),'preservedFiles':len(preserved),'manifestFilesRead':len(manifests),'lsof':[{'exitCode':x['exitCode'],'stdout':x['stdout'],'stderr':x['stderr']} for x in commands[2:]]},ensure_ascii=False,indent=2))
