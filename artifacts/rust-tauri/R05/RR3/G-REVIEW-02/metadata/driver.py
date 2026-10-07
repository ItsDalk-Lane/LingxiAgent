import os,sys,json,hashlib,subprocess,time,datetime,stat
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
OUT=Path('/private/tmp/rr3-g-review-02-20261007')
EV=ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-02'
TARGET=Path('/Users/study_superior/.cache/lingxi-r05-neg-target')
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with open(p,'rb') as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def save(n,x): (OUT/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def cmd(args):
 r=subprocess.run(args,cwd=ROOT,capture_output=True,text=True)
 return dict(argv=args,exit=r.returncode,stdout=r.stdout,stderr=r.stderr)
def snapshot(name):
 paths=[]
 for base in ['rust','scripts','docs/rust-tauri','contracts']:
  for d,ds,fs in os.walk(ROOT/base):
   ds[:]=[x for x in ds if x not in ['target','__pycache__','.git','node_modules']]
   paths.extend(Path(d)/x for x in fs if not x.endswith('.pyc'))
 paths.extend(p for p in ROOT.iterdir() if p.is_file())
 rows=[]
 for p in sorted(set(paths)):
  s=p.lstat();rows.append(dict(path=str(p.relative_to(ROOT)),sha256=sha(p) if not p.is_symlink() else None,link=os.readlink(p) if p.is_symlink() else None,bytes=s.st_size,mode=stat.S_IMODE(s.st_mode)))
 save(name,dict(at=utc(),files=rows,digest=hashlib.sha256(json.dumps(rows,sort_keys=True).encode()).hexdigest()))
 return rows
if EV.exists():raise SystemExit('refuse existing review output')
EV.mkdir()
snapshot('source-before.json')
save('preflight.json',dict(at=utc(),disk=os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize,checks=[cmd(x) for x in [['git','rev-parse','HEAD'],['git','branch','--show-current'],['git','status','--porcelain=v1','-uall'],['git','diff','--stat'],['/Users/study_superior/.cargo/bin/cargo','--version'],['/Users/study_superior/.cargo/bin/rustc','-vV'],['node','--version'],['npm','--version'],['uname','-a'],['ps','-axo','pid,ppid,stat,command']]],gitIndex=sha(ROOT/'.git/index')))
# 仓外保存动态观察，避免让观察记录成为运行中的候选输入。
args=['bash','scripts/rust-tauri/r05_t08_negative_gate.sh','artifacts/rust-tauri/R05/RR3/G-REVIEW-02/default16-01']
env=os.environ.copy();inherited=env.pop('R02_LEGACY_REGRESSION_MODE',None);env['CARGO_INCREMENTAL']='0';env['PYTHONDONTWRITEBYTECODE']='1'
start=utc();save('command-start.json',dict(argv=args,cwd=str(ROOT),start=start,environment={'CARGO_INCREMENTAL':'0','PYTHONDONTWRITEBYTECODE':'1','R02_LEGACY_REGRESSION_MODE':None},removedInheritedLegacyMode=inherited))
seen={};lastps='';lastcopy=None;lastmutation=None
with open(OUT/'default-console.log','w') as console,open(OUT/'process-observations.jsonl','w') as obs,open(OUT/'binary-observations.jsonl','w') as bins:
 p=subprocess.Popen(args,cwd=ROOT,env=env,stdout=console,stderr=subprocess.STDOUT)
 save('process-start.json',dict(at=utc(),pid=p.pid))
 while p.poll() is None:
  ps=subprocess.run(['ps','-axo','pid,ppid,stat,command'],capture_output=True,text=True).stdout
  filtered='\n'.join(x for x in ps.splitlines() if any(t in x for t in ['lingxi-r05-neg-target','r05t08-work/negcopy.','r05_t08_negative_gate.sh','r05_t08_stage_suites.sh','r02_t08_legacy_entry_regression.sh']))
  if filtered!=lastps:obs.write(json.dumps(dict(at=utc(),processes=filtered,freeBytes=os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize))+'\n');obs.flush();lastps=filtered
  for d in [TARGET/'debug',TARGET/'debug/deps']:
   if not d.exists():continue
   for b in d.iterdir():
    try:
     s=b.stat()
     if not b.is_file() or not s.st_mode&0o111:continue
     key=(s.st_mtime_ns,s.st_size,s.st_ino)
     if seen.get(str(b))==key:continue
     digest=sha(b);seen[str(b)]=key
     bins.write(json.dumps(dict(at=utc(),path=str(b),mtime_ns=s.st_mtime_ns,bytes=s.st_size,sha256=digest))+'\n');bins.flush()
    except FileNotFoundError:pass
  time.sleep(2)
 rc=p.wait()
save('command.json',dict(argv=args,cwd=str(ROOT),start=start,end=utc(),exit=rc,consoleSha256=sha(OUT/'default-console.log'),environment={'CARGO_INCREMENTAL':'0','R02_LEGACY_REGRESSION_MODE':None}))
snapshot('source-after-default.json')
save('post-default.json',dict(at=utc(),freeBytes=os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize,gitIndex=sha(ROOT/'.git/index'),processes=cmd(['ps','-axo','pid,ppid,stat,command'])))
print(json.dumps(dict(exit=rc,end=utc(),console=str(OUT/'default-console.log'))),flush=True)
